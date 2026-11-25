//! The key/value table shared by the Params, Headers and urlencoded-body tabs.

use crate::editor::Which;
use crate::request_tab::RequestTab;
use crate::theme::{Palette, S1, S2, SMALL_TEXT};
use crate::ui::{icon_btn, quiet_btn};
use crate::var_complete::var_input;
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, Context};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::{IconName, Sizable as _};

/// Width of the enable column and of the delete column.
const EDGE_W: f32 = 28.0;

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
        let p = Palette::of(cx);
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(S1))
            .px(px(S2))
            .py(px(S2))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(S2))
                    .pb(px(2.))
                    .text_size(px(SMALL_TEXT))
                    .text_color(p.text_placeholder())
                    .child(div().w(px(EDGE_W)).flex_none())
                    .child(div().flex_1().pl(px(S2)).child("Key"))
                    .child(div().flex_1().pl(px(S2)).child("Value"))
                    .child(div().w(px(EDGE_W)).flex_none()),
            );
        for (ix, row) in rows.iter().enumerate() {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(S2))
                    .child(
                        div()
                            .w(px(EDGE_W))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(Checkbox::new((en_id, ix)).checked(row.enabled).on_click(
                                cx.listener(move |this, checked: &bool, _window, cx| {
                                    if let Some(r) = this.tab_mut().rows_mut(which).get_mut(ix) {
                                        r.enabled = *checked;
                                    }
                                    cx.notify();
                                }),
                            )),
                    )
                    // A disabled row stays editable but reads as switched off.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(!row.enabled, |d| d.opacity(0.5))
                            .child(Input::new(&row.key).small()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(!row.enabled, |d| d.opacity(0.5))
                            .child(var_input(&row.value, Input::new(&row.value).small())),
                    )
                    .child(div().w(px(EDGE_W)).flex_none().child(
                        icon_btn((del_id, ix), IconName::Close, "Remove row", cx).on_click(
                            cx.listener(move |this, _ev, _window, cx| {
                                let rows = this.tab_mut().rows_mut(which);
                                if ix < rows.len() {
                                    rows.remove(ix);
                                }
                                cx.notify();
                            }),
                        ),
                    )),
            );
        }
        col.child(
            div().flex().pl(px(EDGE_W + S2 - S1)).child(
                quiet_btn(add_id, "Add".to_string(), cx)
                    .icon(IconName::Plus)
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        let row = Self::new_row(window, cx, "", "");
                        this.tab_mut().rows_mut(which).push(row);
                        cx.notify();
                    })),
            ),
        )
    }
}
