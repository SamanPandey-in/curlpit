//! The Settings editor tab: per-request timeout, redirect following and TLS
//! verification.

use crate::request_tab::RequestTab;
use crate::theme::Palette;
use crate::ui::seg_btn_ix;
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, Context};
use gpui_component::input::Input;

impl Workspace {
    pub(crate) fn render_settings(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let muted = p.text_muted();
        div()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .text_color(muted)
                            .child("Timeout (ms)"),
                    )
                    .child(div().w(px(140.)).flex_none().child(Input::new(&tab.timeout)))
                    .child(div().text_color(muted).child("0 = no timeout")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        seg_btn_ix(
                            "set-redirect",
                            0,
                            if tab.follow_redirects {
                                "Follow redirects: on"
                            } else {
                                "Follow redirects: off"
                            },
                            tab.follow_redirects,
                            cx,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            let tab = this.tab_mut();
                            tab.follow_redirects = !tab.follow_redirects;
                            cx.notify();
                        })),
                    )
                    .child(
                        seg_btn_ix(
                            "set-tls",
                            0,
                            if tab.verify_tls {
                                "Verify TLS certificates: on"
                            } else {
                                "Verify TLS certificates: off"
                            },
                            tab.verify_tls,
                            cx,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            let tab = this.tab_mut();
                            tab.verify_tls = !tab.verify_tls;
                            cx.notify();
                        })),
                    ),
            )
            .children((!tab.verify_tls).then(|| {
                div()
                    .text_color(p.warning_text())
                    .child("Certificate checks are off for this request: only use this for local or test servers.")
            }))
    }
}
