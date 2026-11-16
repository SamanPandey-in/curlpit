//! Sending one request. `execute` is the whole off-thread pipeline (pre-request script,
//! build, send, test script, history) and is also what the collection runner drives; the
//! `Workspace` methods here start a send, fold the result back into its tab, and keep
//! whatever its scripts changed.

use crate::response::{content_type, ViewData};
use crate::script::{
    merge_script_vars, script_input, script_response, take_script_request, take_script_vars,
    to_var_map, ScriptReport,
};
use crate::sidebar::SideView;
use crate::ui::Status;
use crate::workspace::{sendable, SendOutcome, Workspace};
use at_core::{build, Environment, Request, Variables};
use at_history::{now_ms, History, Record};
use at_http::{send, CookieJar, EngineConfig, HttpError, ResponseBody};
use at_script::{Phase, ScriptInput, VarMap};
use gpui::{Context, Window};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// What history keeps of one send. `request` is the request as typed (variables unresolved).
pub(crate) fn history_record(
    request: Request,
    result: &Result<at_http::HttpResponse, HttpError>,
    elapsed: Duration,
) -> Record {
    match result {
        Ok(resp) => Record {
            request,
            status: Some(resp.status),
            status_text: resp.status_text.clone(),
            elapsed_ms: resp.elapsed.as_millis() as u64,
            size: resp.size,
            error: None,
            resp_headers: resp.headers.clone(),
            resp_body: match &resp.body {
                // Copy at most one byte more than history keeps, so it can tell the body was cut.
                ResponseBody::Memory(b) => {
                    Some(b[..b.len().min(at_history::DEFAULT_MAX_BODY + 1)].to_vec())
                }
                ResponseBody::File { .. } => None,
            },
        },
        Err(e) => Record {
            request,
            elapsed_ms: elapsed.as_millis() as u64,
            error: Some(e.to_string()),
            ..Default::default()
        },
    }
}

/// One request through the whole pipeline, off the UI thread: pre-request script -> build ->
/// send -> test script (scripts on the blocking pool so a slow one never stalls networking).
/// Returns what happened plus the variables after its scripts, so a run can chain them.
/// `history: None` skips recording (the collection runner doesn't fill History).
pub(crate) async fn execute(
    mut req: Request,
    mut vars: Variables,
    env_name: Option<String>,
    token: CancellationToken,
    history: Option<Arc<Mutex<History>>>,
    cookies: Arc<CookieJar>,
) -> (SendOutcome, Variables) {
    let env_before = to_var_map(&vars.environment);
    let globals_before = to_var_map(&vars.global);
    let run = |src: String, phase: Phase, input: ScriptInput| {
        tokio::task::spawn_blocking(move || {
            at_script::run(&src, phase, &input, at_script::Limits::default())
        })
    };

    let pre = if req.pre_request_script.trim().is_empty() {
        None
    } else {
        let input = script_input(&req, &vars, env_name.clone(), None);
        let before = input.request.clone();
        let out = run(req.pre_request_script.clone(), Phase::PreRequest, input)
            .await
            .ok();
        if let Some(o) = &out {
            take_script_vars(&mut vars, o);
            take_script_request(&mut req, &before, &o.request);
        }
        out
    };

    let req = sendable(req, &vars);
    let built = match build(&req, &vars) {
        Ok(b) => b,
        Err(e) => {
            let out = SendOutcome {
                sent: Err(e.to_string()),
                unresolved: Vec::new(),
                pre,
                test: None,
                env_before,
                globals_before,
                hist_err: None,
            };
            return (out, vars);
        }
    };
    let started = Instant::now();
    let engine = EngineConfig {
        cookies: Some(cookies),
        ..Default::default()
    };
    let r = send(&built, &req.settings, &engine, &token).await;

    let test = match &r {
        Ok(resp) if !req.test_script.trim().is_empty() => {
            let input = script_input(&req, &vars, env_name, Some(script_response(resp)));
            let out = run(req.test_script.clone(), Phase::Test, input).await.ok();
            if let Some(o) = &out {
                take_script_vars(&mut vars, o);
            }
            out
        }
        _ => None,
    };

    // Record on the network thread so the UI never waits for the disk.
    let hist_err = history.and_then(|h| {
        let rec = history_record(req, &r, started.elapsed());
        match h.lock() {
            Ok(mut h) => h.add(&rec, now_ms()).err().map(|e| e.to_string()),
            Err(_) => Some("history is unavailable".to_string()),
        }
    });
    let out = SendOutcome {
        sent: Ok(r),
        unresolved: built.unresolved,
        pre,
        test,
        env_before,
        globals_before,
        hist_err,
    };
    (out, vars)
}

impl Workspace {
    /// Send the active tab's request, or cancel it if one is already in flight.
    pub(crate) fn send_active(&mut self, cx: &mut Context<Self>) {
        let id = self.tab().id;
        self.send_tab(id, cx);
    }

