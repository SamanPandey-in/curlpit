//! Find in the response body (FR-RES-05): opening the bar and stepping through matches.
//! The matches themselves are computed and revealed by `RequestTab`, per tab.

use crate::response::ResTab;
use crate::workspace::Workspace;
use gpui::{Context, Window};

impl Workspace {
    pub(crate) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        tab.find_open = true;
        tab.res_tab = ResTab::Body;
        tab.scroll_to_match = !tab.matches.is_empty();
        tab.find.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn find_step(&mut self, id: u64, delta: isize, cx: &mut Context<Self>) {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
            let n = t.matches.len();
            if n > 0 {
                t.match_ix = (t.match_ix as isize + delta).rem_euclid(n as isize) as usize;
                t.scroll_to_match = true;
                cx.notify();
            }
        }
    }
}
