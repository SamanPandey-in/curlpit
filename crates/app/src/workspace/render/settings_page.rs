//! Drawing the Settings screen (FR-SET-02, 03): the app-wide defaults as a settings form
//! — a label column with a hint under each label, a control column — under the shared
//! page header, whose action is Save.

use crate::theme::{Palette, S1, S2, S3, SMALL_TEXT};
use crate::ui::Status;
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::switch::Switch;
use gpui_component::{Icon, IconName, Sizable as _};

/// Width of the label column.
const LABEL_W: f32 = 280.0;

impl Workspace {
    pub(crate) fn render_settings_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let data_dir = self
            .store
            .as_ref()
            .map(|s| s.data_dir().display().to_string())
            .unwrap_or_else(|| "(storage unavailable)".into());

        let row = |label: &'static str, hint: &'static str, control: AnyElement| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(S3))
                .child(
                    div()
                        .w(px(LABEL_W))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .child(div().text_color(p.text()).child(label))
                        .child(
                            div()
                                .text_size(px(SMALL_TEXT))
                                .text_color(p.text_placeholder())
                                .child(hint),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(420.))
                        .flex()
                        .child(control),
                )
        };

        let save = Button::new("settings-save")
            .small()
            .primary()
            .label("Save")
            .on_click(cx.listener(|this, _ev, _window, cx| this.save_settings_page(cx)))
            .into_any_element();
        let header = self.page_header(
            "Settings",
            Some(save),
            |this, _window, cx| {
                this.main_view = crate::sidebar::MainView::Request;
                cx.notify();
            },
            cx,
        );

        let note = match &self.status {
            Status::Info(m) => Some((m.clone(), p.text_muted(), IconName::Info)),
            Status::Error(e) => Some((e.clone(), p.status_error(), IconName::CircleX)),
            _ => None,
        };

        let form = div()
            .id("settings-form")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(S3))
            .flex()
            .flex_col()
            .gap(px(S3 + S1))
            .child(row(
                "Default timeout",
                "Milliseconds, 0 for none. Applies to new tabs.",
                div()
                    .w(px(140.))
                    .child(Input::new(&self.set_timeout).small())
                    .into_any_element(),
            ))
            .child(row(
                "Verify TLS certificates",
                "Default for new tabs.",
                Switch::new("settings-tls")
                    .checked(self.set_verify_tls)
                    .on_click(cx.listener(|this, on: &bool, _window, cx| {
                        if *on != this.set_verify_tls {
                            this.toggle_setting_tls(cx);
                        }
                    }))
                    .into_any_element(),
            ))
            .child(row(
                "History limit",
                "Entries kept. Applies next launch.",
                div()
                    .w(px(140.))
                    .child(Input::new(&self.set_history_cap).small())
                    .into_any_element(),
            ))
            .child(row(
                "Outbound proxy",
                "Proxy URL; empty for a direct connection.",
                Input::new(&self.set_proxy)
                    .small()
                    .flex_1()
                    .into_any_element(),
            ))
            .child(row(
                "Data directory",
                "Collections, history and settings.",
                div()
                    .font_family(crate::ui::mono())
                    .text_size(px(SMALL_TEXT))
                    .text_color(p.text_muted())
                    .child(data_dir)
                    .into_any_element(),
            ))
            .children(note.map(|(m, col, icon)| {
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(S2))
                    .text_color(col)
                    .child(Icon::new(icon).small().flex_none())
                    .child(div().flex_1().min_w_0().child(m))
            }))
            .child(
                div()
                    .text_size(px(SMALL_TEXT))
                    .text_color(p.text_placeholder())
                    .child("This app sends no telemetry."),
            );

        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(header)
            .child(form)
    }
}
