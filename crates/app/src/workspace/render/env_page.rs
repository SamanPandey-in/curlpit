//! The environments and variables page: scope picker, the variable table with its
//! secret/reveal controls, and the import/export footer.

use crate::sidebar::Scope;
use crate::theme::Palette;
use crate::ui::{seg_btn, seg_btn_ix, Status};
use crate::workspace::Workspace;
use at_core::EnvVar;
use gpui::prelude::*;
use gpui::{div, px, Context, Hsla};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;

impl Workspace {
    pub(crate) fn render_env_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let (muted, border) = (p.text_muted(), p.border());

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .p_2()
            .child(div().text_color(muted).child("ENVIRONMENTS & VARIABLES"))
            .child(
                Button::new("env-back")
                    .label("← Back to requests")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.close_envs(cx))),
            );

        // scope picker: Globals, each environment (● marks the active one), + New
        let mut scopes = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .pb_2();
        let g = seg_btn(
            "env-scope-g",
            "Globals".to_string(),
            self.env_scope == Scope::Globals,
            cx,
        );
        scopes = scopes.child(g.on_click(
            cx.listener(|this, _ev, window, cx| this.select_scope(Scope::Globals, window, cx)),
        ));
        for (ix, (e, _)) in self.envs.iter().enumerate() {
            let label = if self.active_env == Some(ix) {
                format!("● {}", e.name)
            } else {
                e.name.clone()
            };
            let b = seg_btn_ix(
                "env-scope",
                ix,
                &label,
                self.env_scope == Scope::Env(ix),
                cx,
            );
            scopes = scopes.child(b.on_click(cx.listener(move |this, _ev, window, cx| {
                this.select_scope(Scope::Env(ix), window, cx)
            })));
        }
        scopes = scopes.child(
            Button::new("env-new")
                .label("+ New")
                .on_click(cx.listener(|this, _ev, window, cx| this.on_env_new(window, cx))),
        );

        let detail_head = match self.env_scope {
            Scope::Globals => div()
                .px_2()
                .pb_2()
                .text_color(muted)
                .child("Globals apply everywhere. The active environment overrides them."),
            Scope::Env(ix) => {
                let is_active = self.active_env == Some(ix);
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .pb_2()
                    .child(
                        div()
                            .w(px(240.))
                            .flex_none()
                            .child(Input::new(&self.env_name)),
                    )
                    .child(
                        Button::new("env-active")
                            .label(if is_active {
                                "Active ✓ (click to deactivate)"
                            } else {
                                "Make active"
                            })
                            .on_click(cx.listener(move |this, _ev, _window, cx| {
                                this.active_env = if this.active_env == Some(ix) {
                                    None
                                } else {
                                    Some(ix)
                                };
                                this.persist_active();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("env-del")
                            .label(if self.confirm_delete {
                                "Click again to delete"
                            } else {
                                "Delete"
                            })
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                if this.confirm_delete {
                                    this.confirm_delete = false;
                                    this.on_env_delete(window, cx);
                                } else {
                                    this.confirm_delete = true;
                                    cx.notify();
                                }
                            })),
                    )
            }
        };

        let mut table = div()
            .id("env-table")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .text_color(muted)
                    .child(div().w(px(24.)).flex_none())
                    .child(div().flex_1().child("Variable"))
                    .child(div().flex_1().child("Value"))
                    .child(div().w(px(150.)).flex_none())
                    .child(div().w(px(36.)).flex_none()),
            );
        for (ix, row) in self.env_rows.iter().enumerate() {
            let sec = seg_btn_ix("env-sec", ix, "Secret", row.secret, cx);
            let reveal = row.secret.then(|| {
                Button::new(("env-show", ix))
                    .label(if row.revealed { "Hide" } else { "Show" })
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        if let Some(r) = this.env_rows.get_mut(ix) {
                            r.revealed = !r.revealed;
                            let masked = r.secret && !r.revealed;
                            r.value.update(cx, |s, cx| s.set_masked(masked, window, cx));
                        }
                        cx.notify();
                    }))
            });
            table = table.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div().w(px(24.)).flex_none().child(
                            Checkbox::new(("env-en", ix)).checked(row.enabled).on_click(
                                cx.listener(move |this, checked: &bool, _window, cx| {
                                    if let Some(r) = this.env_rows.get_mut(ix) {
                                        r.enabled = *checked;
                                    }
                                    cx.notify();
                                }),
                            ),
                        ),
                    )
                    .child(div().flex_1().child(Input::new(&row.key)))
                    .child(div().flex_1().child(Input::new(&row.value)))
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .child(sec.on_click(cx.listener(move |this, _ev, window, cx| {
                                if let Some(r) = this.env_rows.get_mut(ix) {
                                    r.secret = !r.secret;
                                    r.revealed = false;
                                    let masked = r.secret;
                                    r.value.update(cx, |s, cx| s.set_masked(masked, window, cx));
                                }
                                cx.notify();
                            })))
                            .children(reveal),
                    )
                    .child(div().w(px(36.)).flex_none().child(
                        Button::new(("env-rm", ix)).label("✕").on_click(cx.listener(
                            move |this, _ev, _window, cx| {
                                if ix < this.env_rows.len() {
                                    this.env_rows.remove(ix);
                                }
                                cx.notify();
                            },
                        )),
                    )),
            );
        }

        let footer = div()
            .flex()
            .flex_row()
            .gap_2()
            .p_2()
            .child(
                Button::new("env-add")
                    .label("+ Add variable")
                    .on_click(cx.listener(|this, _ev, window, cx| {
                        let row = Self::new_env_row(window, cx, &EnvVar::new("", ""));
                        this.env_rows.push(row);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("env-save")
                    .primary()
                    .label("Save")
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.save_env_scope(cx);
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new("env-import")
                    .label("Import…")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_import(window, cx))),
            )
            .child(
                seg_btn(
                    "env-secrets",
                    if self.export_secrets {
                        "Include secrets: on".to_string()
                    } else {
                        "Include secrets: off".to_string()
                    },
                    self.export_secrets,
                    cx,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.export_secrets = !this.export_secrets;
                    cx.notify();
                })),
            )
            .child(
                Button::new("env-export")
                    .label("Export…")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_export_env(window, cx))),
            );

        let (msg, msg_col): (String, Hsla) = match &self.status {
            Status::Info(m) => (m.clone(), muted),
            Status::Error(e) => (e.clone(), p.status_error()),
            _ => (String::new(), muted),
        };

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(scopes)
            .child(detail_head)
            .child(table)
            .child(footer)
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(border)
                    .text_color(msg_col)
                    .child(if msg.is_empty() {
                        "Use {{name}} in the URL, params, headers, body and auth. Secret values are masked here but stored as plain text on disk for now.".to_string()
                    } else {
                        msg
                    }),
            )
    }
}
