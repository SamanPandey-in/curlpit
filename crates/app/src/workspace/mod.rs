//! `Workspace` is the single gpui view: it owns the open tabs, the storage handles, the
//! variables, history, the runner and the cookie jar. Its behaviour is split across the
//! child modules of this one, each of which adds an inherent `impl Workspace` block for
//! one area of the app; this file holds the state itself, its construction, and the few
//! helpers every area needs.

mod codegen;
mod collections;
mod cookies;
mod environments;
mod find;
mod history;
mod interop;
mod render;
mod runner;
mod send;
mod session;
mod tabs;
mod tree;

use crate::editor::{has_body, EnvRow};
use crate::request_tab::{ClosedTab, RequestTab};
use crate::runner::Runner;
use crate::sidebar::{MainView, Scope, SideView, SidebarItem};
use crate::ui::Status;
use crate::workspace::session::SESSION_SAVE_EVERY;
use at_codegen::Lang;
use at_core::{Body, EnvVar, Environment, Request, Variables};
use at_history::{History, Summary};
use at_http::{CookieJar, HttpError};
use at_script::{ScriptOutput, VarMap};
use at_store::{Session, Store, StoreError};
use gpui::prelude::*;
use gpui::{Context, Entity, FocusHandle, Subscription, Window};
use gpui_component::input::{InputEvent, InputState};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(crate) struct Workspace {
    rt: tokio::runtime::Handle,
    pub(crate) tabs: Vec<RequestTab>,
    active: usize,
    next_tab_id: u64,
    /// Messages for the environments page (and startup storage errors); each tab has its own
    /// status for request messages.
    status: Status,
    store: Option<Store>,
    items: Vec<SidebarItem>,
    focus: FocusHandle,
    focused_once: bool,
    // ---- environments (Phase 3b) ----
    main_view: MainView,
    globals: Vec<EnvVar>,
    /// Environments with the file each one lives in.
    envs: Vec<(Environment, PathBuf)>,
    active_env: Option<usize>,
    env_scope: Scope,
    env_name: Entity<InputState>,
    env_rows: Vec<EnvRow>,
    confirm_delete: bool,
    // ---- history ----
    side_view: SideView,
    /// Shared with the network threads, which record each send as it finishes.
    history: Option<Arc<Mutex<History>>>,
    hist_items: Vec<Summary>,
    hist_search: Entity<InputState>,
    /// Why history is unavailable or the last history operation failed.
    hist_error: Option<String>,
    confirm_clear: bool,
    /// Environment export includes secret values (off by default, FR-ENV-04).
    export_secrets: bool,
    // ---- code generation ----
    code_lang: Lang,
    /// Snippets keep `{{variables}}` instead of their values (safe to share).
    code_keep_vars: bool,
    code_copied: bool,
    // ---- session (hot exit) ----
    /// False when the session file can't be written, or is from a newer app (never overwrite).
    session_enabled: bool,
    /// What was last written, so unchanged sessions aren't rewritten every few seconds.
    last_session: Option<Session>,
    closed: Vec<ClosedTab>,
    runner: Runner,
    // ---- cookie jar ----
    cookies: Arc<CookieJar>,
    /// Why cookies couldn't be loaded or saved.
    cookie_note: Option<String>,
    confirm_clear_cookies: bool,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub(crate) fn new(
        rt: tokio::runtime::Handle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Everything paints from the One palette (`theme`), and the appearance follows the
        // system both now and whenever the user flips it (FR-SET-01).
        crate::theme::sync(window, cx);
        let appearance = window.observe_window_appearance(|window, cx| {
            crate::theme::sync(window, cx);
            window.refresh();
        });
        let env_name = cx.new(|cx| InputState::new(window, cx));
        let (store, store_err) = match Store::open_default() {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let (cookies, cookie_note) = match &store {
            Some(s) => match CookieJar::load(&s.data_dir().join("cookies.json")) {
                Ok(j) => (j, None),
                Err(e) => (
                    CookieJar::new(),
                    Some(format!(
                        "Saved cookies could not be read, starting empty: {e}"
                    )),
                ),
            },
            None => (CookieJar::new(), None),
        };
        let (history, hist_error) = match &store {
            Some(s) => match History::open(&s.data_dir().join("history.db")) {
                Ok(h) => (Some(Arc::new(Mutex::new(h))), None),
                Err(e) => (None, Some(format!("History is off: {e}"))),
            },
            None => (
                None,
                Some("History is off: storage is unavailable".to_string()),
            ),
        };
        let hist_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search URL or name"));
        let sub = cx.subscribe_in(
            &hist_search,
            window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_history(cx);
                    cx.notify();
                }
            },
        );
        let (session, session_note, session_enabled) = match &store {
            Some(s) => match s.load_session() {
                Ok(x) => (x, None, true),
                Err(e @ StoreError::TooNew { .. }) => {
                    (None, Some(format!("Last session not restored: {e}")), false)
                }
                Err(e) => (
                    None,
                    Some(format!(
                        "Last session could not be read, starting fresh: {e}"
                    )),
                    true,
                ),
            },
            None => (None, None, false),
        };
        let mut first = RequestTab::create(0, false, window, cx);
        if let Some(e) = &store_err {
            first.status = Status::Error(e.clone());
        }
        let mut me = Self {
            rt,
            tabs: vec![first],
            active: 0,
            next_tab_id: 1,
            status: store_err.map(Status::Error).unwrap_or(Status::Idle),
            store,
            items: Vec::new(),
            focus: cx.focus_handle(),
            focused_once: false,
            main_view: MainView::Request,
            globals: Vec::new(),
            envs: Vec::new(),
            active_env: None,
            env_scope: Scope::Globals,
            env_name,
            env_rows: Vec::new(),
            confirm_delete: false,
            side_view: SideView::Collections,
            history,
            hist_items: Vec::new(),
            hist_search,
            hist_error,
            confirm_clear: false,
            export_secrets: false,
            code_lang: Lang::Curl,
            code_keep_vars: false,
            code_copied: false,
            session_enabled,
            last_session: None,
            closed: Vec::new(),
            cookies: Arc::new(cookies),
            cookie_note,
            confirm_clear_cookies: false,
            runner: Runner {
                target: None,
                iterations: cx.new(|cx| InputState::new(window, cx).default_value("1")),
                delay: cx.new(|cx| InputState::new(window, cx).default_value("0")),
                stop_on_fail: false,
                rows: Vec::new(),
                planned: 0,
                cancel: None,
                started: None,
                elapsed: None,
                notes: Vec::new(),
                copied: false,
            },
            _subs: vec![sub, appearance],
        };
        me.refresh_sidebar();
        me.reload_envs();
        me.load_scope(Scope::Globals, window, cx);
        if let Some(s) = session {
            me.restore_session(s, window, cx);
        }
        if let Some(note) = session_note {
            me.tab_mut().status = Status::Error(note);
        }

        // Keep the session file current: on a timer (cheap: written only when it changed),
        // when the window closes, and when the app quits.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(SESSION_SAVE_EVERY).await;
            if this
                .update(cx, |this, cx| this.persist_session(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_window, cx| {
            if let Some(ws) = weak.upgrade() {
                ws.update(cx, |this, cx| this.persist_session(cx));
            }
            true
        });
        me._subs.push(cx.on_app_quit(|this, cx| {
            this.persist_session(cx);
            async {}
        }));
        me
    }

    pub(crate) fn tab(&self) -> &RequestTab {
        &self.tabs[self.active]
    }

    pub(crate) fn tab_mut(&mut self) -> &mut RequestTab {
        &mut self.tabs[self.active]
    }

    /// Keep keyboard shortcuts working after a button click moved focus away.
    pub(crate) fn refocus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    /// Show a message where the user is looking: the environments page or the active tab.
    pub(crate) fn notice(&mut self, s: Status) {
        if self.main_view == MainView::Environments {
            self.status = s;
        } else {
            self.tab_mut().status = s;
        }
    }
}

