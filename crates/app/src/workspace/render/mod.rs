//! `Render for Workspace`: the window itself, assembled the way Zed assembles its own —
//! title bar, a resizable sidebar beside the main pane, a status bar along the bottom —
//! from the builders in the sibling modules (`chrome` for the bars, `side` for the
//! sidebar). Inside the main pane the request editor and the response sit in a vertical
//! split the user can drag. Everything that depends on the current window geometry is
//! computed here, because only `render` is handed the `Window`.

mod auth;
mod body;
mod chrome;
mod code;
mod cookies;
mod env_page;
mod kv;
mod palette;
mod runner;
mod scripts;
mod settings;
mod settings_page;
mod side;
mod tree;

use crate::actions::{
    CloseTab, FindInResponse, NewTab, PaletteDismiss, PaletteNext, PalettePrev, ReopenTab,
    SaveRequest, SendRequest, ToggleCommandPalette, ToggleSidebar,
};
use crate::editor::{next_method, AuthKind, ReqTab, Which};
use crate::format::{body_label, human};
use crate::response::{row_spans, ResTab, Span, FIND_MAX};
use crate::sidebar::MainView;
use crate::theme::{Palette, BAR_H, ROW_PAD_Y, S1, S2, S3, SMALL_TEXT};
use crate::ui::{
    avail_width, char_w, icon_btn, kbd, key_label, mono, primary_key, quiet_btn, under_tab, Status,
    GUTTER,
};
use crate::workspace::Workspace;
use at_core::lines::{build_rows, CONT};
use at_core::Method;
use gpui::prelude::*;
use gpui::{
    div, px, uniform_list, AnyElement, ClipboardItem, Context, ExternalPaths, Focusable as _, Hsla,
    ScrollWheelEvent, Window,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _};
use std::sync::Arc;

