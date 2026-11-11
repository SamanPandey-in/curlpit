//! The collection runner's control loop (FR-RUN): what gets run, starting the sequential
//! run on the network runtime, and folding each progress message back into `Runner`.
//! Variables set by one request's scripts flow into the next.

use crate::format::parse_count;
use crate::runner::{run_order, RunMsg, RunResult, RunRow};
use crate::script::to_var_map;
use crate::sidebar::{ItemKind, MainView};
use crate::workspace::send::execute;
use crate::workspace::Workspace;
use at_core::{Method, Variables};
use at_store::Store;
use gpui::Context;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

impl Workspace {
    /// What "Run…" runs: the folder or collection last clicked in the sidebar (for a tab with
    /// no file), else the collection holding the active tab's request.
    pub(crate) fn run_target(&self) -> Option<(String, PathBuf)> {
        let tab = self.tab();
        if tab.current_path.is_none() {
            if let Some(dir) = &tab.save_dir {
                if let Some(i) = self.items.iter().find(|i| {
                    matches!(i.kind, ItemKind::Collection | ItemKind::Folder) && &i.path == dir
                }) {
                    return Some((i.label.clone(), i.path.clone()));
                }
            }
        }
        self.selected_collection()
    }

    pub(crate) fn open_runner(&mut self, cx: &mut Context<Self>) {
        if !self.leave_env_page(cx) {
            return;
        }
        if self.runner.cancel.is_none() {
            // not running: pick up the current selection
            self.runner.target = self.run_target();
        }
        self.main_view = MainView::Runner;
        cx.notify();
    }

    pub(crate) fn start_run(&mut self, cx: &mut Context<Self>) {
        let Some((name, dir)) = self.runner.target.clone() else {
            return;
        };
        let mut files = Vec::new();
        match Store::load_tree(&dir) {
            Ok(nodes) => run_order(&nodes, &mut files),
            Err(e) => {
                self.runner.notes = vec![e.to_string()];
                cx.notify();
                return;
            }
        }
        if files.is_empty() {
            self.runner.notes = vec![format!("\"{name}\" has no requests to run")];
            cx.notify();
            return;
        }
        let iterations = parse_count(&self.runner.iterations.read(cx).value(), 1, 1, 100) as usize;
        let delay = Duration::from_millis(parse_count(
            &self.runner.delay.read(cx).value(),
            0,
            0,
            60_000,
        ));
        let stop_on_fail = self.runner.stop_on_fail;
        let token = CancellationToken::new();
        let r = &mut self.runner;
        r.rows.clear();
        r.notes.clear();
        r.copied = false;
        r.planned = files.len() * iterations;
        r.cancel = Some(token.clone());
        r.started = Some(Instant::now());
        r.elapsed = None;

        let vars = self.variables();
        let env_name = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.name.clone());
        let (tx, rx) = async_channel::unbounded();
        let cookies = self.cookies.clone();
        // Requests run one after another; variables set by scripts flow into the next one.
        self.rt.spawn(async move {
            let mut vars = vars;
            'run: for iteration in 0..iterations {
                for (name, file) in &files {
                    if token.is_cancelled() {
                        break 'run;
                    }
                    let msg = match file.as_ref().map(|p| (p, Store::load_request(p))) {
                        Err(e) => RunMsg::Row {
                            row: Box::new(RunRow {
                                iteration,
                                name: name.clone(),
                                path: None,
                                method: Method::Get,
                                result: RunResult::Failed(format!("unreadable file: {e}")),
                                tests: Vec::new(),
                                errors: Vec::new(),
                            }),
                            env: Default::default(),
                            globals: Default::default(),
                        },
                        Ok((path, Err(e))) => RunMsg::Row {
                            row: Box::new(RunRow {
                                iteration,
                                name: name.clone(),
                                path: Some(path.clone()),
                                method: Method::Get,
                                result: RunResult::Failed(e.to_string()),
                                tests: Vec::new(),
                                errors: Vec::new(),
                            }),
                            env: Default::default(),
                            globals: Default::default(),
                        },
                        Ok((path, Ok(req))) => {
                            let method = req.method;
                            let (out, after) = execute(
                                req,
                                vars.clone(),
                                env_name.clone(),
                                token.child_token(),
                                None,
                                cookies.clone(),
                            )
                            .await;
                            let row = RunRow::from_outcome(
                                iteration,
                                name.clone(),
                                path.clone(),
                                method,
                                &out,
                            );
                            let msg = RunMsg::Row {
                                row: Box::new(row),
                                env: (out.env_before.clone(), to_var_map(&after.environment)),
                                globals: (out.globals_before.clone(), to_var_map(&after.global)),
                            };
                            // runtime (pm.variables) values live for one request only
                            vars = Variables {
                                runtime: Default::default(),
                                ..after
                            };
                            msg
                        }
                    };
                    let failed = matches!(&msg, RunMsg::Row { row, .. } if row.failed());
                    if tx.send(msg).await.is_err() || (failed && stop_on_fail) {
                        break 'run;
                    }
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
            let _ = tx.send(RunMsg::Done).await;
        });
        cx.spawn(async move |this, cx| {
            while let Ok(msg) = rx.recv().await {
                let done = matches!(msg, RunMsg::Done);
                if this
                    .update(cx, |this, cx| this.on_run_msg(msg, cx))
                    .is_err()
                    || done
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn on_run_msg(&mut self, msg: RunMsg, cx: &mut Context<Self>) {
        match msg {
            RunMsg::Row { row, env, globals } => {
                if let Some(n) = self.keep_script_vars(&env.0, &env.1, &globals.0, &globals.1) {
                    if !self.runner.notes.contains(&n) {
                        self.runner.notes.push(n);
                    }
                }
                self.runner.rows.push(*row);
                self.save_cookies_if_changed();
            }
            RunMsg::Done => {
                self.runner.cancel = None;
                self.runner.elapsed = self.runner.started.map(|s| s.elapsed());
            }
        }
        cx.notify();
    }
}
