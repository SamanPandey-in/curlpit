//! The Code pane: the generated snippet for the active request, the language picker and
//! the values-or-placeholders toggle (FR-GEN).

use crate::theme::{Palette, S1, S2};
use crate::ui::{quiet_btn, seg_btn_ix};
use crate::workspace::Workspace;
use at_codegen::Lang;
use gpui::prelude::*;
use gpui::{div, px, ClipboardItem, Context};

impl Workspace {
    /// The Code view: language picker, values-or-placeholders toggle, Copy, and the snippet.
    pub(crate) fn render_code(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let mut bar = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(S1))
            .p(px(S2));
        // The language picker is a segmented row like every other one in the app, not a
        // wall of accent-filled buttons.
        for (i, lang) in Lang::ALL.into_iter().enumerate() {
            bar = bar.child(
                seg_btn_ix("code-lang", i, lang.label(), self.code_lang == lang, cx).on_click(
                    cx.listener(move |this, _ev, _window, cx| {
                        this.code_lang = lang;
                        this.code_copied = false;
                        cx.notify();
                    }),
                ),
            );
        }
        bar = bar
            .child(div().flex_1())
            .child(
                quiet_btn(
                    "code-vars",
                    if self.code_keep_vars {
                        "Variables: {{kept}}"
                    } else {
                        "Variables: values"
                    }
                    .to_string(),
                    cx,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.code_keep_vars = !this.code_keep_vars;
                    this.code_copied = false;
                    cx.notify();
                })),
            )
            .child(
                quiet_btn(
                    "code-copy",
                    if self.code_copied {
                        "Copied ✓"
                    } else {
                        "Copy"
                    }
                    .to_string(),
                    cx,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    if let Ok((code, _)) = this.code_snippet(cx) {
                        cx.write_to_clipboard(ClipboardItem::new_string(code));
                        this.code_copied = true;
                        cx.notify();
                    }
                })),
            );

        let (lines, warn): (Vec<String>, Option<String>) = match self.code_snippet(cx) {
            Ok((code, secret)) => (
                code.lines().map(|l| l.replace('\t', "    ")).collect(),
                secret.then(|| {
                    "This snippet contains secret values. Switch to \"Variables: {{kept}}\" before sharing it."
                        .to_string()
                }),
            ),
            Err(e) => (Vec::new(), Some(format!("Can't generate code yet: {e}"))),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bar)
            .children(warn.map(|w| {
                div()
                    .px(px(S2))
                    .pb(px(S1))
                    .text_color(p.warning_text())
                    .child(w)
            }))
            .child(
                div()
                    .id("code-view")
                    .flex_1()
                    .overflow_scroll()
                    .px(px(S2))
                    .children(
                        lines
                            .into_iter()
                            .map(|l| div().h(px(18.)).flex_none().whitespace_nowrap().child(l)),
                    ),
            )
    }
}