/// Default sidebar width.
const SIDE_W: f32 = 260.0;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            window.focus(&self.focus);
            self.focused_once = true;
        }
        self.ensure_tree();
        // ---- viewer geometry (depends on the window width and the sidebar's) ----
        let side_w = if self.sidebar_open {
            self.side_split
                .read(cx)
                .sizes()
                .first()
                .map(|w| f32::from(*w))
                .unwrap_or(SIDE_W)
                + 1.0
        } else {
            0.0
        };
        let avail = avail_width(window, side_w);
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

        // One palette, read once: the window, the library's widgets and everything drawn
        // by hand below all come from here (see `theme`).
        let p = Palette::of(cx);
        let (bg, fg, muted) = (p.editor_background(), p.text(), p.text_muted());
        let in_flight = tab.cancel.is_some();
        let clear = Hsla::transparent_black();

        // ---- toolbar: the request's name, and what you can do with the request ----
        let toolbar = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(S1))
            .px(px(S2))
            .pt(px(S2))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(420.))
                    .child(Input::new(&tab.name).appearance(false).small()),
            )
            .child(div().flex_1())
            .child(
                quiet_btn("save", "Save".to_string(), cx)
                    .tooltip(format!("Save request ({})", key_label("S")))
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_save(window, cx))),
            )
            .child(
                icon_btn("copy-curl", IconName::Copy, "Copy as cURL", cx)
                    .on_click(cx.listener(|this, _ev, _window, cx| this.on_copy_curl(cx))),
            );

        // ---- URL bar: [METHOD v | url ............] [Send] ----
        // The method and the URL share one field, as every HTTP client draws them; the
        // field takes the focus ring when the URL input inside it is focused.
        let url_focused = tab.url.read(cx).focus_handle(cx).is_focused(window);
        let cur = tab.method;
        let ws = cx.entity().downgrade();
        let method_btn = Button::new("method")
            .small()
            .label(cur.as_str())
            .dropdown_caret(true)
            .custom(
                ButtonCustomVariant::new(cx)
                    .shadow(false)
                    .color(clear)
                    .border(clear)
                    .foreground(p.method_color(cur.as_str()))
                    .hover(p.element_hover())
                    .active(p.element_selected()),
            )
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu.min_w(px(140.));
                let mut m = Method::Get;
                loop {
                    let ws = ws.clone();
                    let pick = m;
                    menu = menu.item(
                        PopupMenuItem::new(pick.as_str())
                            .checked(pick == cur)
                            .on_click(move |_, _, cx| {
                                let _ = ws.update(cx, |this, cx| {
                                    this.tab_mut().method = pick;
                                    cx.notify();
                                });
                            }),
                    );
                    m = next_method(m);
                    if m == Method::Get {
                        break menu;
                    }
                }
            });
        let send_tip = format!("Send request ({})", key_label("Enter"));
        let send_btn = if in_flight {
            Button::new("send").label("Cancel").outline()
        } else {
            Button::new("send").primary().label("Send")
        };
        let url_bar = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(S2))
            .p(px(S2))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(if url_focused {
                        p.border_focused()
                    } else {
                        p.border()
                    })
                    .bg(p.element_background())
                    .child(div().flex_none().pl(px(3.)).child(method_btn))
                    .child(div().flex_none().w(px(1.)).h(px(18.)).bg(p.border()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&tab.url).appearance(false)),
                    ),
            )
            .child(
                send_btn
                    .h(px(32.))
                    .px(px(S3 + S1))
                    .tooltip(send_tip)
                    .on_click(cx.listener(|this, _ev, _window, cx| this.send_active(cx))),
            );

        // ---- request editor: sub-tabs + panel ----
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
        let count = |n: usize| (n > 0).then(|| n.to_string());
        let req_tab = |id: &'static str, label: String, badge: Option<String>, which: ReqTab| {
            under_tab(id, label, badge, tab.req_tab == which, cx).on_click(cx.listener(
                move |this, _ev, _window, cx| {
                    this.tab_mut().req_tab = which;
                    cx.notify();
                },
            ))
        };
        let req_tabs = div()
            .flex_none()
            .h(px(BAR_H - 2.0))
            .flex()
            .flex_row()
            .px(px(S1))
            .border_b_1()
            .border_color(p.border_variant())
            .child(req_tab(
                "t-params",
                "Params".into(),
                count(pcount),
                ReqTab::Params,
            ))
            .child(req_tab(
                "t-headers",
                "Headers".into(),
                count(hcount),
                ReqTab::Headers,
            ))
            .child(req_tab(
                "t-body",
                body_label(tab.body_mode, tab.raw_kind),
                None,
                ReqTab::Body,
            ))
            .child(req_tab(
                "t-auth",
                "Auth".into(),
                match tab.auth_kind {
                    AuthKind::None => None,
                    k => Some(k.label().to_string()),
                },
                ReqTab::Auth,
            ))
            .child(req_tab(
                "t-scripts",
                "Scripts".into(),
                Self::has_scripts(tab, cx).then(|| "on".to_string()),
                ReqTab::Scripts,
            ))
            .child(req_tab(
                "t-settings",
                "Settings".into(),
                None,
                ReqTab::Settings,
            ));
        let req_panel = div()
            .id("req-panel")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(match tab.req_tab {
                ReqTab::Params => self.render_kv(tab, Which::Params, cx).into_any_element(),
                ReqTab::Headers => self.render_kv(tab, Which::Headers, cx).into_any_element(),
                ReqTab::Body => self.render_body(tab, cx).into_any_element(),
                ReqTab::Auth => self.render_auth(tab, cx).into_any_element(),
                ReqTab::Scripts => self.render_scripts(tab, cx).into_any_element(),
                ReqTab::Settings => self.render_settings(tab, cx).into_any_element(),
            });

        // ---- response header: view tabs on the left, the outcome on the right ----
        let outcome: AnyElement = match &tab.status {
            Status::Idle => div().into_any_element(),
            Status::Loading => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(S2))
                .text_color(muted)
                .child(Spinner::new().small().color(p.text_accent()))
                .child("Sending…")
                .into_any_element(),
            Status::Info(m) => div()
                .flex()
                .min_w_0()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(muted)
                        .child(m.clone()),
                )
                .into_any_element(),
            Status::Done {
                code,
                text,
                ms,
                size,
            } => {
                let col = p.status_class(*code);
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(S2 + 2.0))
                    .font_family(mono())
                    .text_size(px(SMALL_TEXT))
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(col.alpha(0.16))
                            .text_color(col)
                            .child(format!("{code} {text}")),
                    )
                    .child(div().text_color(muted).child(format!("{ms} ms")))
                    .child(div().text_color(muted).child(human(*size)))
                    .into_any_element()
            }
            Status::Error(e) => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(S1 + 2.0))
                .min_w_0()
                .text_color(p.status_error())
                .child(Icon::new(IconName::CircleX).small().flex_none())
                .child(div().flex_1().min_w_0().truncate().child(e.clone()))
                .into_any_element(),
        };

        let has_body = tab.res_tab == ResTab::Body && tab.view.is_some();
        // The wrap toggle appears only when text overflows the pane (or while wrap is on).
        let wrap_btn = (tab.res_tab == ResTab::Body && (tab.wrap || overflow)).then(|| {
            quiet_btn("wrap", "Wrap".to_string(), cx)
                .selected(tab.wrap)
                .xsmall()
                .tooltip(if tab.wrap {
                    "Soft wrap is on"
                } else {
                    "Soft wrap long lines"
                })
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
        let copy_btn = has_body.then(|| {
            icon_btn(
                "copy",
                if tab.copied {
                    IconName::Check
                } else {
                    IconName::Copy
                },
                "Copy response body",
                cx,
            )
            .xsmall()
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
        let res_tab = |id: &'static str, label: String, badge: Option<String>, which: ResTab| {
            under_tab(id, label, badge, tab.res_tab == which, cx).on_click(cx.listener(
                move |this, _ev, _window, cx| {
                    this.tab_mut().res_tab = which;
                    if which == ResTab::Code {
                        this.code_copied = false;
                    }
                    cx.notify();
                },
            ))
        };
        let warning = tab.warning.clone().map(|w| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(S1))
                .min_w_0()
                .text_size(px(SMALL_TEXT))
                .text_color(p.warning_text())
                .child(Icon::new(IconName::TriangleAlert).xsmall())
                .child(div().truncate().child(w))
        });
        let resp_header = div()
            .flex_none()
            .h(px(BAR_H - 2.0))
            .flex()
            .flex_row()
            .items_center()
            .pl(px(S1))
            .pr(px(S3))
            .gap(px(S2))
            .bg(p.surface())
            .border_b_1()
            .border_color(p.border())
            .child(
                div()
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_row()
                    .child(res_tab("r-body", "Body".into(), None, ResTab::Body))
                    .child(res_tab("r-tree", "Tree".into(), None, ResTab::Tree))
                    .child(res_tab(
                        "r-headers",
                        "Headers".into(),
                        count(tab.resp_headers.len()),
                        ResTab::Headers,
                    ))
                    .child(res_tab(
                        "r-tests",
                        tab.report
                            .as_ref()
                            .map(|r| r.label())
                            .unwrap_or_else(|| "Tests".to_string()),
                        None,
                        ResTab::Tests,
                    ))
                    .child(res_tab("r-code", "Code".into(), None, ResTab::Code)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .children(has_body.then(|| {
                        icon_btn("find-open", IconName::Search, "Find in response", cx)
                            .xsmall()
                            .on_click(
                                cx.listener(|this, _ev, window, cx| this.open_find(window, cx)),
                            )
                    }))
                    .children(copy_btn)
                    .children(has_body.then(|| {
                        icon_btn(
                            "save-body",
                            IconName::ArrowDown,
                            "Save response to a file",
                            cx,
                        )
                        .xsmall()
                        .on_click(
                            cx.listener(|this, _ev, window, cx| this.on_save_body(window, cx)),
                        )
                    }))
                    .children(wrap_btn),
            )
            // Warning and outcome share the rest of the bar and are what gives way when
            // it is narrow: they shrink and ellipsize instead of running off the edge.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(S3))
                    .children(warning)
                    .child(div().flex_shrink().min_w_0().max_w(px(520.)).child(outcome)),
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
                // Find colours are resolved out here: the list closure outlives this render
                // pass, so it cannot look the theme up for itself.
                let (hit_bg, cur_bg, hit_fg) =
                    (p.find_match_bg(), p.find_current_bg(), p.find_match_fg());
                let (row_bg, gutter_fg) = (p.find_row_bg(), p.gutter_text());
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
                                .when(hl_row == Some(ix), |d| d.bg(row_bg))
                                // sticky gutter: lives outside the horizontally shifted text
                                .child(
                                    div()
                                        .w(px(GUTTER))
                                        .flex_none()
                                        .flex()
                                        .justify_end()
                                        // Zed's gutter: numbers right-aligned against the
                                        // text, no rule, so the eye runs straight across.
                                        .pr(px(S3 + S1))
                                        .text_color(gutter_fg)
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
                                                // theme-aware: the text on a highlight is the
                                                // pane colour, so it inverts with the appearance
                                                match kind {
                                                    Span::Current => {
                                                        d.bg(cur_bg).text_color(hit_fg)
                                                    }
                                                    Span::Hit => d.bg(hit_bg).text_color(hit_fg),
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
            None => self.render_empty_response(cx),
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
                    .gap(px(S1))
                    .px(px(S2))
                    .py(px(ROW_PAD_Y))
                    .bg(p.surface())
                    .border_b_1()
                    .border_color(p.border())
                    .child(div().w(px(260.)).child(Input::new(&tab.find).small()))
                    .child(div().w(px(90.)).text_color(muted).child(count))
                    .child(quiet_btn("find-prev", "↑".to_string(), cx).on_click(
                        cx.listener(move |this, _ev, _window, cx| this.find_step(id, -1, cx)),
                    ))
                    .child(quiet_btn("find-next", "↓".to_string(), cx).on_click(
                        cx.listener(move |this, _ev, _window, cx| this.find_step(id, 1, cx)),
                    ))
                    .child(
                        quiet_btn("find-close", "✕".to_string(), cx).on_click(cx.listener(
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
                        )),
                    )
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
                .p(px(S2))
                .flex()
                .flex_col()
                .gap(px(S1))
                .children(tab.resp_headers.iter().map(|(k, v)| {
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(S3))
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

        let request_pane = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .child(req_tabs)
            .child(req_panel);
        let response_pane = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .child(resp_header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .font_family(mono())
                    .text_size(px(13.))
                    // horizontal scrolling is ours (so the gutter can stay put); vertical is the list's
                    .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, window, cx| {
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
                        let max = tab.max_scroll_x(avail_width(window, side_w));
                        tab.scroll_x = (tab.scroll_x - dx).clamp(0.0, max);
                        cx.notify();
                    }))
                    .child(resp_area),
            );

        let main_col = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .child(self.render_tab_bar(cx))
            .child(toolbar)
            .child(url_bar)
            .child(
                div().flex_1().min_h_0().child(
                    v_resizable("req-split")
                        .child(
                            resizable_panel()
                                .size(px(230.))
                                .size_range(px(80.)..px(2000.))
                                .child(request_pane),
                        )
                        .child(resizable_panel().child(response_pane)),
                ),
            );

        let main = match self.main_view {
            MainView::Environments => self.render_env_page(cx).into_any_element(),
            MainView::Runner => self.render_runner(cx).into_any_element(),
            MainView::Cookies => self.render_cookies(cx).into_any_element(),
            MainView::Settings => self.render_settings_page(cx).into_any_element(),
            MainView::Request => main_col.into_any_element(),
        };

        // The workspace proper: everything below the title bar. It owns the key context
        // and the focus, so the shortcuts work wherever focus is inside it.
        let workspace = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
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
            .on_action(cx.listener(|this, _: &ToggleSidebar, _window, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleCommandPalette, window, cx| {
                this.toggle_palette(window, cx)
            }))
            .on_action(cx.listener(|this, _: &PaletteNext, _window, cx| this.palette_move(1, cx)))
            .on_action(cx.listener(|this, _: &PalettePrev, _window, cx| this.palette_move(-1, cx)))
            .on_action(cx.listener(|this, _: &PaletteDismiss, _window, cx| this.close_palette(cx)))
            // Drop a collection, environment or HAR file anywhere to import it (FR-IE-09).
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.import_dropped(paths.paths(), window, cx)
            }))
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("side-split")
                        .with_state(&self.side_split)
                        .child(
                            resizable_panel()
                                .size(px(SIDE_W))
                                .size_range(px(180.)..px(520.))
                                .visible(self.sidebar_open)
                                .child(self.render_sidebar(cx)),
                        )
                        .child(
                            resizable_panel().child(
                                div()
                                    .size_full()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .overflow_hidden()
                                    .child(main),
                            ),
                        ),
                ),
            )
            .child(self.render_status_bar(cx));

        // The title bar sits outside the focus-tracking element on purpose. Focus tracking
        // marks every mouse-down inside it as handled, and on Windows a handled mouse-down
        // on the caption never reaches the OS, so the window could not be dragged.
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .child(self.render_title_bar(cx))
            .child(workspace)
            .when(self.palette.is_some(), |d| d.child(self.render_palette(cx)))
    }
}

