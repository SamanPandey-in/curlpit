//! The shared cookie jar (FR-COOKIE): persisting it after a send that changed it, and
//! opening the page that lists it.

use crate::sidebar::MainView;
use crate::workspace::Workspace;
use gpui::Context;

impl Workspace {
    pub(crate) fn save_cookies_if_changed(&mut self) {
        if !self.cookies.take_changed() {
            return;
        }
        let Some(store) = &self.store else {
            return;
        };
        if let Err(e) = self.cookies.save(&store.data_dir().join("cookies.json")) {
            self.cookie_note = Some(format!("Cookies not saved: {e}"));
        }
    }

    pub(crate) fn open_cookies(&mut self, cx: &mut Context<Self>) {
        if !self.leave_env_page(cx) {
            return;
        }
        self.confirm_clear_cookies = false;
        self.main_view = MainView::Cookies;
        cx.notify();
    }
}
