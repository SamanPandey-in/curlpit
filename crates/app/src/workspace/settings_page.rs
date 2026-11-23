//! The app Settings screen (FR-SET-02, 03): load the stored defaults into the page, edit
//! them, and save. Per-request settings still live on each request tab; this is the app
//! default that seeds new requests, plus the history cap and outbound proxy.
use gpui::{Context, Window};

use crate::sidebar::MainView;
use crate::ui::Status;
use crate::workspace::Workspace;

impl Workspace {
    /// Open the Settings page, loading the current values into its fields.
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.settings.clone();
        self.set_timeout.update(cx, |st, cx| {
            st.set_value(s.default_timeout_ms.to_string(), window, cx)
        });
        self.set_history_cap.update(cx, |st, cx| {
            st.set_value(s.history_cap.to_string(), window, cx)
        });
        self.set_proxy
            .update(cx, |st, cx| st.set_value(s.proxy.clone(), window, cx));
        self.set_verify_tls = s.verify_tls;
        self.status = Status::Idle;
        self.main_view = MainView::Settings;
        cx.notify();
    }

    pub(crate) fn toggle_setting_tls(&mut self, cx: &mut Context<Self>) {
        self.set_verify_tls = !self.set_verify_tls;
        cx.notify();
    }

    /// Validate the fields, persist, and apply what can change live.
    pub(crate) fn save_settings_page(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            self.notice(Status::Error(
                "Storage is unavailable on this system".into(),
            ));
            cx.notify();
            return;
        };
        let timeout_txt = self.set_timeout.read(cx).value().trim().to_string();
        let cap_txt = self.set_history_cap.read(cx).value().trim().to_string();
        let proxy = self.set_proxy.read(cx).value().trim().to_string();

        let Ok(default_timeout_ms) = timeout_txt.parse::<u64>() else {
            self.notice(Status::Error(
                "Default timeout must be a whole number of milliseconds (0 for none)".into(),
            ));
            cx.notify();
            return;
        };
        let history_cap = match cap_txt.parse::<usize>() {
            Ok(n) if n >= 1 => n,
            _ => {
                self.notice(Status::Error(
                    "History cap must be a whole number, 1 or more".into(),
                ));
                cx.notify();
                return;
            }
        };

        // Load fresh so the active environment (saved separately) is preserved.
        let mut s = store.load_settings();
        s.default_timeout_ms = default_timeout_ms;
        s.verify_tls = self.set_verify_tls;
        s.history_cap = history_cap;
        s.proxy = proxy;
        if let Err(e) = store.save_settings(&s) {
            self.notice(Status::Error(e.to_string()));
            cx.notify();
            return;
        }
        self.settings = s;
        self.notice(Status::Info(
            "Settings saved. Proxy applies to the next request; the default timeout and TLS apply to new tabs; the history limit applies next launch.".into(),
        ));
        cx.notify();
    }
}