/// What a send produced; sent from the network task back to the UI thread.
pub(crate) struct SendOutcome {
    pub(crate) sent: Result<Result<at_http::HttpResponse, HttpError>, String>,
    pub(crate) unresolved: Vec<String>,
    pub(crate) pre: Option<ScriptOutput>,
    pub(crate) test: Option<ScriptOutput>,
    pub(crate) env_before: VarMap,
    pub(crate) globals_before: VarMap,
    pub(crate) hist_err: Option<String>,
}

/// The request as it goes out: `https://` is added when the URL has no scheme *after*
/// variable substitution (so `{{base}}/users` works when `base` contains one), and methods
/// that don't take a body send none.
pub(crate) fn sendable(mut req: Request, vars: &Variables) -> Request {
    if !req.url.is_empty() && !vars.resolve(&req.url).text.contains("://") {
        req.url = format!("https://{}", req.url);
    }
    if !has_body(req.method) {
        req.body = Body::None;
    }
    req
}

/// Merge imported variables into existing ones: same key (exact) replaces, new keys are added.
/// Returns the merged list and how many were replaced.
pub(crate) fn merge_vars(existing: &[EnvVar], incoming: Vec<EnvVar>) -> (Vec<EnvVar>, usize) {
    let mut out = existing.to_vec();
    let mut replaced = 0;
    for v in incoming {
        match out.iter_mut().find(|e| e.key == v.key) {
            Some(e) => {
                *e = v;
                replaced += 1;
            }
            None => out.push(v),
        }
    }
    (out, replaced)
}

/// Which tab is active after `closed` is removed, leaving `remaining` tabs: the same tab if it
/// is still there, else its right neighbour, else the last one.
pub(crate) fn active_after_close(active: usize, closed: usize, remaining: usize) -> usize {
    if remaining == 0 {
        0
    } else if closed < active {
        active - 1
    } else {
        active.min(remaining - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_tab_after_closing() {
        // closing a tab left of the active one shifts the active index down
        assert_eq!(active_after_close(2, 0, 3), 1);
        // closing the active tab selects its right neighbour (same index)
        assert_eq!(active_after_close(1, 1, 3), 1);
        // closing the active last tab selects the new last one
        assert_eq!(active_after_close(2, 2, 2), 1);
        // closing a tab right of the active one changes nothing
        assert_eq!(active_after_close(0, 2, 2), 0);
        // closing the only tab (a fresh one is added by the caller)
        assert_eq!(active_after_close(0, 0, 1), 0);
        assert_eq!(active_after_close(0, 0, 0), 0);
    }
}
