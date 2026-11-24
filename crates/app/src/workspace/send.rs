//! Sending one request. The off-thread pipeline itself is `at_runner::execute`; the
//! `Workspace` methods here start a send, fold the result back into its tab, and keep
//! whatever its scripts changed.

use crate::response::{content_type, ViewData};
use crate::script::ScriptReport;
use crate::sidebar::SideView;
use crate::ui::Status;
use crate::workspace::{SendOutcome, Workspace};
use at_core::Environment;
use at_runner::{execute, merge_script_vars};
use at_script::VarMap;
use at_store::Store;
use gpui::{App, Context, Global, Window};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// A collection whose scripts are held back until the user decides (G-16).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Untrusted {
    /// Collection root.
    pub(crate) dir: PathBuf,
    pub(crate) name: String,
    /// Scripts in the collection (or in the folder about to run).
    pub(crate) scripts: usize,
}

/// The trust prompts on screen: per request tab, and for the collection runner. App-wide
/// state kept as a gpui global so the prompt needs no new `Workspace` fields.
#[derive(Default)]
pub(crate) struct TrustGate {
    /// Tab id -> the untrusted collection its last Send was stopped for.
    pub(crate) tabs: HashMap<u64, Untrusted>,
    /// Set while the runner page asks before running an untrusted collection.
    pub(crate) runner: Option<Untrusted>,
    /// Script-free copies made for "Run without scripts": copy -> original request file.
    pub(crate) run_copies: HashMap<PathBuf, PathBuf>,
    /// Folder holding those copies, removed when the run ends.
    pub(crate) run_copy_dir: Option<PathBuf>,
}

impl Global for TrustGate {}

impl TrustGate {
    pub(crate) fn get(cx: &App) -> Option<&TrustGate> {
        cx.try_global::<TrustGate>()
    }

    pub(crate) fn tab(cx: &App, id: u64) -> Option<Untrusted> {
        Self::get(cx).and_then(|g| g.tabs.get(&id).cloned())
    }

    pub(crate) fn runner(cx: &App) -> Option<Untrusted> {
        Self::get(cx).and_then(|g| g.runner.clone())
    }
}

/// How a Send treats scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScriptMode {
    /// Run them, unless the request is in an untrusted collection (then ask).
    Normal,
    /// Send once with the pre-request and test scripts blanked.
    Without,
}

impl Workspace {
    /// The collection (display name, root) that `path` lives in: the first directory under
    /// the store's collections root. Worked out from the path, not the sidebar, so a
    /// collapsed or filtered sidebar can never let a request skip the trust check.
    pub(crate) fn collection_of(&self, path: &Path) -> Option<(String, PathBuf)> {
        let root = self.store.as_ref()?.root();
        let first = path.strip_prefix(root).ok()?.components().next()?;
        let dir = root.join(first);
        if !dir.is_dir() {
            return None;
        }
        let name = Store::load_collection_meta(&dir)
            .map(|m| m.name)
            .unwrap_or_else(|_| first.as_os_str().to_string_lossy().into_owned());
        Some((name, dir))
    }

    /// The untrusted collection holding `path`, with its script count; `None` when the
    /// collection is trusted, unknown or unreadable (a broken `collection.toml` is reported
    /// elsewhere, and treating it as untrusted would block every send).
    pub(crate) fn untrusted_at(&self, path: &Path, within: Option<&Path>) -> Option<Untrusted> {
        let (name, dir) = self.collection_of(path)?;
        let meta = Store::load_collection_meta(&dir).ok()?;
        if meta.trusted {
            return None;
        }
        let counted = within.unwrap_or(&dir);
        let scripts = at_interop::read_collection(&name, counted)
            .map(|(c, _)| at_interop::count_scripts(&c))
            .unwrap_or(0);
        Some(Untrusted {
            name: meta.name,
            dir,
            scripts,
        })
    }

    /// Mark a collection trusted (G-16) and drop every prompt that was waiting on it.
    pub(crate) fn trust_collection(&mut self, dir: &Path, cx: &mut Context<Self>) -> bool {
        let result = Store::load_collection_meta(dir).and_then(|mut meta| {
            meta.trusted = true;
            Store::save_collection_meta(dir, &meta)
        });
        if let Err(e) = result {
            self.notice(Status::Error(format!(
                "Could not trust the collection: {e}"
            )));
            cx.notify();
            return false;
        }
        let gate = cx.default_global::<TrustGate>();
        gate.tabs.retain(|_, u| u.dir != dir);
        if gate.runner.as_ref().is_some_and(|u| u.dir == dir) {
            gate.runner = None;
        }
        true
    }

    /// "Trust collection" on a tab's prompt: trust it, then send.
    pub(crate) fn trust_and_send(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(u) = TrustGate::tab(cx, id) else {
            return;
        };
        if self.trust_collection(&u.dir, cx) {
            self.send_tab(id, cx);
        }
    }

    /// "Send without scripts" on a tab's prompt.
    pub(crate) fn send_without_scripts(&mut self, id: u64, cx: &mut Context<Self>) {
        cx.default_global::<TrustGate>().tabs.remove(&id);
        self.send_tab_with(id, ScriptMode::Without, cx);
    }

    /// Close a tab's prompt without sending.
    pub(crate) fn dismiss_trust(&mut self, id: u64, cx: &mut Context<Self>) {
        cx.default_global::<TrustGate>().tabs.remove(&id);
        cx.notify();
    }

    /// Send the active tab's request, or cancel it if one is already in flight.
    pub(crate) fn send_active(&mut self, cx: &mut Context<Self>) {
        let id = self.tab().id;
        self.send_tab(id, cx);
    }

    /// Send a tab's request, or cancel it if one is already in flight.
    pub(crate) fn send_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        self.send_tab_with(id, ScriptMode::Normal, cx);
    }

    /// Send a tab's request (or cancel the one in flight). A request with scripts in an
    /// untrusted collection is not sent: the tab shows the trust prompt instead (G-16).
    pub(crate) fn send_tab_with(&mut self, id: u64, mode: ScriptMode, cx: &mut Context<Self>) {
        let vars = self.variables();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if let Some(token) = tab.cancel.take() {
            token.cancel();
            return;
        }
        let mut req = tab.current_request(cx);
        let path = tab.current_path.clone();
        let has_scripts =
            !req.pre_request_script.trim().is_empty() || !req.test_script.trim().is_empty();
        if mode == ScriptMode::Without {
            req.pre_request_script.clear();
            req.test_script.clear();
        } else if has_scripts {
            if let Some(u) = path.as_deref().and_then(|p| self.untrusted_at(p, None)) {
                cx.default_global::<TrustGate>().tabs.insert(id, u);
                cx.notify();
                return;
            }
        }
        if cx.has_global::<TrustGate>() {
            cx.global_mut::<TrustGate>().tabs.remove(&id);
        }
        crate::workspace::settings_page::apply_tls(&self.settings);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
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
        let proxy = self.settings.proxy.clone();
        self.rt.spawn(async move {
            let (out, _) = execute(req, vars, env_name, token, history, cookies, proxy).await;
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
