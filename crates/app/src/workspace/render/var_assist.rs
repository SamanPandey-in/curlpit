//! The workspace's side of `{{variable}}` assistance (`crate::var_complete` holds the logic):
//! publishing the variables in effect, hooking the request inputs up to them, the row of
//! undefined-variable chips under the URL bar, and "Create variable…", which opens the
//! Variables page with a row for the name ready to fill in.

use crate::editor::KvRow;
use crate::sidebar::{MainView, Scope};
use crate::theme::{Palette, S1, S2, SMALL_TEXT};
use crate::var_complete::{attach, catalog, refresh, sync};
use crate::workspace::Workspace;
use at_core::{build_template, EnvVar};
use gpui::prelude::*;
use gpui::{div, px, Context, Entity, Window};
use gpui_component::input::InputState;
use gpui_component::tooltip::Tooltip;

impl Workspace {
    /// Publish the variables in effect and make sure the active tab's URL and key/value value
    /// cells have highlighting, hover and completion, with colours for the current variables.
    pub(super) fn var_assist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let env = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.variables.as_slice())
            .unwrap_or_default();
        sync(catalog(&self.globals, env), cx);
        if self.main_view != MainView::Request {
            return;
        }
        let tab = self.tab();
        let values = |rows: &[KvRow]| rows.iter().map(|r| r.value.clone()).collect::<Vec<_>>();
        let mut inputs: Vec<Entity<InputState>> = vec![tab.url.clone()];
        inputs.extend(values(&tab.params));
        inputs.extend(values(&tab.headers));
        inputs.extend(values(&tab.form));
        for input in &inputs {
            attach(input, cx);
            refresh(input, window, cx);
        }
    }

    /// Names the active request uses but no scope defines (dynamic `$` names excluded), each
    /// as a chip that creates it. `None` when there are none.
    pub(super) fn render_var_chips(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let mut vars = self.variables();
        vars.keep_dynamic = true;
        let missing = build_template(&self.tab().current_request(cx), &vars)
            .map(|b| b.unresolved)
            .unwrap_or_default();
        if missing.is_empty() {
            return None;
        }
        let p = Palette::of(cx);
        let target = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| format!("the \"{}\" environment", e.name))
            .unwrap_or_else(|| "global variables".to_string());
        let err = p.status_error();
        let mut row = div()
            .flex_none()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(S1 + 2.0))
            .px(px(S2 + 2.0))
            .pb(px(S1 + 2.0))
            .text_size(px(SMALL_TEXT))
            .child(div().text_color(p.text_muted()).child("Undefined:"));
        for (ix, name) in missing.into_iter().enumerate() {
            let tip = format!("Create variable \"{name}\" in {target}");
            let label = format!("+ {name}");
            row = row.child(
                div()
                    .id(("var-chip", ix))
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(3.))
                    .bg(err.alpha(0.14))
                    .text_color(err)
                    .cursor_pointer()
                    .hover(move |s| s.bg(err.alpha(0.26)))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .child(label)
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.create_variable(&name, window, cx)
                    })),
            );
        }
        Some(row)
    }

    /// "Create variable…": open the Variables page on the active environment (globals when
    /// there is none) with a row for `name` and its value field focused. An existing row with
    /// that name is focused instead of duplicated. Edits on the page autosave as usual (D25).
    pub(crate) fn create_variable(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scope = self.active_env.map(Scope::Env).unwrap_or(Scope::Globals);
        // Saves pending edits first; if that fails it stays put and says why.
        self.select_scope(scope, window, cx);
        if self.env_scope != scope {
            self.open_envs(cx);
            return;
        }
        let existing = self
            .env_rows
            .iter()
            .position(|r| r.key.read(cx).value().trim() == name);
        let ix = match existing {
            Some(ix) => ix,
            None => {
                let row = Self::new_env_row(window, cx, &EnvVar::new(name, ""));
                // Keep the empty spare row last.
                let spare_last = self
                    .env_rows
                    .last()
                    .is_some_and(|r| r.key.read(cx).value().is_empty());
                let at = if spare_last {
                    self.env_rows.len() - 1
                } else {
                    self.env_rows.len()
                };
                self.env_rows.insert(at, row);
                at
            }
        };
        if let Some(r) = self.env_rows.get(ix) {
            let value = r.value.clone();
            value.update(cx, |s, cx| s.focus(window, cx));
        }
        self.open_envs(cx);
    }
}
