//! The collection runner page's control (FR-RUN): what gets run, starting
//! `at_runner::run` on the network runtime, and folding each progress message back into
//! `Runner`.

use crate::format::parse_count;
use crate::sidebar::{ItemKind, MainView};
use crate::workspace::send::{ScriptMode, TrustGate};
use crate::workspace::Workspace;
use at_runner::{run_order, RunMsg, RunPlan};
use at_store::Store;
use gpui::Context;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

type RunFiles = Vec<(String, Result<PathBuf, String>)>;

/// Script-free copies of the requests to run, for "Run without scripts" (G-16). `at-runner`
/// reads each request from its file, so the copies go to a temporary folder; the map leads
/// each copy back to its original so result rows still open the real request.
fn script_free_copies(
    files: &RunFiles,
) -> std::io::Result<(RunFiles, PathBuf, HashMap<PathBuf, PathBuf>)> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "api-tester-run-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir)?;
    let mut out = Vec::with_capacity(files.len());
    let mut back = HashMap::new();
    for (ix, (name, file)) in files.iter().enumerate() {
        let copied = match file {
            Ok(path) => match Store::load_request(path) {
                Ok(mut req) => {
                    req.pre_request_script.clear();
                    req.test_script.clear();
                    // one folder per request keeps the copies from ever sharing a name
                    let sub = dir.join(ix.to_string());
                    Store::save_request(&sub, &req, None)
                        .inspect(|copy| {
                            back.insert(copy.clone(), path.clone());
                        })
                        .map_err(|e| e.to_string())
                }
                // keep the original so the runner reports its own load error
                Err(_) => Ok(path.clone()),
            },
            Err(e) => Err(e.clone()),
        };
        out.push((name.clone(), copied));
    }
    Ok((out, dir, back))
}

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
        self.start_run_with(ScriptMode::Normal, cx);
    }

    /// "Trust collection" on the runner's prompt: trust it, then run.
    pub(crate) fn trust_and_run(&mut self, cx: &mut Context<Self>) {
        let Some(u) = TrustGate::runner(cx) else {
            return;
        };
        if self.trust_collection(&u.dir, cx) {
            self.start_run(cx);
        }
    }

    /// Close the runner's prompt without running.
    pub(crate) fn dismiss_run_trust(&mut self, cx: &mut Context<Self>) {
        cx.default_global::<TrustGate>().runner = None;
        cx.notify();
    }

    /// Run the target. A run of an untrusted collection whose requests carry scripts waits
    /// for the user's choice on the page instead (G-16), unless `mode` already says to run
    /// without scripts.
    pub(crate) fn start_run_with(&mut self, mode: ScriptMode, cx: &mut Context<Self>) {
        let Some((name, dir)) = self.runner.target.clone() else {
            return;
        };
        cx.default_global::<TrustGate>().runner = None;
        if mode == ScriptMode::Normal {
            if let Some(u) = self.untrusted_at(&dir, Some(&dir)) {
                if u.scripts > 0 {
                    cx.default_global::<TrustGate>().runner = Some(u);
                    cx.notify();
                    return;
                }
            }
        }
        let mut files: RunFiles = Vec::new();
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
        self.clean_run_copies(cx);
        if mode == ScriptMode::Without {
            match script_free_copies(&files) {
                Ok((copies, copy_dir, back)) => {
                    files = copies;
                    let gate = cx.default_global::<TrustGate>();
                    gate.run_copies = back;
                    gate.run_copy_dir = Some(copy_dir);
                }
                Err(e) => {
                    self.runner.notes =
                        vec![format!("Could not prepare a run without scripts: {e}")];
                    cx.notify();
                    return;
                }
            }
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
        let planned = files.len() * iterations;
        let r = &mut self.runner;
        r.rows.clear();
        r.notes.clear();
        r.copied = false;
        r.planned = planned;
        r.cancel = Some(token.clone());
        r.started = Some(Instant::now());
        r.elapsed = None;
        if mode == ScriptMode::Without {
            r.notes
                .push("Running without scripts: this collection isn't trusted yet.".into());
        }

        crate::workspace::settings_page::apply_tls(&self.settings);
        let plan = RunPlan {
            files,
            iterations,
            delay,
            stop_on_fail,
            vars: self.variables(),
            env_name: self
                .active_env
                .and_then(|i| self.envs.get(i))
                .map(|(e, _)| e.name.clone()),
            cookies: self.cookies.clone(),
            proxy: self.settings.proxy.clone(),
            ..Default::default()
        };
        let (tx, rx) = async_channel::unbounded();
        self.rt.spawn(at_runner::run(plan, token, tx));
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
            RunMsg::Row {
                mut row,
                env,
                globals,
            } => {
                // a script-free copy's row points back at the real request
                if let Some(orig) = row
                    .path
                    .as_ref()
                    .and_then(|p| TrustGate::get(cx).and_then(|g| g.run_copies.get(p).cloned()))
                {
                    row.path = Some(orig);
                }
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
                self.clean_run_copies(cx);
            }
        }
        cx.notify();
    }

    /// Remove the script-free copies of the last run, if there are any.
    fn clean_run_copies(&mut self, cx: &mut Context<Self>) {
        if !cx.has_global::<TrustGate>() {
            return;
        }
        let gate = cx.global_mut::<TrustGate>();
        gate.run_copies.clear();
        if let Some(dir) = gate.run_copy_dir.take() {
            remove_copies(&dir);
        }
    }
}

fn remove_copies(dir: &Path) {
    // best effort: a leftover folder in the temp directory is harmless
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use at_core::Request;

    #[test]
    fn script_free_copies_blank_scripts_and_map_back() {
        let root = std::env::temp_dir().join(format!("api-tester-g16-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let req = Request {
            name: "Scripted".into(),
            url: "https://x.test".into(),
            pre_request_script: "pm.environment.set('a', 1)".into(),
            test_script: "pm.test('t', () => {})".into(),
            ..Default::default()
        };
        let orig = Store::save_request(&root, &req, None).unwrap();
        let twin = Store::save_request(&root, &req, None).unwrap();
        let files: RunFiles = vec![
            ("Scripted".into(), Ok(orig.clone())),
            ("Twin".into(), Ok(twin.clone())),
            ("Broken".into(), Err("unreadable".into())),
        ];
        let (copies, dir, back) = script_free_copies(&files).unwrap();
        assert_eq!(copies.len(), 3);
        for (copy, from) in [(&copies[0], &orig), (&copies[1], &twin)] {
            let path = copy.1.as_ref().unwrap();
            assert_ne!(path, from);
            assert_eq!(back.get(path), Some(from));
            let r = Store::load_request(path).unwrap();
            assert!(r.pre_request_script.is_empty() && r.test_script.is_empty());
            assert_eq!(r.url, "https://x.test");
        }
        assert_eq!(copies[2].1, Err("unreadable".to_string()));
        // the originals keep their scripts
        assert!(!Store::load_request(&orig)
            .unwrap()
            .pre_request_script
            .is_empty());
        remove_copies(&dir);
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
