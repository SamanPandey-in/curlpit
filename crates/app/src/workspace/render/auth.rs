//! The Auth editor tab: one set of inputs per auth type, with the fields of the types
//! that have no editor yet shown as "kept from file".

use crate::editor::AuthKind;
use crate::request_tab::RequestTab;
use crate::ui::seg_btn_ix;
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, Context, Entity};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};
use gpui_component::ActiveTheme as _;

impl Workspace {
    pub(crate) fn render_auth(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mut kinds = div().flex().flex_row().flex_wrap().gap_1();
        for (i, kind) in AuthKind::EDITABLE.into_iter().enumerate() {
            kinds = kinds.child(
                seg_btn_ix("auth-kind", i, kind.label(), tab.auth_kind == kind, cx).on_click(
                    cx.listener(move |this, _ev, _window, cx| {
                        this.tab_mut().auth_kind = kind;
                        cx.notify();
                    }),
                ),
            );
        }
        if tab.kept_auth().is_some() {
            kinds = kinds.child(seg_btn_ix(
                "auth-kind",
                AuthKind::EDITABLE.len(),
                AuthKind::Kept.label(),
                tab.auth_kind == AuthKind::Kept,
                cx,
            ));
        }
        let row = |label: &'static str, input: &Entity<InputState>| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().w(px(110.)).flex_none().text_color(muted).child(label))
                .child(div().flex_1().child(Input::new(input)))
        };
        let reveal = Button::new("auth-reveal")
            .label(if tab.auth_reveal { "Hide" } else { "Show" })
            .on_click(cx.listener(|this, _ev, window, cx| {
                let tab = this.tab_mut();
                tab.auth_reveal = !tab.auth_reveal;
                tab.set_auth_masked(window, cx);
                cx.notify();
            }));
        let fields = match tab.auth_kind {
            AuthKind::None => div()
                .text_color(muted)
                .child("No authorization is added to the request.")
                .into_any_element(),
            AuthKind::Basic => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(row("Username", &tab.auth_user))
                .child(row("Password", &tab.auth_pass))
                .child(div().child(reveal))
                .child(
                    div()
                        .text_color(muted)
                        .child("Sent as Authorization: Basic <base64 of username:password>."),
                )
                .into_any_element(),
            AuthKind::Bearer => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(row("Token", &tab.auth_token))
                .child(div().child(reveal))
                .child(
                    div()
                        .text_color(muted)
                        .child("Sent as Authorization: Bearer <token>."),
                )
                .into_any_element(),
            AuthKind::ApiKey => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(row("Key", &tab.auth_key))
                .child(row("Value", &tab.auth_value))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .child(
                            seg_btn_ix("auth-in", 0, "Header", !tab.auth_in_query, cx).on_click(
                                cx.listener(|this, _ev, _window, cx| {
                                    this.tab_mut().auth_in_query = false;
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            seg_btn_ix("auth-in", 1, "Query param", tab.auth_in_query, cx).on_click(
                                cx.listener(|this, _ev, _window, cx| {
                                    this.tab_mut().auth_in_query = true;
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(reveal),
                )
                .into_any_element(),
            AuthKind::Kept => div()
                .text_color(muted)
                .child(format!(
                    "This request uses {} from its file. It is sent and saved unchanged; pick another type above to replace it.",
                    tab.kept_auth().unwrap_or("an auth type")
                ))
                .into_any_element(),
        };
        div()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(kinds)
            .child(fields)
            .child(
                div()
                    .text_color(muted)
                    .child("{{variables}} work in every field. A header you add yourself with the same name wins."),
            )
    }
}
