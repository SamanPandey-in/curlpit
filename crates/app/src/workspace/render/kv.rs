//! The key/value table shared by the Params, Headers and urlencoded-body tabs.

use crate::editor::Which;
use crate::request_tab::RequestTab;
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, Context};
use gpui_component::button::Button;
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::ActiveTheme as _;

impl Workspace {
    pub(crate) fn render_kv(
        &self,
        tab: &RequestTab,
        which: Which,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let (rows, en_id, del_id, add_id) = match which {
            Which::Params => (&tab.params, "p-en", "p-del", "p-add"),
            Which::Headers => (&tab.headers, "h-en", "h-del", "h-add"),
            Which::Form => (&tab.form, "f-en", "f-del", "f-add"),
        };
        let muted = cx.theme().muted_foreground;
        let mut col = div().flex().flex_col().gap_1().p_2().child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .text_color(muted)
                .child(div().w(px(24.)).flex_none())
                .child(div().flex_1().child("Key"))
                .child(div().flex_1().child("Value"))
                .child(div().w(px(36.)).flex_none()),
        );
        for (ix, row) in rows.iter().enumerate() {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div().w(px(24.)).flex_none().child(
                            Checkbox::new((en_id, ix))
                                .checked(row.enabled)
                                .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                                    if let Some(r) = this.tab_mut().rows_mut(which).get_mut(ix) {
                                        r.enabled = *checked;
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(div().flex_1().child(Input::new(&row.key)))
                    .child(div().flex_1().child(Input::new(&row.value)))
                    .child(div().w(px(36.)).flex_none().child(
                        Button::new((del_id, ix)).label("✕").on_click(cx.listener(
                            move |this, _ev, _window, cx| {
                                let rows = this.tab_mut().rows_mut(which);
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
            div().child(Button::new(add_id).label("+ Add").on_click(cx.listener(
                move |this, _ev, window, cx| {
                    let row = Self::new_row(window, cx, "", "");
                    this.tab_mut().rows_mut(which).push(row);
                    cx.notify();
                },
            ))),
        )
    }
}