    /// Send a tab's request, or cancel it if one is already in flight.
    pub(crate) fn send_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let vars = self.variables();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if let Some(token) = tab.cancel.take() {
            token.cancel();
            return;
        }
        let req = tab.current_request(cx);
        tab.copied = false;
        let token = CancellationToken::new();
        tab.cancel = Some(token.clone());
        tab.status = Status::Loading;
        cx.notify();

        let env_name = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.name.clone());
        let (tx, rx) = async_channel::bounded(1);
        let history = self.history.clone();
        let cookies = self.cookies.clone();
        self.rt.spawn(async move {
            let (out, _) = execute(req, vars, env_name, token, history, cookies).await;
            let _ = tx.send(out).await;
        });

        cx.spawn(async move |this, cx| {
            if let Ok(out) = rx.recv().await {
                this.update(cx, |this, cx| this.finish_send(id, out, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Show a finished send in its tab and keep what its scripts changed.
    pub(crate) fn finish_send(&mut self, id: u64, out: SendOutcome, cx: &mut Context<Self>) {
        let SendOutcome {
            sent,
            unresolved,
            pre,
            test,
            env_before,
            globals_before,
            hist_err,
        } = out;
        let mut notes = Vec::new();
        // The last script that ran holds the final variables.
        let last = test.as_ref().or(pre.as_ref());
        if let Some(o) = last {
            if let Some(n) =
                self.keep_script_vars(&env_before, &o.environment, &globals_before, &o.globals)
            {
                notes.push(n);
            }
        }
        // The tab may have been closed while the request was running.
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
            t.cancel = None;
            match sent {
                Ok(result) => {
                    t.apply(result);
                    t.warning = (!unresolved.is_empty())
                        .then(|| format!("Unresolved variables: {}", unresolved.join(", ")));
                }
                Err(e) => {
                    t.status = Status::Error(e);
                    t.warning = None;
                }
            }
            t.report =
                (pre.is_some() || test.is_some()).then_some(ScriptReport { pre, test, notes });
            if let Some(path) = t.loading_file.take() {
                let generation = t.view_gen;
                self.load_body_file(id, generation, path, cx);
            }
        }
        self.save_cookies_if_changed();
        if let Some(e) = hist_err {
            self.hist_error = Some(format!("Not saved to history: {e}"));
        }
        if self.side_view == SideView::History {
            self.refresh_history(cx);
        }
        cx.notify();
    }

    /// Save the variable changes scripts made to the active environment and the globals.
    /// Returns a note when something couldn't be kept.
    pub(crate) fn keep_script_vars(
        &mut self,
        env_before: &VarMap,
        env_after: &VarMap,
        globals_before: &VarMap,
        globals_after: &VarMap,
    ) -> Option<String> {
        let store = self.store.clone()?;
        let mut note = None;
        if let Some(new) = merge_script_vars(&self.globals, globals_before, globals_after) {
            match store.save_globals(&new) {
                Ok(()) => self.globals = new,
                Err(e) => note = Some(format!("Global variables not saved: {e}")),
            }
        }
        if env_before != env_after {
            match self
                .active_env
                .and_then(|i| self.envs.get(i).map(|x| (i, x.clone())))
            {
                Some((ix, (env, path))) => {
                    if let Some(vars) = merge_script_vars(&env.variables, env_before, env_after) {
                        let env = Environment {
                            name: env.name,
                            variables: vars,
                        };
                        match store.save_environment(&env, Some(&path)) {
                            Ok(p) => self.envs[ix] = (env, p),
                            Err(e) => note = Some(format!("Environment not saved: {e}")),
                        }
                    }
                }
                None => {
                    note = Some(
                        "pm.environment changes were not kept: no environment is active".into(),
                    )
                }
            }
        }
        note
    }

    /// Read a spilled body and index it on a background thread, then show it, unless the tab
    /// got a newer response meanwhile.
    pub(crate) fn load_body_file(
        &mut self,
        id: u64,
        generation: u64,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { std::fs::read(&path).map(ViewData::new) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(t) = this
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == id && t.view_gen == generation)
                {
                    match loaded {
                        Ok(v) => t.set_view_data(v),
                        Err(e) => t.set_view(
                            format!("Could not read the saved response body: {e}").into_bytes(),
                        ),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Save the response body: the original bytes for big (spilled) bodies, else what's shown.
    pub(crate) fn on_save_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab();
        let Some(view) = tab.view.as_ref() else {
            return;
        };
        let ct = content_type(&tab.resp_headers).to_ascii_lowercase();
        let ext = if ct.contains("json") {
            "json"
        } else if ct.contains("xml") {
            "xml"
        } else if ct.contains("html") {
            "html"
        } else {
            "txt"
        };
        let source = tab.body_file.clone();
        let data = view.data.clone();
        let rx = cx.prompt_for_new_path(&self.export_dir(), Some(&format!("response.{ext}")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let written = match &source {
                Some(src) => std::fs::copy(src, &path).map(|_| ()),
                None => std::fs::write(&path, data.as_slice()),
            };
            this.update(cx, |this, cx| {
                this.tab_mut().status = match written {
                    Ok(()) => Status::Info(format!("Response body saved to {}", path.display())),
                    Err(e) => Status::Error(format!("Could not write {}: {e}", path.display())),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
