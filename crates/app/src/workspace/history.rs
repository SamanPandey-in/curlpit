//! The history sidebar (FR-HIS): listing and filtering past sends, clearing them, and
//! reopening one as a tab that shows the request as sent plus the stored response.

use crate::format::human;
use crate::response::{content_type, display_body};
use crate::sidebar::SideView;
use crate::ui::Status;
use crate::workspace::Workspace;
use at_history::{ago, now_ms};
use gpui::{App, Context, Window};

/// Most history rows shown in the sidebar at once (the file keeps up to 1,000).
pub(crate) const HISTORY_LIST_MAX: usize = 500;

impl Workspace {
    pub(crate) fn refresh_history(&mut self, cx: &App) {
        let Some(h) = self.history.clone() else {
            self.hist_items.clear();
            return;
        };
        let filter = self.hist_search.read(cx).value().to_string();
        let listed = match h.lock() {
            Ok(h) => h.list(&filter, HISTORY_LIST_MAX).map_err(|e| e.to_string()),
            Err(_) => Err("history is unavailable".to_string()),
        };
        match listed {
            Ok(items) => self.hist_items = items,
            Err(e) => self.hist_error = Some(e),
        }
    }

    pub(crate) fn show_side(&mut self, view: SideView, cx: &mut Context<Self>) {
        self.side_view = view;
        self.confirm_clear = false;
        if view == SideView::History {
            self.refresh_history(cx);
        }
        cx.notify();
    }

    pub(crate) fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.confirm_clear = false;
        if let Some(h) = self.history.clone() {
            let cleared = match h.lock() {
                Ok(mut h) => h.clear().map_err(|e| e.to_string()),
                Err(_) => Err("history is unavailable".to_string()),
            };
            match cleared {
                Ok(()) => self.hist_error = None,
                Err(e) => self.hist_error = Some(e),
            }
        }
        self.refresh_history(cx);
        cx.notify();
    }

    /// Open a history entry in a tab: the request as it was sent plus the stored response.
    /// The tab is not linked to any file; Save puts it into a collection as a new request.
    pub(crate) fn open_history(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.leave_env_page(cx) {
            return;
        }
        let Some(h) = self.history.clone() else {
            return;
        };
        let entry = match h.lock() {
            Ok(h) => h.get(id),
            Err(_) => return,
        };
        let entry = match entry {
            Ok(Some(e)) => e,
            Ok(None) => {
                self.hist_error = Some("That history entry no longer exists".into());
                self.refresh_history(cx);
                cx.notify();
                return;
            }
            Err(e) => {
                self.hist_error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        if !self.tab().is_pristine(cx) {
            self.add_tab(window, cx);
        }
        let s = entry.summary;
        let ct = content_type(&entry.resp_headers);
        let mut headers = entry.resp_headers;
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        let body = match entry.resp_body {
            // A cut-off JSON body can't be pretty-printed reliably; show it as it arrived.
            Some(mut b) if entry.body_truncated => {
                b.extend_from_slice(
                    format!(
                        "\n\n[… history kept only the first {} of {}]",
                        human(b.len() as u64),
                        human(s.size)
                    )
                    .as_bytes(),
                );
                b
            }
            Some(b) => display_body(&b, &ct),
            None if s.status.is_some() => format!(
                "The response body ({}) was too large to keep in history.",
                human(s.size)
            )
            .into_bytes(),
            None => format!(
                "No response: {}",
                s.error.clone().unwrap_or_else(|| "unknown error".into())
            )
            .into_bytes(),
        };
        let tab = self.tab_mut();
        tab.apply_request(entry.request, window, cx);
        tab.current_path = None;
        tab.confirm_close = false;
        tab.copied = false;
        tab.resp_headers = headers;
        tab.set_view(body);
        tab.status = match s.status {
            Some(code) => Status::Done {
                code,
                text: entry.status_text,
                ms: s.elapsed_ms as u128,
                size: s.size,
            },
            None => Status::Error(s.error.unwrap_or_else(|| "No response".into())),
        };
        tab.warning = Some(format!("From history, sent {}", ago(now_ms(), s.ts_ms)));
        cx.notify();
    }
}