impl Workspace {
    /// What the response pane shows before there is a response: the keys that drive the
    /// app, the way Zed fills an empty pane — or, after a failed send, the whole error,
    /// which the one-line outcome in the header can only show the start of.
    fn render_empty_response(&self, cx: &Context<Self>) -> AnyElement {
        let p = Palette::of(cx);
        let ui_font = cx.theme().font_family.clone();
        let tab = self.tab();
        let center = div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(S2))
            .p(px(S3))
            .font_family(ui_font);
        match &tab.status {
            Status::Loading => center
                .child(Spinner::new().large().color(p.text_accent()))
                .into_any_element(),
            Status::Error(e) => center
                .child(
                    Icon::new(IconName::CircleX)
                        .large()
                        .text_color(p.status_error()),
                )
                .child(div().text_color(p.text()).child("The request failed"))
                .child(
                    div()
                        .max_w(px(560.))
                        .text_center()
                        .text_color(p.text_muted())
                        .child(e.clone()),
                )
                .into_any_element(),
            _ => {
                let hint = |label: &'static str, keys: String| {
                    div()
                        .w(px(260.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_color(p.text_muted()).child(label))
                        .child(kbd(&keys))
                };
                center
                    .gap(px(S2 + 2.0))
                    .child(hint("Send request", primary_key("enter")))
                    .child(hint("New request", primary_key("t")))
                    .child(hint("Save request", primary_key("s")))
                    .child(hint("Find in response", primary_key("f")))
                    .child(hint("Toggle sidebar", primary_key("b")))
                    .child(hint("Command palette", primary_key("k")))
                    .into_any_element()
            }
        }
    }
}
