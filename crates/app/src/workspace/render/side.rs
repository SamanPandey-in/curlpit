//! The left panel: saved collections as a tree, or the history of sent requests. Laid
//! out as a Zed panel — a header of tabs, rows that highlight on hover, icons that say
//! what each row is — so it reads as one surface rather than a column of buttons.

use crate::sidebar::{ItemKind, SideView};
use crate::theme::{Palette, BAR_H, S1, S2, SMALL_TEXT};
use crate::ui::{icon_btn, key_label, mono, quiet_btn, under_tab};
use crate::workspace::Workspace;
use at_history::{ago, now_ms};
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context};
use gpui_component::input::Input;
use gpui_component::{Icon, IconName, Sizable as _};

/// Height of one tree row.
const ROW_H: f32 = 26.0;
/// Indent per tree level.
const INDENT: f32 = 14.0;

impl Workspace {
    pub(super) fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let head = div()
            .flex_none()
            .h(px(BAR_H))
            .flex()
            .flex_row()
            .items_center()
            .pr(px(S1))
            .border_b_1()
            .border_color(p.border())
            .child(
                under_tab(
                    "side-col",
                    "Collections",
                    None,
                    self.side_view == SideView::Collections,
                    cx,
                )
                .on_click(
                    cx.listener(|this, _ev, _window, cx| this.show_side(SideView::Collections, cx)),
                ),
            )
            .child(
                under_tab(
                    "side-hist",
                    "History",
                    None,
                    self.side_view == SideView::History,
                    cx,
                )
                .on_click(
                    cx.listener(|this, _ev, _window, cx| this.show_side(SideView::History, cx)),
                ),
            )
            .child(div().flex_1())
            .children((self.side_view == SideView::Collections).then(|| {
                icon_btn("new-col", IconName::Plus, "New collection", cx)
                    .on_click(cx.listener(|this, _ev, _window, cx| this.on_new_collection(cx)))
            }));

        let body = match self.side_view {
            SideView::Collections => self.render_collections(cx),
            SideView::History => self.render_history(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface())
            .child(head)
            .child(body)
    }

