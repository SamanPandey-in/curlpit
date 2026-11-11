//! The Scripts editor tab and the Tests result pane: the two script editors, and the
//! pass/fail list plus console output of the last send.

use crate::request_tab::RequestTab;
use crate::theme::Palette;
use crate::ui::{mono, seg_btn_ix};
use crate::workspace::Workspace;
use at_script::LogLevel;
use gpui::prelude::*;
use gpui::{div, px, App, Context, Entity};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};
use gpui_component::ActiveTheme as _;

impl Workspace {
    pub(crate) fn has_scripts(tab: &RequestTab, cx: &App) -> bool {
        !tab.pre_script.read(cx).value().trim().is_empty()
            || !tab.test_script.read(cx).value().trim().is_empty()
    }

    pub(crate) fn render_scripts(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let filled = |e: &Entity<InputState>| !e.read(cx).value().trim().is_empty();
        let label = |name: &str, e: &Entity<InputState>| {
            if filled(e) {
                format!("{name} ●")
            } else {
                name.to_string()
            }
        };
        let show_tests = tab.scripts_show_tests;
        let snippets: &[(&str, &str)] = if show_tests {
            &[
                (
                    "Status is 200",
                    "pm.test(\"Status is 200\", () => pm.response.to.have.status(200));\n",
                ),
                (
                    "Fast response",
                    "pm.test(\"Response is fast\", () => pm.expect(pm.response.responseTime).to.be.below(500));\n",
                ),
                (
                    "JSON field",
                    "pm.test(\"Has an id\", () => pm.expect(pm.response.json()).to.have.property(\"id\"));\n",
                ),
                (
                    "Save to env",
                    "const json = pm.response.json();\npm.environment.set(\"id\", json.id);\n",
                ),
            ]
        } else {
            &[
                (
                    "Set variable",
                    "pm.variables.set(\"ts\", String(Date.now()));\n",
                ),
                (
                    "Add header",
                    "pm.request.headers.upsert({ key: \"X-Request-Id\", value: String(Date.now()) });\n",
                ),
                ("Log", "console.log(pm.request.url);\n"),
            ]
        };
        let mut bar = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(
                seg_btn_ix(
                    "script-kind",
                    0,
                    &label("Pre-request", &tab.pre_script),
                    !show_tests,
                    cx,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().scripts_show_tests = false;
                    cx.notify();
                })),
            )
            .child(
                seg_btn_ix(
                    "script-kind",
                    1,
                    &label("Tests", &tab.test_script),
                    show_tests,
                    cx,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().scripts_show_tests = true;
                    cx.notify();
                })),
            )
            .child(div().w(px(12.)))
            .child(div().text_color(muted).child("Insert:"));
        for (i, (name, code)) in snippets.iter().enumerate() {
            let code = code.to_string();
            bar = bar.child(
                Button::new(("snippet", i))
                    .label(name.to_string())
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        let tab = this.tab_mut();
                        let editor = if tab.scripts_show_tests {
                            tab.test_script.clone()
                        } else {
                            tab.pre_script.clone()
                        };
                        let cur = editor.read(cx).value().to_string();
                        let text = if cur.is_empty() || cur.ends_with('\n') {
                            format!("{cur}{code}")
                        } else {
                            format!("{cur}\n{code}")
                        };
                        editor.update(cx, |s, cx| s.set_value(text, window, cx));
                        cx.notify();
                    })),
            );
        }
        let editor = if show_tests {
            &tab.test_script
        } else {
            &tab.pre_script
        };
        div()
            .p_2()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h(px(120.))
                    .child(Input::new(editor).h_full()),
            )
            .child(div().text_color(muted).child(if show_tests {
                "Runs after the response arrives: pm.response, pm.test(name, fn), pm.expect(value)…"
            } else {
                "Runs before sending: pm.environment / pm.globals / pm.variables .get/.set, pm.request.headers…"
            }))
    }

    pub(crate) fn render_tests(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let muted = p.text_muted();
        let (green, red, amber) = (p.success_text(), p.status_error(), p.warning_text());
        let mut col = div()
            .id("tests-view")
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        let Some(rep) = &tab.report else {
            return col.child(div().text_color(muted).child(
                "No scripts ran. Write tests on the request's Scripts tab, e.g. pm.test(\"ok\", () => pm.response.to.be.ok), then Send.",
            ));
        };
        for (what, out) in [("Pre-request script", &rep.pre), ("Test script", &rep.test)] {
            if let Some(o) = out {
                if let Some(e) = &o.error {
                    col = col.child(div().text_color(red).child(format!("{what}: {e}")));
                }
            }
        }
        for n in &rep.notes {
            col = col.child(div().text_color(amber).child(n.clone()));
        }
        if let Some(t) = &rep.test {
            for r in &t.tests {
                let (mark, colour) = if r.skipped {
                    ("○", muted)
                } else if r.passed {
                    ("✓", green)
                } else {
                    ("✗", red)
                };
                col = col.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(div().w(px(16.)).text_color(colour).child(mark))
                        .child(r.name.clone()),
                );
                if let Some(e) = &r.error {
                    if !r.passed {
                        col = col.child(div().pl(px(24.)).text_color(muted).child(e.clone()));
                    }
                }
            }
        }
        let logs: Vec<_> = [&rep.pre, &rep.test]
            .into_iter()
            .flatten()
            .flat_map(|o| o.logs.iter())
            .collect();
        if !logs.is_empty() {
            col = col.child(div().pt_2().text_color(muted).child("CONSOLE"));
            for l in logs {
                let colour = match l.level {
                    LogLevel::Warn => amber,
                    LogLevel::Error => red,
                    LogLevel::Log | LogLevel::Info => p.text(),
                };
                col = col.child(
                    div()
                        .font_family(mono())
                        .text_color(colour)
                        .child(l.text.clone()),
                );
            }
        }
        let timing: Vec<String> = [("pre-request", &rep.pre), ("tests", &rep.test)]
            .into_iter()
            .filter_map(|(n, o)| {
                o.as_ref()
                    .map(|o| format!("{n} {} ms", o.elapsed.as_millis()))
            })
            .collect();
        col.child(div().pt_2().text_color(muted).child(timing.join(" · ")))
    }
}
