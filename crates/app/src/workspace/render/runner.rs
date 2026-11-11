//! The collection runner page: target and options, the live result rows, and the
//! summary with its copy-report button (FR-RUN).

use crate::format::human;
use crate::runner::RunResult;
use crate::sidebar::MainView;
use crate::ui::{c, seg_btn_ix, status_color};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, ClipboardItem, Context};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::ActiveTheme as _;

impl Workspace {
    pub(crate) fn render_runner(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let (green, red) = (c(0xa6e3a1), c(0xf38ba8));
        let r = &self.runner;
        let running = r.cancel.is_some();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .p_2()
            .child(div().text_color(muted).child("COLLECTION RUNNER"))
            .child(
                Button::new("run-back")
                    .label("← Back to requests")
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.main_view = MainView::Request;
                        cx.notify();
                    })),
            );
        let target = match &r.target {
            Some((name, _)) => format!("Runs every request in \"{name}\", in sidebar order."),
            None => "Click a collection or folder in the sidebar, then Run….".to_string(),
        };
        let options = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_2()
            .child(div().text_color(muted).child("Iterations"))
            .child(div().w(px(60.)).child(Input::new(&r.iterations)))
            .child(div().text_color(muted).child("Delay (ms)"))
            .child(div().w(px(80.)).child(Input::new(&r.delay)))
            .child(
                seg_btn_ix(
                    "run-stop-fail",
                    0,
                    if r.stop_on_fail {
                        "Stop on first failure: on"
                    } else {
                        "Stop on first failure: off"
                    },
                    r.stop_on_fail,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.runner.stop_on_fail = !this.runner.stop_on_fail;
                    cx.notify();
                })),
            )
            .child(
                Button::new("run-go")
                    .primary()
                    .label(if running { "Stop" } else { "Run" })
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        if let Some(t) = this.runner.cancel.clone() {
                            t.cancel();
                        } else {
                            this.start_run(cx);
                        }
                    })),
            )
            .children((!r.rows.is_empty()).then(|| {
                Button::new("run-copy")
                    .label(if r.copied {
                        "Copied ✓"
                    } else {
                        "Copy summary"
                    })
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            this.runner.summary_text(),
                        ));
                        this.runner.copied = true;
                        cx.notify();
                    }))
            }));

        let (n, failed, pass, fail) = r.counts();
        let time = match (r.elapsed, r.started) {
            (Some(e), _) => format!("{:.1} s", e.as_secs_f32()),
            (None, Some(s)) if running => format!("{:.1} s…", s.elapsed().as_secs_f32()),
            _ => String::new(),
        };
        let progress = if r.rows.is_empty() && !running {
            String::new()
        } else {
            format!(
                "{n} / {} requests · {failed} failed · tests {pass} passed, {fail} failed · {time}{}",
                r.planned,
                if running {
                    ""
                } else if n < r.planned {
                    " · stopped"
                } else {
                    " · done"
                }
            )
        };

        let multi = r.rows.iter().any(|x| x.iteration > 0);
        let mut list = div()
            .id("run-rows")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px_2();
        for (ix, row) in r.rows.iter().enumerate() {
            let (status, colour) = match &row.result {
                RunResult::Response {
                    code,
                    text,
                    ms,
                    size,
                } => (
                    format!("{code} {text} · {ms} ms · {}", human(*size)),
                    status_color(*code),
                ),
                RunResult::Failed(e) => (e.clone(), red),
            };
            let counted: Vec<_> = row.tests.iter().filter(|t| !t.skipped).collect();
            let passed = counted.iter().filter(|t| t.passed).count();
            let tests = if counted.is_empty() {
                String::new()
            } else {
                format!("tests {passed}/{}", counted.len())
            };
            let path = row.path.clone();
            let mut item = div()
                .id(("run-row", ix))
                .py_1()
                .border_b_1()
                .border_color(border)
                .cursor_pointer()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .w(px(16.))
                                .text_color(if row.failed() { red } else { green })
                                .child(if row.failed() { "✗" } else { "✓" }),
                        )
                        .children(multi.then(|| {
                            div()
                                .text_color(muted)
                                .child(format!("#{}", row.iteration + 1))
                        }))
                        .child(div().w(px(64.)).child(row.method.as_str()))
                        .child(div().flex_1().truncate().child(row.name.clone()))
                        .child(div().text_color(colour).child(status))
                        .child(
                            div()
                                .w(px(80.))
                                .text_color(if passed == counted.len() { muted } else { red })
                                .child(tests),
                        ),
                );
            for t in row.tests.iter().filter(|t| !t.passed) {
                item = item.child(div().pl(px(24.)).text_color(red).child(format!(
                    "✗ {}: {}",
                    t.name,
                    t.error.as_deref().unwrap_or("")
                )));
            }
            for e in &row.errors {
                item = item.child(div().pl(px(24.)).text_color(red).child(e.clone()));
            }
            if let Some(p) = path {
                item = item.on_click(cx.listener(move |this, _ev, window, cx| {
                    this.main_view = MainView::Request;
                    this.open_request(p.clone(), window, cx);
                    cx.notify();
                }));
            }
            list = list.child(item);
        }
        if r.rows.is_empty() && !running {
            list = list.child(div().p_2().text_color(muted).child(
                "Variables set by scripts carry over from one request to the next and are saved to the active environment. Runs use the saved files (unsaved tab edits aren't included) and don't fill History.",
            ));
        }

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(header)
            .child(div().px_2().child(target))
            .child(options)
            .children(
                r.notes
                    .iter()
                    .map(|n| div().px_2().text_color(c(0xf9e2af)).child(n.clone())),
            )
            .child(div().px_2().text_color(muted).child(progress))
            .child(list)
    }
}
