//! The trust prompt for imported collections with scripts (G-16): an inline banner under
//! the URL bar of a tab whose Send was held back, and the same choice on the runner page.

use crate::theme::{Palette, S1, S2, S3};
use crate::workspace::send::{TrustGate, Untrusted};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Icon, IconName, Sizable as _};

/// "This collection was imported with N scripts. …"
pub(crate) fn trust_message(u: &Untrusted, what: &str) -> String {
    let n = match u.scripts {
        1 => "1 script".to_string(),
        n => format!("{n} scripts"),
    };
    format!(
        "\"{}\" was imported with {n}. Scripts don't run until you trust the collection; {what}.",
        u.name
    )
}

impl Workspace {
    /// The banner for the active tab, if its last Send is waiting on a trust decision.
    pub(crate) fn render_trust_banner(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let id = self.tab().id;
        let u = TrustGate::tab(cx, id)?;
        let text = trust_message(&u, "this request was not sent");
        Some(
            trust_bar(text, cx)
                .child(
                    Button::new("trust-collection")
                        .small()
                        .primary()
                        .label("Trust collection")
                        .on_click(
                            cx.listener(move |this, _ev, _window, cx| this.trust_and_send(id, cx)),
                        ),
                )
                .child(
                    Button::new("send-no-scripts")
                        .small()
                        .label("Send without scripts")
                        .on_click(cx.listener(move |this, _ev, _window, cx| {
                            this.send_without_scripts(id, cx)
                        })),
                )
                .child(
                    Button::new("trust-dismiss")
                        .small()
                        .ghost()
                        .label("Cancel")
                        .on_click(
                            cx.listener(move |this, _ev, _window, cx| this.dismiss_trust(id, cx)),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// The banner frame: warning icon and message; the caller adds the buttons.
pub(crate) fn trust_bar(text: String, cx: &Context<Workspace>) -> gpui::Div {
    let p = Palette::of(cx);
    div()
        .flex_none()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(S2))
        .px(px(S3))
        .py(px(S1 + 2.0))
        .bg(p.muted_surface())
        .border_b_1()
        .border_color(p.border())
        .child(
            Icon::new(IconName::TriangleAlert)
                .small()
                .text_color(p.warning_text()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(200.))
                .text_color(p.text())
                .child(text),
        )
}
