//! Opening, switching and closing request tabs, including the two-click confirmation
//! that stops an unsaved tab being discarded by accident.

use crate::request_tab::{ClosedTab, RequestTab};
use crate::ui::Status;
use crate::workspace::session::CLOSED_TABS_MAX;
use crate::workspace::{active_after_close, Workspace};
use gpui::{Context, Window};

impl Workspace {
    pub(crate) fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = RequestTab::create(id, true, window, cx);
        // New requests go to the folder the current tab was using.
        let dir = self.tab().save_dir.clone();
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.tab_mut().save_dir = dir;
        self.refocus(window);
        cx.notify();
    }

    pub(crate) fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        for (i, t) in self.tabs.iter_mut().enumerate() {
            if i != ix {
                t.confirm_close = false;
            }
        }
        self.active = ix;
        self.refocus(window);
        cx.notify();
    }

    /// Close a tab. An unsaved tab needs a second click (or Cmd+W again) to discard it.
    pub(crate) fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        if self.tabs[ix].is_dirty(cx) && !self.tabs[ix].confirm_close {
            self.tabs[ix].confirm_close = true;
            self.tabs[ix].status = Status::Info(
                "Unsaved changes: Save first, or click the tab's Discard? to close it anyway"
                    .into(),
            );
            cx.notify();
            return;
        }
        if let Some(token) = self.tabs[ix].cancel.take() {
            token.cancel();
        }
        let t = &self.tabs[ix];
        // Untouched blank tabs aren't worth reopening.
        if !t.is_pristine(cx) {
            self.closed.push(ClosedTab {
                path: t.current_path.clone(),
                save_dir: t.save_dir.clone(),
                draft: t.current_request(cx),
                saved: t.saved.clone(),
            });
            if self.closed.len() > CLOSED_TABS_MAX {
                self.closed.remove(0);
            }
        }
        self.tabs.remove(ix);
        if self.tabs.is_empty() {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            self.tabs.push(RequestTab::create(id, true, window, cx));
        }
        self.active = active_after_close(self.active, ix, self.tabs.len());
        self.refocus(window);
        cx.notify();
    }
}
