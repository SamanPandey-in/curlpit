//! The Settings editor tab: per-request timeout, redirect following and TLS
//! verification, laid out as a settings form — a label column and a control column —
//! with real switches for the two on/off options.

use crate::request_tab::RequestTab;
use crate::theme::{Palette, S1, S2, S3, SMALL_TEXT};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context};
use gpui_component::input::Input;
use gpui_component::switch::Switch;
use gpui_component::{Icon, IconName, Sizable as _};

/// Width of the label column.
const LABEL_W: f32 = 170.0;

impl Workspace {
    pub(crate) fn render_settings(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
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
                .child(control)
        };
        div()
            .p(px(S3))
            .flex()
            .flex_col()
            .gap(px(S3 + S1))
            .child(row(
                "Timeout",
                "Milliseconds; 0 for none",
                div()
                    .w(px(140.))
                    .flex_none()
                    .child(Input::new(&tab.timeout).small())
                    .into_any_element(),
            ))
            .child(row(
                "Follow redirects",
                "Up to 10 hops",
                Switch::new("set-redirect")
                    .checked(tab.follow_redirects)
                    .on_click(cx.listener(|this, on: &bool, _window, cx| {
                        this.tab_mut().follow_redirects = *on;
                        cx.notify();
                    }))
                    .into_any_element(),
            ))
            .child(row(
                "Verify TLS certificates",
                "Reject invalid or self-signed",
                Switch::new("set-tls")
                    .checked(tab.verify_tls)
                    .on_click(cx.listener(|this, on: &bool, _window, cx| {
                        this.tab_mut().verify_tls = *on;
                        cx.notify();
                    }))
                    .into_any_element(),
            ))
            .children((!tab.verify_tls).then(|| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(S2))
                    .text_color(p.warning_text())
                    .child(Icon::new(IconName::TriangleAlert).small())
                    .child(
                        "Certificate checks are off for this request: only use this for \
                         local or test servers.",
                    )
            }))
    }
}
