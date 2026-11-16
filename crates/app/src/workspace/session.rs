//! Hot exit: writing the open tabs to `session.toml` and rebuilding them next start,
//! the most-recently-closed-tab stack behind Cmd/Ctrl+Shift+T, and the constructors for
//! the editor rows a restored request needs.

use crate::editor::{FormRow, KvRow};
use crate::request_tab::RequestTab;
use crate::ui::Status;
use crate::workspace::Workspace;
use at_core::{FormField, FormValue, Request};
use at_store::{Session, SessionTab, Store};
use gpui::prelude::*;
use gpui::{App, Context, PathPromptOptions, Window};
use gpui_component::input::InputState;
use std::time::Duration;

/// How often open tabs are written to `session.toml` (only when something changed).
pub(crate) const SESSION_SAVE_EVERY: Duration = Duration::from_secs(3);

/// Closed tabs remembered for Cmd/Ctrl+Shift+T.
pub(crate) const CLOSED_TABS_MAX: usize = 20;

impl Workspace {
    pub(crate) fn session(&self, cx: &App) -> Session {
        Session {
            active: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|t| SessionTab {
                    path: t.current_path.clone(),
                    save_dir: t.save_dir.clone(),
                    unsaved: t.is_dirty(cx),
                    draft: t.current_request(cx),
                })
                .collect(),
            ..Default::default()
        }
    }

    pub(crate) fn persist_session(&mut self, cx: &App) {
        if !self.session_enabled {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        let s = self.session(cx);
        if self.last_session.as_ref() == Some(&s) {
            return;
        }
        match store.save_session(&s) {
            Ok(()) => self.last_session = Some(s),
            Err(e) => {
                // Say it once and stop trying, rather than every few seconds.
                self.session_enabled = false;
                self.tab_mut().status =
                    Status::Error(format!("Open tabs won't be restored next time: {e}"));
            }
        }
    }

    /// Rebuild the tabs from the last session. Each tab shows what its editor showed; a tab
    /// that had unsaved edits is still marked unsaved against its file.
    pub(crate) fn restore_session(
        &mut self,
        s: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut tabs = Vec::new();
        for st in s.tabs {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            let mut tab = RequestTab::create(id, true, window, cx);
            tab.save_dir = st.save_dir;
            match st.path {
                Some(path) => match Store::load_request(&path) {
                    Ok(file) => {
                        tab.apply_request(file, window, cx);
                        tab.current_path = Some(path);
                        if st.unsaved {
                            let saved = tab.saved.clone();
                            tab.apply_request(st.draft, window, cx);
                            tab.saved = saved;
                        }
                    }
                    Err(_) => {
                        // Moved or deleted since: keep the user's last version, unsaved.
                        tab.apply_request(st.draft, window, cx);
                        tab.saved = Request::default();
                        tab.status = Status::Info(format!(
                            "{} was moved or deleted; this is your last version (not saved)",
                            path.display()
                        ));
                    }
                },
                None => {
                    tab.apply_request(st.draft, window, cx);
                    if st.unsaved {
                        tab.saved = Request::default();
                    }
                }
            }
            tabs.push(tab);
        }
        if tabs.is_empty() {
            return;
        }
        self.active = s.active.min(tabs.len() - 1);
        self.tabs = tabs;
    }

    /// Cmd/Ctrl+Shift+T: bring back the most recently closed tab, edits included.
    pub(crate) fn reopen_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.closed.pop() else {
            self.tab_mut().status = Status::Info("No closed tabs to reopen".into());
            cx.notify();
            return;
        };
        if let Some(ix) = c.path.as_ref().and_then(|p| {
            self.tabs
                .iter()
                .position(|t| t.current_path.as_ref() == Some(p))
        }) {
            self.activate_tab(ix, window, cx);
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let mut tab = RequestTab::create(id, true, window, cx);
        tab.apply_request(c.draft, window, cx);
        tab.saved = c.saved;
        tab.current_path = c.path.filter(|p| p.exists());
        tab.save_dir = c.save_dir;
        tab.status = Status::Info("Reopened".into());
        if self.tab().is_pristine(cx) && self.tabs.len() == 1 {
            self.tabs[0] = tab;
            self.active = 0;
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
        self.refocus(window);
        cx.notify();
    }

    pub(crate) fn new_row(window: &mut Window, cx: &mut Context<Self>, k: &str, v: &str) -> KvRow {
        let key = cx.new(|cx| InputState::new(window, cx).default_value(k.to_string()));
        let value = cx.new(|cx| InputState::new(window, cx).default_value(v.to_string()));
        KvRow {
            enabled: true,
            key,
            value,
        }
    }

    pub(crate) fn new_form_row(
        window: &mut Window,
        cx: &mut Context<Self>,
        f: &FormField,
    ) -> FormRow {
        let (is_file, v) = match &f.value {
            FormValue::Text { value } => (false, value.clone()),
            FormValue::File { path } => (true, path.clone()),
        };
        let key = cx.new(|cx| InputState::new(window, cx).default_value(f.key.clone()));
        let value = cx.new(|cx| InputState::new(window, cx).default_value(v));
        FormRow {
            enabled: f.enabled,
            is_file,
            key,
            value,
        }
    }

    /// Pick a file for form-data row `ix` of tab `id`.
    pub(crate) fn choose_form_file(
        &mut self,
        id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(p) = paths.into_iter().next() {
                    this.update_in(cx, |this, window, cx| {
                        if let Some(row) = this
                            .tabs
                            .iter()
                            .find(|t| t.id == id)
                            .and_then(|t| t.multipart.get(ix))
                        {
                            let p = p.to_string_lossy().into_owned();
                            row.value.update(cx, |s, cx| s.set_value(p, window, cx));
                        }
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}
