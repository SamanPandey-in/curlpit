//! The Body editor tab: the mode and raw-flavour pickers with the text editor, and the
//! separate table the form-data mode needs for its text-or-file rows.

use crate::editor::{has_body, BodyMode, RawKind, Which};
use crate::request_tab::RequestTab;
use crate::ui::seg_btn_ix;
use crate::workspace::Workspace;
use at_core::{FormField, FormValue};
use gpui::prelude::*;
use gpui::{div, px, Context};
use gpui_component::button::Button;
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::ActiveTheme as _;

impl Workspace {
    pub(crate) fn render_body(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mut modes = div().flex().flex_row().flex_wrap().gap_1();
        for (i, (mode, label)) in [
            (BodyMode::None, "None"),
            (BodyMode::Raw, "Raw"),
            (BodyMode::UrlEncoded, "urlencoded"),
            (BodyMode::FormData, "form-data"),
        ]
        .into_iter()
        .enumerate()
        {
            modes = modes.child(
                seg_btn_ix("body-mode", i, label, tab.body_mode == mode, cx).on_click(cx.listener(
                    move |this, _ev, _window, cx| {
                        this.tab_mut().body_mode = mode;
                        cx.notify();
                    },
                )),
            );
        }
        if tab.body_mode == BodyMode::Raw {
            modes = modes.child(div().w(px(12.)));
            for (i, kind) in RawKind::ALL.into_iter().enumerate() {
                modes = modes.child(
                    seg_btn_ix("raw-kind", i, kind.label(), tab.raw_kind == kind, cx).on_click(
                        cx.listener(move |this, _ev, _window, cx| {
                            this.tab_mut().raw_kind = kind;
                            cx.notify();
                        }),
                    ),
                );
            }
        }
        let note = (!has_body(tab.method)).then(|| {
            div().text_color(muted).child(format!(
                "{} requests don't send a body.",
                tab.method.as_str()
            ))
        });
        let editor = match tab.body_mode {
            BodyMode::None => div().text_color(muted).child("No body.").into_any_element(),
            BodyMode::Raw => div()
                .flex_1()
                .min_h(px(120.))
                .child(Input::new(&tab.body).h_full())
                .into_any_element(),
            BodyMode::UrlEncoded => self.render_kv(tab, Which::Form, cx).into_any_element(),
            BodyMode::FormData => self.render_multipart(tab, cx).into_any_element(),
        };
        div()
            .p_2()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(modes)
            .children(note)
            .child(editor)
    }

    pub(crate) fn render_multipart(
        &self,
        tab: &RequestTab,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let id = tab.id;
        let mut col = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .text_color(muted)
                .child(div().w(px(24.)).flex_none())
                .child(div().flex_1().child("Key"))
                .child(div().w(px(64.)).flex_none().child("Type"))
                .child(div().flex_1().child("Value / file"))
                .child(div().w(px(36.)).flex_none()),
        );
        for (ix, row) in tab.multipart.iter().enumerate() {
            let choose = row.is_file.then(|| {
                Button::new(("fd-choose", ix))
                    .label("Choose…")
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.choose_form_file(id, ix, window, cx)
                    }))
            });
            col =
                col.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(24.)).flex_none().child(
                            Checkbox::new(("fd-en", ix)).checked(row.enabled).on_click(
                                cx.listener(move |this, checked: &bool, _window, cx| {
                                    if let Some(r) = this.tab_mut().multipart.get_mut(ix) {
                                        r.enabled = *checked;
                                    }
                                    cx.notify();
                                }),
                            ),
                        ))
                        .child(div().flex_1().child(Input::new(&row.key)))
                        .child(
                            div().w(px(64.)).flex_none().child(
                                Button::new(("fd-type", ix))
                                    .label(if row.is_file { "File" } else { "Text" })
                                    .on_click(cx.listener(move |this, _ev, _window, cx| {
                                        if let Some(r) = this.tab_mut().multipart.get_mut(ix) {
                                            r.is_file = !r.is_file;
                                        }
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .child(div().flex_1().child(Input::new(&row.value)))
                                .children(choose),
                        )
                        .child(div().w(px(36.)).flex_none().child(
                            Button::new(("fd-del", ix)).label("✕").on_click(cx.listener(
                                move |this, _ev, _window, cx| {
                                    let rows = &mut this.tab_mut().multipart;
                                    if ix < rows.len() {
                                        rows.remove(ix);
                                    }
                                    cx.notify();
                                },
                            )),
                        )),
                );
        }
        col.child(
            div().child(Button::new("fd-add").label("+ Add").on_click(cx.listener(
                |this, _ev, window, cx| {
                    let row = Self::new_form_row(
                        window,
                        cx,
                        &FormField {
                            key: String::new(),
                            value: FormValue::Text {
                                value: String::new(),
                            },
                            enabled: true,
                        },
                    );
                    this.tab_mut().multipart.push(row);
                    cx.notify();
                },
            ))),
        )
    }
}
