//! `Render for Workspace`: the window itself — tab bar, request editor, status row,
//! response viewer and sidebar — assembled from the panel builders in the sibling
//! modules. Everything that depends on the current window geometry is computed here,
//! because only `render` is handed the `Window`.

mod auth;
mod body;
mod code;
mod cookies;
mod env_page;
mod kv;
mod runner;
mod scripts;
mod settings;
mod tree;

use crate::actions::{CloseTab, FindInResponse, NewTab, ReopenTab, SaveRequest, SendRequest};
use crate::editor::{next_method, AuthKind, ReqTab, Which};
use crate::format::{body_label, human};
use crate::response::{row_spans, ResTab, Span, FIND_MAX};
use crate::sidebar::{ItemKind, MainView, SideView};
use crate::ui::{avail_width, c, char_w, mono, seg_btn, status_color, Status, GUTTER};
use crate::workspace::Workspace;
use at_core::lines::{build_rows, CONT};
use at_history::{ago, now_ms};
use gpui::prelude::*;
use gpui::{div, px, uniform_list, ClipboardItem, Context, Hsla, ScrollWheelEvent, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::ActiveTheme as _;
use std::sync::Arc;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            window.focus(&self.focus);
            self.focused_once = true;
        }
        self.ensure_tree();
        // ---- viewer geometry (depends on the current window width) ----
        let avail = avail_width(window);
        let cols = (((avail / char_w()).floor() as usize).saturating_sub(1)).max(10);
        {
            let tab = self.tab_mut();
            if tab.wrap {
                if let Some(v) = tab.view.as_mut() {
                    let stale = match &v.wrapped {
                        Some((c, _)) => *c != cols,
                        None => true,
                    };
                    if stale {
                        let rows = build_rows(&v.data, cols);
                        v.wrapped = Some((cols, Arc::new(rows)));
                    }
                }
            }
            if tab.find_open {
                tab.refresh_matches(cx);
                if tab.scroll_to_match {
                    tab.reveal_match(avail);
                }
            }
            let max_x = tab.max_scroll_x(avail);
            if tab.scroll_x > max_x {
                tab.scroll_x = max_x;
            }
        }
        let tab = self.tab();
        let overflow = tab
            .view
            .as_ref()
            .map(|v| v.longest_chars as f32 * char_w() > avail)
            .unwrap_or(false);
        let scroll_x = if tab.wrap { 0.0 } else { tab.scroll_x };

        let theme = cx.theme();
        let (bg, fg, muted, border, accent) = (
            theme.background,
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.accent,
        );
        let in_flight = tab.cancel.is_some();

        // ---- tab bar: one button per open request, ● marks unsaved changes ----
        let mut tab_bar = div()
            .id("tab-bar")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .pt_2()
            .overflow_x_scroll();
        for (ix, t) in self.tabs.iter().enumerate() {
            let label = format!(
                "{}{}{}",
                if t.cancel.is_some() { "… " } else { "" },
                if t.is_dirty(cx) { "● " } else { "" },
                t.title(cx)
            );
            let b = Button::new(("tab", ix)).label(label);
            let b = if ix == self.active { b.primary() } else { b };
            tab_bar = tab_bar.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .child(b.on_click(
                        cx.listener(move |this, _ev, window, cx| this.activate_tab(ix, window, cx)),
                    ))
                    .child(
                        Button::new(("tab-x", ix))
                            .label(if t.confirm_close { "Discard?" } else { "✕" })
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.close_tab(ix, window, cx)
                            })),
                    ),
            );
        }
        tab_bar = tab_bar.child(
            div().flex_none().child(
                Button::new("tab-new")
                    .label("+")
                    .on_click(cx.listener(|this, _ev, window, cx| this.add_tab(window, cx))),
            ),
        );
        tab_bar = tab_bar.child(
            div().flex_none().child(
                Button::new("tab-curl")
                    .label("+ cURL")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_paste_curl(window, cx))),
            ),
        );

        let top = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .p_2()
            .child(div().w(px(180.)).flex_none().child(Input::new(&tab.name)))
            .child(
                Button::new("method")
                    .label(tab.method.as_str())
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        let tab = this.tab_mut();
                        tab.method = next_method(tab.method);
                        cx.notify();
                    })),
            )
            .child(div().flex_1().child(Input::new(&tab.url)))
            .child(
                Button::new("send")
                    .primary()
                    .label(if in_flight { "Cancel" } else { "Send" })
                    .on_click(cx.listener(|this, _ev, _window, cx| this.send_active(cx))),
            )
            .child(
                Button::new("save")
                    .label("Save")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_save(window, cx))),
            )
            .child(
                Button::new("copy-curl")
                    .label("Copy cURL")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.on_copy_curl(cx))),
            )
            .child(
                Button::new("env-pick")
                    .label(format!(
                        "Env: {}",
                        self.active_env
                            .and_then(|i| self.envs.get(i))
                            .map(|(e, _)| e.name.as_str())
                            .unwrap_or("none")
                    ))
                    .on_click(cx.listener(|this, _ev, _window, cx| this.cycle_env(cx))),
            )
            .child(
                Button::new("env-manage")
                    .label("Variables…")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.open_envs(cx))),
            )
            .child(
                Button::new("cookies-open")
                    .label("Cookies")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.open_cookies(cx))),
            );

        // ---- request editor: tabs + panel ----
        let pcount = tab
            .params
            .iter()
            .filter(|r| r.enabled && !r.key.read(cx).value().is_empty())
            .count();
        let hcount = tab
            .headers
            .iter()
            .filter(|r| r.enabled && !r.key.read(cx).value().is_empty())
            .count();
        let req_tabs = div()
            .flex()
            .flex_row()
            .gap_1()
            .px_2()
            .child(
                seg_btn(
                    "t-params",
                    format!("Params ({pcount})"),
                    tab.req_tab == ReqTab::Params,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Params;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-headers",
                    format!("Headers ({hcount})"),
                    tab.req_tab == ReqTab::Headers,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Headers;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-body",
                    body_label(tab.body_mode, tab.raw_kind),
                    tab.req_tab == ReqTab::Body,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Body;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-auth",
                    match tab.auth_kind {
                        AuthKind::None => "Auth".to_string(),
                        k => format!("Auth ({})", k.label()),
                    },
                    tab.req_tab == ReqTab::Auth,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Auth;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-scripts",
                    if Self::has_scripts(tab, cx) {
                        "Scripts ●".to_string()
                    } else {
                        "Scripts".to_string()
                    },
                    tab.req_tab == ReqTab::Scripts,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Scripts;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-settings",
                    "Settings".to_string(),
                    tab.req_tab == ReqTab::Settings,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Settings;
                    cx.notify();
                })),
            );
        let req_panel =
            div()
                .id("req-panel")
                .h(px(210.))
                .overflow_y_scroll()
                .child(match tab.req_tab {
                    ReqTab::Params => self.render_kv(tab, Which::Params, cx).into_any_element(),
                    ReqTab::Headers => self.render_kv(tab, Which::Headers, cx).into_any_element(),
                    ReqTab::Body => self.render_body(tab, cx).into_any_element(),
                    ReqTab::Auth => self.render_auth(tab, cx).into_any_element(),
                    ReqTab::Scripts => self.render_scripts(tab, cx).into_any_element(),
                    ReqTab::Settings => self.render_settings(tab, cx).into_any_element(),
                });

        let (status_text, status_col): (String, Hsla) = match &tab.status {
            Status::Idle => ("Ready".into(), muted),
            Status::Loading => ("Sending…".into(), muted),
            Status::Info(m) => (m.clone(), muted),
            Status::Done {
                code,
                text,
                ms,
                size,
            } => (
                format!("{code} {text}   ·   {ms} ms   ·   {}", human(*size)),
                status_color(*code),
            ),
            Status::Error(e) => (e.clone(), c(0xf38ba8)),
        };

        // The wrap toggle appears only when text overflows the window (or while wrap is on).
        let wrap_btn = (tab.res_tab == ResTab::Body && (tab.wrap || overflow)).then(|| {
            Button::new("wrap")
                .label(if tab.wrap { "Wrap: on" } else { "Wrap: off" })
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    let tab = this.tab_mut();
                    tab.wrap = !tab.wrap;
                    tab.scroll_x = 0.0;
                    if !tab.wrap {
                        if let Some(v) = tab.view.as_mut() {
                            v.wrapped = None;
                        }
                    }
                    cx.notify();
                }))
        });

        // Copy the (pretty-printed) response body to the clipboard (FR-RES-01).
        let copy_btn = (tab.res_tab == ResTab::Body && tab.view.is_some()).then(|| {
            Button::new("copy")
                .label(if tab.copied { "Copied ✓" } else { "Copy" })
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    let tab = this.tab_mut();
                    if let Some(v) = &tab.view {
                        let text = String::from_utf8_lossy(&v.data).into_owned();
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        tab.copied = true;
                        cx.notify();
                    }
                }))
        });

        let status_row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_3()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .child(div().text_color(status_col).child(status_text))
                    .children(
                        tab.warning
                            .clone()
                            .map(|w| div().text_color(c(0xf9e2af)).child(w)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(
                        seg_btn("r-body", "Body".to_string(), tab.res_tab == ResTab::Body)
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.tab_mut().res_tab = ResTab::Body;
                                cx.notify();
                            })),
                    )
                    .child(
                        seg_btn("r-tree", "Tree".to_string(), tab.res_tab == ResTab::Tree)
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.tab_mut().res_tab = ResTab::Tree;
                                cx.notify();
                            })),
                    )
                    .child(
                        seg_btn(
                            "r-headers",
                            format!("Headers ({})", tab.resp_headers.len()),
                            tab.res_tab == ResTab::Headers,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            this.tab_mut().res_tab = ResTab::Headers;
                            cx.notify();
                        })),
                    )
                    .child(
                        seg_btn(
                            "r-tests",
                            tab.report
                                .as_ref()
                                .map(|r| r.label())
                                .unwrap_or_else(|| "Tests".to_string()),
                            tab.res_tab == ResTab::Tests,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            this.tab_mut().res_tab = ResTab::Tests;
                            cx.notify();
                        })),
                    )
                    .child(
                        seg_btn("r-code", "Code".to_string(), tab.res_tab == ResTab::Code)
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.tab_mut().res_tab = ResTab::Code;
                                this.code_copied = false;
                                cx.notify();
                            })),
                    )
                    .children(copy_btn)
                    .children(
                        (tab.res_tab == ResTab::Body && tab.view.is_some()).then(|| {
                            Button::new("save-body").label("Save…").on_click(
                                cx.listener(|this, _ev, window, cx| this.on_save_body(window, cx)),
                            )
                        }),
                    )
                    .children(
                        (tab.res_tab == ResTab::Body && tab.view.is_some()).then(|| {
                            Button::new("find-open").label("Find").on_click(
                                cx.listener(|this, _ev, window, cx| this.open_find(window, cx)),
                            )
                        }),
                    )
                    .children(wrap_btn),
            );

        let viewer = match &tab.view {
            Some(v) => {
                let rows = v.rows(tab.wrap);
                let data = v.data.clone();
                let n = rows.len();
                // the row holding the current Find match is highlighted
                let hl_row = if tab.find_open {
                    tab.matches.get(tab.match_ix).map(|&off| rows.row_of(off))
                } else {
                    None
                };
                // Every match on screen is highlighted inside its line (FR-RES-05).
                let (hits, hit_len, cur_hit) = if tab.find_open {
                    (
                        tab.matches.clone(),
                        tab.find.read(cx).value().len(),
                        tab.matches.get(tab.match_ix).copied(),
                    )
                } else {
                    (Vec::new(), 0, None)
                };
                uniform_list("response", n, move |range, _window, _cx| {
                    let mut items = Vec::new();
                    for ix in range {
                        // trailing whitespace is not drawn, so spans stop at `keep`
                        let full = rows.row(&data, ix);
                        let mut keep = full.len();
                        while keep > 0 && full[keep - 1].is_ascii_whitespace() {
                            keep -= 1;
                        }
                        let spans = row_spans(
                            &full[..keep],
                            rows.starts[ix] as usize,
                            &hits,
                            hit_len,
                            cur_hit,
                        );
                        // Continuation rows of a wrapped line get an empty gutter, so numbers
                        // stay attached to the first row of their line.
                        let label = if rows.lines[ix] & CONT != 0 {
                            String::new()
                        } else {
                            rows.line_no(ix).to_string()
                        };
                        items.push(
                            div()
                                .flex()
                                .flex_row()
                                .h(px(18.))
                                .when(hl_row == Some(ix), |d| d.bg(accent))
                                // sticky gutter: lives outside the horizontally shifted text
                                .child(
                                    div()
                                        .w(px(GUTTER))
                                        .flex_none()
                                        .px_2()
                                        .text_color(muted)
                                        .border_r_1()
                                        .border_color(border)
                                        .child(label),
                                )
                                .child(
                                    div().flex_1().overflow_hidden().child(
                                        div()
                                            .relative()
                                            .left(px(-scroll_x))
                                            .flex_none()
                                            .flex()
                                            .flex_row()
                                            .whitespace_nowrap()
                                            .children(spans.into_iter().map(|(s, kind)| {
                                                let d = div().whitespace_nowrap().child(s);
                                                // fixed colours, so a match stays readable in
                                                // both the light and the dark theme
                                                match kind {
                                                    Span::Current => {
                                                        d.bg(c(0xfab387)).text_color(c(0x11111b))
                                                    }
                                                    Span::Hit => {
                                                        d.bg(c(0xf9e2af)).text_color(c(0x11111b))
                                                    }
                                                    Span::Plain => d,
                                                }
                                            })),
                                    ),
                                ),
                        );
                    }
                    items
                })
                .track_scroll(tab.list_scroll.clone())
                .h_full()
                .into_any_element()
            }
            None => div()
                .p_4()
                .text_color(muted)
                .child("Send a request to see the response")
                .into_any_element(),
        };

        let find_bar =
            (tab.res_tab == ResTab::Body && tab.find_open).then(|| {
                let count = if tab.find.read(cx).value().is_empty() {
                    String::new()
                } else if tab.matches.is_empty() {
                    "No matches".to_string()
                } else if tab.matches.len() >= FIND_MAX {
                    format!("{} / {}+", tab.match_ix + 1, FIND_MAX)
                } else {
                    format!("{} / {}", tab.match_ix + 1, tab.matches.len())
                };
                let id = tab.id;
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(div().w(px(260.)).child(Input::new(&tab.find)))
                    .child(div().w(px(90.)).text_color(muted).child(count))
                    .child(Button::new("find-prev").label("↑").on_click(
                        cx.listener(move |this, _ev, _window, cx| this.find_step(id, -1, cx)),
                    ))
                    .child(Button::new("find-next").label("↓").on_click(
                        cx.listener(move |this, _ev, _window, cx| this.find_step(id, 1, cx)),
                    ))
                    .child(Button::new("find-close").label("✕").on_click(cx.listener(
                        |this, _ev, window, cx| {
                            // closing is also "clear" (FR-RES-05): no stale query or matches
                            let find = this.tab().find.clone();
                            find.update(cx, |s, cx| s.set_value(String::new(), window, cx));
                            let tab = this.tab_mut();
                            tab.find_open = false;
                            tab.matches.clear();
                            tab.match_ix = 0;
                            this.refocus(window);
                            cx.notify();
                        },
                    )))
            });

        let resp_area = match tab.res_tab {
            ResTab::Body => div()
                .size_full()
                .flex()
                .flex_col()
                .children(find_bar)
                .child(div().flex_1().overflow_hidden().child(viewer))
                .into_any_element(),
            ResTab::Headers => div()
                .id("resp-headers")
                .size_full()
                .overflow_y_scroll()
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .children(tab.resp_headers.iter().map(|(k, v)| {
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(
                            div()
                                .w(px(240.))
                                .flex_none()
                                .text_color(muted)
                                .child(k.clone()),
                        )
                        .child(div().flex_1().child(v.clone()))
                }))
                .into_any_element(),
            ResTab::Tree => self.render_tree(cx),
            ResTab::Tests => self.render_tests(tab, cx).into_any_element(),
            ResTab::Code => self.render_code(cx).into_any_element(),
        };

        let mut side_list = div()
            .id("sidebar-list")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if self.items.is_empty() {
            side_list = side_list.child(
                div()
                    .p_2()
                    .text_color(muted)
                    .child("No collections yet. Press + or just Save a request."),
            );
        }
        for (ix, item) in self.items.iter().enumerate() {
            let selected = match item.kind {
                ItemKind::Request => tab.current_path.as_ref() == Some(&item.path),
                ItemKind::Collection | ItemKind::Folder => {
                    tab.current_path.is_none() && tab.save_dir.as_ref() == Some(&item.path)
                }
                ItemKind::Broken(_) => false,
            };
            let (prefix, color) = match item.kind {
                ItemKind::Collection => ("▾ ", fg),
                ItemKind::Folder => ("▸ ", fg),
                ItemKind::Request => ("", fg),
                ItemKind::Broken(_) => ("⚠ ", c(0xf38ba8)),
            };
            side_list = side_list.child(
                div()
                    .id(("side", ix))
                    .w_full()
                    .py_1()
                    .pl(px(8.0 + item.depth as f32 * 14.0))
                    .pr_2()
                    .text_color(color)
                    .cursor_pointer()
                    .bg(if selected { accent } else { bg })
                    .child(format!("{prefix}{}", item.label))
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.on_sidebar_click(ix, window, cx)
                    })),
            );
        }
        let side_head = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .p_2()
            .child(
                seg_btn(
                    "side-col",
                    "Collections".to_string(),
                    self.side_view == SideView::Collections,
                )
                .on_click(
                    cx.listener(|this, _ev, _window, cx| this.show_side(SideView::Collections, cx)),
                ),
            )
            .child(
                seg_btn(
                    "side-hist",
                    "History".to_string(),
                    self.side_view == SideView::History,
                )
                .on_click(
                    cx.listener(|this, _ev, _window, cx| this.show_side(SideView::History, cx)),
                ),
            )
            .child(div().flex_1())
            .children((self.side_view == SideView::Collections).then(|| {
                Button::new("new-col")
                    .label("+")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.on_new_collection(cx)))
            }));

        let mut sidebar = div()
            .w(px(260.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(border)
            .child(side_head);
        sidebar = match self.side_view {
            SideView::Collections => sidebar.child(side_list).child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .p_2()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        Button::new("col-import").label("Import…").on_click(
                            cx.listener(|this, _ev, window, cx| this.on_import(window, cx)),
                        ),
                    )
                    .child(Button::new("col-export").label("Export…").on_click(
                        cx.listener(|this, _ev, window, cx| this.on_export_collection(window, cx)),
                    ))
                    .child(
                        Button::new("col-run")
                            .label(if self.runner.cancel.is_some() {
                                "Run… ●"
                            } else {
                                "Run…"
                            })
                            .on_click(cx.listener(|this, _ev, _window, cx| this.open_runner(cx))),
                    ),
            ),
            SideView::History => {
                let now = now_ms();
                let mut list = div()
                    .id("hist-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col();
                if self.history.is_some() && self.hist_items.is_empty() {
                    let empty = if self.hist_search.read(cx).value().trim().is_empty() {
                        "No requests yet. Everything you send appears here."
                    } else {
                        "No matches."
                    };
                    list = list.child(div().p_2().text_color(muted).child(empty));
                }
                for (ix, s) in self.hist_items.iter().enumerate() {
                    let (code, code_col) = match s.status {
                        Some(n) => (n.to_string(), status_color(n)),
                        None => ("ERR".to_string(), c(0xf38ba8)),
                    };
                    let id = s.id;
                    list = list.child(
                        div()
                            .id(("hist", ix))
                            .w_full()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .flex()
                            .flex_col()
                            .border_b_1()
                            .border_color(border)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(s.method.clone())
                                    .child(div().text_color(code_col).child(code))
                                    .child(div().flex_1())
                                    .child(div().text_color(muted).child(ago(now, s.ts_ms))),
                            )
                            .child(div().w_full().text_color(muted).truncate().child(
                                if s.url.is_empty() {
                                    "(no URL)".to_string()
                                } else {
                                    s.url.clone()
                                },
                            ))
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.open_history(id, window, cx)
                            })),
                    );
                }
                let tools = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .pb_2()
                    .child(div().flex_1().child(Input::new(&self.hist_search)))
                    .child(
                        Button::new("hist-clear")
                            .label(if self.confirm_clear {
                                "Really clear?"
                            } else {
                                "Clear"
                            })
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                if this.confirm_clear {
                                    this.clear_history(cx);
                                } else {
                                    this.confirm_clear = true;
                                    cx.notify();
                                }
                            })),
                    );
                sidebar
                    .children(self.history.is_some().then_some(tools))
                    .children(
                        self.hist_error
                            .clone()
                            .map(|e| div().px_2().pb_2().text_color(c(0xf38ba8)).child(e)),
                    )
                    .child(list)
            }
        };

        let main_col = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(tab_bar)
            .child(top)
            .child(req_tabs)
            .child(req_panel)
            .child(status_row)
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .font_family(mono())
                    .text_size(px(13.))
                    // horizontal scrolling is ours (so the gutter can stay put); vertical is the list's
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, window, cx| {
                        let tab = this.tab_mut();
                        if tab.wrap || tab.res_tab != ResTab::Body {
                            return;
                        }
                        let d = ev.delta.pixel_delta(px(18.));
                        let mut dx = f32::from(d.x);
                        if dx == 0.0 && ev.modifiers.shift {
                            dx = f32::from(d.y);
                        }
                        if dx == 0.0 {
                            return;
                        }
                        let max = tab.max_scroll_x(avail_width(window));
                        tab.scroll_x = (tab.scroll_x - dx).clamp(0.0, max);
                        cx.notify();
                    }))
                    .child(resp_area),
            );

        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(bg)
            .text_color(fg)
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SendRequest, _window, cx| this.send_active(cx)))
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                if this.main_view == MainView::Request {
                    this.add_tab(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                if this.main_view == MainView::Request {
                    let ix = this.active;
                    this.close_tab(ix, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &FindInResponse, window, cx| {
                if this.main_view == MainView::Request && this.tab().view.is_some() {
                    this.open_find(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ReopenTab, window, cx| {
                if this.main_view == MainView::Request {
                    this.reopen_tab(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SaveRequest, window, cx| this.on_save(window, cx)))
            .child(sidebar)
            .child(match self.main_view {
                MainView::Environments => self.render_env_page(cx).into_any_element(),
                MainView::Runner => self.render_runner(cx).into_any_element(),
                MainView::Cookies => self.render_cookies(cx).into_any_element(),
                MainView::Request => main_col.into_any_element(),
            })
    }
}
