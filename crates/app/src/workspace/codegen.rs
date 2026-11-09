//! Turning the active request into code (FR-GEN) and back: the Code view's snippet, the
//! "Copy cURL" action, and pasting a cURL command in as a new tab. Also the secret-name
//! check that warns when a snippet would carry resolved secret values.

use crate::format::notes;
use crate::ui::Status;
use crate::workspace::{sendable, Workspace};
use at_codegen::{generate, parse_curl, Lang};
use at_core::{build, build_template, Request, Variables};
use gpui::{App, ClipboardItem, Context, Window};

impl Workspace {
    /// Names of enabled secret variables currently in effect (globals + active environment).
    pub(crate) fn secret_names(&self) -> Vec<String> {
        let env = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.variables.as_slice())
            .unwrap_or_default();
        self.globals
            .iter()
            .chain(env)
            .filter(|v| v.secret && v.enabled)
            .map(|v| v.key.clone())
            .collect()
    }

    /// The active tab's request as code in the chosen language, plus whether the snippet
    /// contains resolved secret values.
    pub(crate) fn code_snippet(&self, cx: &App) -> Result<(String, bool), String> {
        self.snippet_for(self.code_lang, cx)
    }

    /// The active tab's request as code in `lang`, plus whether the snippet holds resolved
    /// secret values. Shared by the Code view and "Copy cURL" (FR-IE-08).
    pub(crate) fn snippet_for(&self, lang: Lang, cx: &App) -> Result<(String, bool), String> {
        let vars = self.variables();
        let req = sendable(self.tab().current_request(cx), &vars);
        let none = Variables::default();
        let built = if self.code_keep_vars {
            build_template(&req, &none)
        } else {
            build(&req, &vars)
        }
        .map_err(|e| e.to_string())?;
        let has_secret = !self.code_keep_vars && {
            let used = build_template(&req, &none)
                .map(|b| b.unresolved)
                .unwrap_or_default();
            self.secret_names().iter().any(|s| used.contains(s))
        };
        Ok((generate(&built, lang), has_secret))
    }

    /// Copy the active tab's request as a cURL command (FR-IE-08). It honours the Code
    /// view's values-or-placeholders toggle, so a snippet meant for sharing stays shareable.
    pub(crate) fn on_copy_curl(&mut self, cx: &mut Context<Self>) {
        match self.snippet_for(Lang::Curl, cx) {
            Ok((code, has_secret)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(code));
                let msg = if has_secret {
                    "Copied as cURL. It contains secret values; switch Code to \"Variables: {{kept}}\" for a shareable snippet."
                } else {
                    "Copied as cURL."
                };
                self.tab_mut().status = Status::Info(msg.to_string());
            }
            Err(e) => self.tab_mut().status = Status::Error(format!("Copy as cURL: {e}")),
        }
        cx.notify();
    }

    /// Open the cURL command on the clipboard as a new (unsaved) request.
    pub(crate) fn on_paste_curl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
        match parse_curl(&text) {
            Ok((req, warnings)) => {
                if !self.tab().is_pristine(cx) {
                    self.add_tab(window, cx);
                }
                let tab = self.tab_mut();
                tab.apply_request(req, window, cx);
                tab.current_path = None;
                // Not saved anywhere yet: show it as unsaved so closing asks first.
                tab.saved = Request::default();
                tab.status = Status::Info(format!(
                    "Imported from the cURL command on the clipboard{}",
                    notes(&warnings)
                ));
            }
            Err(e) => self.tab_mut().status = Status::Error(format!("Clipboard: {e}")),
        }
        cx.notify();
    }
}
