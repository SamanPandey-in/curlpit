//! Drawing the command palette overlay (FR-KEY-01): a dimmed backdrop and a centred panel
//! with the search box above the ranked command list.
use gpui::prelude::*;
use gpui::{div, hsla, px, Context, MouseButton};
use gpui_component::input::Input;
use gpui_component::{Icon, IconName, Sizable as _};

use crate::theme::{Palette, S1, S2, SMALL_TEXT};
use crate::workspace::Workspace;

impl Workspace {
    pub(crate) fn render_palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let Some(state) = self.palette.as_ref() else {
            // Only drawn when open; this arm keeps the return type a single `Div`.
            return div();
        };

        let rows = state.order.iter().enumerate().map(|(pos, &ci)| {
            let cmd = &state.commands[ci];
            let selected = pos == state.selected;
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .px(px(S2))
                .py(px(S1))
                .flex_none()
                .rounded(px(4.))
                .cursor_pointer()
                .when(selected, |d| d.bg(p.element_selected()))
                .when(!selected, |d| {
                    let hover = p.element_hover();
                    d.hover(move |s| s.bg(hover))
                })
                .child(div().text_color(p.text()).child(cmd.label.clone()))
                .child(
                    div()
                        .text_size(px(SMALL_TEXT))
                        .text_color(p.text_placeholder())
                        .child(cmd.category.to_string()),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev, window, cx| this.palette_click(pos, window, cx)),
                )
        });

        let panel = div()
            .mt(px(80.))
            .w(px(560.))
            .max_h(px(440.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(p.elevated_surface())
            .border_1()
            .border_color(p.border())
            .rounded(px(8.))
            .shadow_lg()
            .key_context("Palette")
            // Clicks inside the panel must not reach the backdrop's dismiss handler.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _ev, _window, cx| cx.stop_propagation()),
            )
            .child(
                div()
                    .px(px(S2))
                    .py(px(S1))
                    .border_b_1()
                    .border_color(p.border())
                    .child(
                        Input::new(&self.palette_query).appearance(false).prefix(
                            Icon::new(IconName::Search)
                                .small()
                                .text_color(p.text_placeholder()),
                        ),
                    ),
            )
            .child(
                div()
                    .id("palette-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .p(px(S1))
                    .children(rows),
            );

        // The backdrop: dim the window and dismiss on a click outside the panel.
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .bg(hsla(0., 0., 0., 0.3))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev, _window, cx| this.close_palette(cx)),
            )
            .child(panel)
    }
}
