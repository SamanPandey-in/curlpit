//! The Code pane: the generated snippet for the active request, the language picker and
//! the values-or-placeholders toggle (FR-GEN).

use crate::ui::c;
use crate::workspace::Workspace;
use at_codegen::Lang;
use gpui::prelude::*;
use gpui::{div, px, ClipboardItem, Context};
use gpui_component::button::{Button, ButtonVariants};

impl Workspace {
    /// The Code view: language picker, values-or-placeholders toggle, Copy, and the snippet.
    pub(crate) fn render_code(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut bar = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_2();
        for (i, lang) in Lang::ALL.into_iter().enumerate() {
            let b = Button::new(("code-lang", i)).label(lang.label());
            let b = if self.code_lang == lang {
                b.primary()
            } else {
                b
            };
            bar = bar.child(b.on_click(cx.listener(move |this, _ev, _window, cx| {
                this.code_lang = lang;
                this.code_copied = false;
                cx.notify();
            })));
        }
        bar = bar
            .child(div().flex_1())
            .child(
                Button::new("code-vars")
                    .label(if self.code_keep_vars {
                        "Variables: {{kept}}"
                    } else {
                        "Variables: values"
                    })
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.code_keep_vars = !this.code_keep_vars;
                        this.code_copied = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("code-copy")
                    .label(if self.code_copied {
                        "Copied ✓"
                    } else {
                        "Copy"
                    })
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
            .children(warn.map(|w| div().px_2().pb_1().text_color(c(0xf9e2af)).child(w)))
            .child(
                div()
                    .id("code-view")
                    .flex_1()
                    .overflow_scroll()
                    .px_2()
                    .children(
                        lines
                            .into_iter()
                            .map(|l| div().h(px(18.)).flex_none().whitespace_nowrap().child(l)),
                    ),
            )
    }
}
