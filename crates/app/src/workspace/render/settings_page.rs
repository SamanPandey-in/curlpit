//! Drawing the Settings screen (FR-SET-02, 03): the global defaults with a Save button.
use crate::theme::Palette;
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context};
use gpui_component::button::Button;
use gpui_component::input::Input;

impl Workspace {
    pub(crate) fn render_settings_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let (muted, text) = (p.text_muted(), p.text());
        let data_dir = self
            .store
            .as_ref()
            .map(|s| s.data_dir().display().to_string())
            .unwrap_or_else(|| "(storage unavailable)".into());

        let labeled = move |label: &str, control: AnyElement| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .py_1()
                .child(div().w(px(190.)).text_color(muted).child(label.to_string()))
                .child(div().flex_1().child(control))
        };

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .p_2()
            .child(div().text_color(muted).child("SETTINGS"))
            .child(
                Button::new("settings-save")
                    .label("Save")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.save_settings_page(cx))),
            );

        div().flex().flex_col().size_full().child(header).child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(labeled(
                    "Default timeout (ms)",
                    Input::new(&self.set_timeout).into_any_element(),
                ))
                .child(labeled(
                    "TLS verification",
                    Button::new("settings-tls")
                        .label(if self.set_verify_tls { "On" } else { "Off" })
                        .on_click(cx.listener(|this, _ev, _window, cx| this.toggle_setting_tls(cx)))
                        .into_any_element(),
                ))
                .child(labeled(
                    "History cap (entries)",
                    Input::new(&self.set_history_cap).into_any_element(),
                ))
                .child(labeled(
                    "Outbound proxy",
                    Input::new(&self.set_proxy).into_any_element(),
                ))
                .child(labeled(
                    "Data directory",
                    div().text_color(text).child(data_dir).into_any_element(),
                ))
                .child(
                    div()
                        .pt_2()
                        .text_color(muted)
                        .child("This app sends no telemetry."),
                ),
        )
    }
}
