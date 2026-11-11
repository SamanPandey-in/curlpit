//! The cookie jar page: the stored cookies with their expiry, and the clear control
//! (FR-COOKIE).

use crate::format::expires_label;
use crate::sidebar::MainView;
use crate::ui::{c, mono};
use crate::workspace::Workspace;
use at_history::now_ms;
use gpui::prelude::*;
use gpui::{div, px, Context};
use gpui_component::button::Button;
use gpui_component::ActiveTheme as _;

impl Workspace {
    pub(crate) fn render_cookies(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let cookies = self.cookies.list();
        let now = now_ms() / 1000;
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .p_2()
            .child(div().text_color(muted).child("COOKIES"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .children((!cookies.is_empty()).then(|| {
                        Button::new("cookies-clear")
                            .label(if self.confirm_clear_cookies {
                                "Click again to delete all"
                            } else {
                                "Delete all"
                            })
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                if this.confirm_clear_cookies {
                                    this.confirm_clear_cookies = false;
                                    this.cookies.clear();
                                    this.save_cookies_if_changed();
                                } else {
                                    this.confirm_clear_cookies = true;
                                }
                                cx.notify();
                            }))
                    }))
                    .child(
                        Button::new("cookies-back")
                            .label("← Back to requests")
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.main_view = MainView::Request;
                                cx.notify();
                            })),
                    ),
            );
        let mut list = div()
            .id("cookie-list")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px_2();
        if cookies.is_empty() {
            list = list.child(div().p_2().text_color(muted).child(
                "No cookies yet. Cookies that servers set (Set-Cookie) are kept here and sent back automatically with matching requests, in tabs and in runs.",
            ));
        }
        let mut last_domain: Option<String> = None;
        for (ix, ck) in cookies.iter().enumerate() {
            if last_domain.as_deref() != Some(ck.domain.as_str()) {
                let domain = ck.domain.clone();
                list = list.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .pt_2()
                        .pb_1()
                        .border_b_1()
                        .border_color(border)
                        .child(div().child(ck.domain.clone()))
                        .child(
                            Button::new(("cookie-domain-clear", ix))
                                .label("Delete domain")
                                .on_click(cx.listener(move |this, _ev, _window, cx| {
                                    this.cookies.clear_domain(&domain);
                                    this.save_cookies_if_changed();
                                    cx.notify();
                                })),
                        ),
                );
                last_domain = Some(ck.domain.clone());
            }
            let flags = [
                ck.secure.then_some("Secure"),
                ck.http_only.then_some("HttpOnly"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let (d, p, n) = (ck.domain.clone(), ck.path.clone(), ck.name.clone());
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .child(
                        div()
                            .w(px(160.))
                            .flex_none()
                            .truncate()
                            .child(ck.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .font_family(mono())
                            .child(ck.value.clone()),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .text_color(muted)
                            .truncate()
                            .child(ck.path.clone()),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .text_color(muted)
                            .child(expires_label(now, ck.expires)),
                    )
                    .child(div().w(px(120.)).flex_none().text_color(muted).child(flags))
                    .child(
                        Button::new(("cookie-del", ix))
                            .label("✕")
                            .on_click(cx.listener(move |this, _ev, _window, cx| {
                                this.cookies.remove(&d, &p, &n);
                                this.save_cookies_if_changed();
                                cx.notify();
                            })),
                    ),
            );
        }
        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .children(
                self.cookie_note
                    .clone()
                    .map(|n| div().px_2().text_color(c(0xf38ba8)).child(n)),
            )
            .child(list)
    }
}
