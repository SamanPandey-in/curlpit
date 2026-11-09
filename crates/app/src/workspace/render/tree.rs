//! The response's JSON tree pane: fold controls, the copy buttons, and a virtualized
//! list of tree rows coloured by JSON type (FR-RES-03).

use crate::tree::{tree_value_display, TreeCopy};
use crate::ui::{c, kind_color};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, uniform_list, AnyElement, Context};
use gpui_component::button::Button;
use gpui_component::ActiveTheme as _;

impl Workspace {
    /// The Tree view: fold controls, the three copy buttons, and a virtualized tree of the
    /// response (FR-RES-03).
    pub(crate) fn render_tree(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, accent) = (theme.muted_foreground, theme.accent);
        let tab = self.tab();
        let (tv, data) = match (tab.tree.as_ref(), tab.view.as_ref()) {
            (Some(Ok(tv)), Some(v)) => (tv, v.data.clone()),
            (Some(Err(e)), _) => {
                return div()
                    .p_4()
                    .text_color(muted)
                    .child(e.clone())
                    .into_any_element()
            }
            _ => {
                return div()
                    .p_4()
                    .text_color(muted)
                    .child("Send a request to see the response")
                    .into_any_element()
            }
        };

        let bar =
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .p_2()
                .child(
                    Button::new("tree-expand")
                        .label("Expand all")
                        .on_click(cx.listener(|this, _ev, _window, cx| this.tree_fold(true, cx))),
                )
                .child(
                    Button::new("tree-collapse")
                        .label("Collapse all")
                        .on_click(cx.listener(|this, _ev, _window, cx| this.tree_fold(false, cx))),
                )
                .child(div().flex_1())
                .child(Button::new("tree-copy-value").label("Copy value").on_click(
                    cx.listener(|this, _ev, _window, cx| this.tree_copy(TreeCopy::Value, cx)),
                ))
                .child(Button::new("tree-copy-path").label("Copy path").on_click(
                    cx.listener(|this, _ev, _window, cx| this.tree_copy(TreeCopy::Path, cx)),
                ))
                .child(Button::new("tree-copy-all").label("Copy all").on_click(
                    cx.listener(|this, _ev, _window, cx| this.tree_copy(TreeCopy::All, cx)),
                ));

        let handle = cx.entity();
        let rows = tv.rows.clone();
        let tree = tv.tree.clone();
        let selected = tv.selected;
        let key_col = c(0x89b4fa);
        let list =
            uniform_list("json-tree", rows.len(), move |range, _window, _cx| {
                let mut items = Vec::new();
                for i in range {
                    let Some(&(ix, folded)) = rows.get(i) else {
                        items.push(div().h(px(18.)).into_any_element());
                        continue;
                    };
                    let Some(node) = tree.nodes().get(ix as usize) else {
                        items.push(div().h(px(18.)).into_any_element());
                        continue;
                    };
                    let marker = if !node.kind.is_container() {
                        " "
                    } else if folded {
                        "\u{25b8}"
                    } else {
                        "\u{25be}"
                    };
                    let key = tree.key_text(&data[..], ix);
                    let value = tree_value_display(&tree, &data[..], ix);
                    let handle = handle.clone();
                    items.push(
                        div()
                            .id(("tree-row", i))
                            .flex()
                            .flex_row()
                            .items_center()
                            .h(px(18.))
                            .gap_1()
                            .pr_2()
                            .pl(px(8.0 + f32::from(node.depth.min(40)) * 14.0))
                            .when(selected == Some(ix), |d| d.bg(accent))
                            .on_click(move |_ev, _window, cx| {
                                handle.update(cx, |this, cx| this.on_tree_row(ix, cx));
                            })
                            .child(div().w(px(12.)).flex_none().text_color(muted).child(marker))
                            .children(key.map(|k| {
                                div().flex_none().text_color(key_col).child(format!("{k}:"))
                            }))
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_color(kind_color(node.kind))
                                    .child(value),
                            )
                            .into_any_element(),
                    );
                }
                items
            })
            .track_scroll(tab.tree_scroll.clone())
            .h_full();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bar)
            .child(div().flex_1().overflow_hidden().child(list))
            .into_any_element()
    }
}