    fn render_collections(&self, cx: &Context<Self>) -> AnyElement {
        let p = Palette::of(cx);
        let tab = self.tab();
        let mut list = div()
            .id("sidebar-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .py(px(S1));
        if self.items.is_empty() {
            list = list.child(empty_note(
                "No collections yet",
                format!(
                    "Create one with + above, or save a request ({}).",
                    key_label("S")
                ),
                cx,
            ));
        }
        for (ix, item) in self.items.iter().enumerate() {
            let selected = match item.kind {
                ItemKind::Request => tab.current_path.as_ref() == Some(&item.path),
                ItemKind::Collection | ItemKind::Folder => {
                    tab.current_path.is_none() && tab.save_dir.as_ref() == Some(&item.path)
                }
                ItemKind::Broken(_) => false,
            };
            let (icon, icon_col, text_col) = match &item.kind {
                ItemKind::Collection => (IconName::ChevronDown, p.text_muted(), p.text()),
                ItemKind::Folder => (IconName::FolderOpen, p.text_muted(), p.text()),
                ItemKind::Request => (IconName::File, p.text_placeholder(), p.text()),
                ItemKind::Broken(_) => {
                    (IconName::TriangleAlert, p.status_error(), p.status_error())
                }
            };
            let hover = p.element_hover();
            list = list.child(
                div()
                    .id(("side", ix))
                    .flex_none()
                    .h(px(ROW_H))
                    .mx(px(S1))
                    .rounded(px(4.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(S1 + item.depth as f32 * INDENT))
                    .pr(px(S2))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(p.element_selected()))
                    .when(!selected, |d| d.hover(move |s| s.bg(hover)))
                    .child(Icon::new(icon).small().text_color(icon_col))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(text_col)
                            .when(matches!(item.kind, ItemKind::Collection), |d| {
                                d.font_weight(gpui::FontWeight::MEDIUM)
                            })
                            .child(item.label.clone()),
                    )
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.on_sidebar_click(ix, window, cx)
                    })),
            );
        }
        let foot = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .overflow_hidden()
            .gap(px(2.))
            .px(px(S1))
            .py(px(S1))
            .border_t_1()
            .border_color(p.border())
            .child(
                quiet_btn("col-import", "Import…".to_string(), cx)
                    .xsmall()
                    .tooltip("Import a Postman collection, HAR, environment or cURL")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_import(window, cx))),
            )
            .child(
                quiet_btn("col-paste", "Paste".to_string(), cx)
                    .xsmall()
                    .tooltip("Import a collection or environment from the clipboard")
                    .on_click(
                        cx.listener(|this, _ev, window, cx| this.import_from_clipboard(window, cx)),
                    ),
            )
            .child(
                quiet_btn("col-export", "Export…".to_string(), cx)
                    .xsmall()
                    .tooltip("Export the selected collection")
                    .on_click(
                        cx.listener(|this, _ev, window, cx| this.on_export_collection(window, cx)),
                    ),
            )
            .child(div().flex_1())
            .child(
                quiet_btn(
                    "col-run",
                    if self.runner.cancel.is_some() {
                        "Run… ●"
                    } else {
                        "Run…"
                    }
                    .to_string(),
                    cx,
                )
                .xsmall()
                .tooltip("Run every request in the selected collection")
                .on_click(cx.listener(|this, _ev, _window, cx| this.open_runner(cx))),
            );
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list)
            .child(foot)
            .into_any_element()
    }

    fn render_history(&self, cx: &Context<Self>) -> AnyElement {
        let p = Palette::of(cx);
        let now = now_ms();
        let mut list = div()
            .id("hist-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .py(px(S1));
        if self.history.is_some() && self.hist_items.is_empty() {
            list = list.child(if self.hist_search.read(cx).value().trim().is_empty() {
                empty_note(
                    "No requests yet",
                    format!(
                        "Everything you send appears here. Send with {}.",
                        key_label("Enter")
                    ),
                    cx,
                )
            } else {
                div()
                    .p(px(S2 + S1))
                    .text_color(p.text_placeholder())
                    .child("No matches.")
                    .into_any_element()
            });
        }
        for (ix, s) in self.hist_items.iter().enumerate() {
            let (code, code_col) = match s.status {
                Some(n) => (n.to_string(), p.status_class(n)),
                None => ("ERR".to_string(), p.status_error()),
            };
            let id = s.id;
            let hover = p.element_hover();
            list = list.child(
                div()
                    .id(("hist", ix))
                    .flex_none()
                    .mx(px(S1))
                    .px(px(S2))
                    .py(px(S1 + 1.0))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(move |st| st.bg(hover))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(S2))
                            .font_family(mono())
                            .text_size(px(SMALL_TEXT - 1.0))
                            .child(
                                div()
                                    .text_color(p.method_color(&s.method))
                                    .child(s.method.clone()),
                            )
                            .child(div().text_color(code_col).child(code))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_color(p.text_placeholder())
                                    .child(ago(now, s.ts_ms)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(px(SMALL_TEXT + 1.0))
                            .text_color(p.text_muted())
                            .child(if s.url.is_empty() {
                                "(no URL)".to_string()
                            } else {
                                s.url.clone()
                            }),
                    )
                    .on_click(
                        cx.listener(move |this, _ev, window, cx| this.open_history(id, window, cx)),
                    ),
            );
        }
        let tools = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(S1))
            .p(px(S2))
            .child(
                Input::new(&self.hist_search)
                    .small()
                    .flex_1()
                    .min_w_0()
                    .prefix(
                        Icon::new(IconName::Search)
                            .small()
                            .text_color(p.text_placeholder()),
                    )
                    .cleanable(true),
            )
            .child(
                quiet_btn(
                    "hist-clear",
                    if self.confirm_clear {
                        "Really clear?"
                    } else {
                        "Clear"
                    }
                    .to_string(),
                    cx,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    if this.confirm_clear {
                        this.clear_history(cx);
                    } else {
                        this.confirm_clear = true;
                        cx.notify();
                    }
                })),
            );
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .children(self.history.is_some().then_some(tools))
            .children(self.hist_error.clone().map(|e| {
                div()
                    .px(px(S2))
                    .pb(px(S2))
                    .text_color(p.status_error())
                    .child(e)
            }))
            .child(list)
            .into_any_element()
    }
}

/// The empty-panel message: a title and a one-line hint.
fn empty_note(title: &'static str, hint: String, cx: &Context<Workspace>) -> AnyElement {
    let p = Palette::of(cx);
    div()
        .p(px(S2 + S1))
        .flex()
        .flex_col()
        .gap(px(S1))
        .child(div().text_color(p.text_muted()).child(title))
        .child(
            div()
                .text_size(px(SMALL_TEXT))
                .text_color(p.text_placeholder())
                .child(hint),
        )
        .into_any_element()
}
