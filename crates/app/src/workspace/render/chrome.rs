//! The window chrome around the request pane, laid out the way Zed lays out its own:
//! a title bar that carries the global context (the environment picker), a tab bar of
//! open requests, and a status bar along the bottom for panel toggles and the pages that
//! are not a request (variables, cookies, the runner).

use crate::theme::{Palette, BAR_H, S1, S2, S3, SMALL_TEXT, STATUS_H};
use crate::ui::{icon_btn, mono, quiet_btn, Status};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, Context, Hsla, SharedString};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::spinner::Spinner;
use gpui_component::{Icon, IconName, Selectable as _, Sizable as _, TitleBar};

impl Workspace {
    /// The title bar: what this window is, and the environment every request resolves
    /// against. The environment is global context, so it sits where Zed shows the project.
    pub(super) fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let env_name = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.name.clone());
        let ws = cx.entity().downgrade();
        let names: Vec<String> = self.envs.iter().map(|(e, _)| e.name.clone()).collect();
        let active = self.active_env;
        let env_variant = ButtonCustomVariant::new(cx)
            .shadow(false)
            .color(Hsla::transparent_black())
            .border(Hsla::transparent_black())
            .foreground(if env_name.is_some() {
                p.text()
            } else {
                p.text_muted()
            })
            .hover(p.element_hover())
            .active(p.element_selected());
        let env_picker = Button::new("env-pick")
            .small()
            .icon(
                Icon::new(IconName::Globe).text_color(if env_name.is_some() {
                    p.success_text()
                } else {
                    p.text_placeholder()
                }),
            )
            .label(env_name.unwrap_or_else(|| "No environment".to_string()))
            .dropdown_caret(true)
            .custom(env_variant)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu.min_w(px(220.)).item({
                    let ws = ws.clone();
                    PopupMenuItem::new("No environment")
                        .checked(active.is_none())
                        .on_click(move |_, _, cx| {
                            let _ = ws.update(cx, |this, cx| this.set_active_env(None, cx));
                        })
                });
                for (ix, name) in names.iter().enumerate() {
                    let ws = ws.clone();
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(active == Some(ix))
                            .on_click(move |_, _, cx| {
                                let _ = ws.update(cx, |this, cx| this.set_active_env(Some(ix), cx));
                            }),
                    );
                }
                let ws = ws.clone();
                menu.separator().item(
                    PopupMenuItem::new("Manage environments…")
                        .icon(IconName::Settings)
                        .on_click(move |_, _, cx| {
                            let _ = ws.update(cx, |this, cx| this.open_envs(cx));
                        }),
                )
            });

        TitleBar::new().child(
            div()
                .flex_1()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .pr(px(S2))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(S2))
                        .child(
                            Icon::new(IconName::SquareTerminal)
                                .small()
                                .text_color(p.text_accent()),
                        )
                        .child(div().text_color(p.text()).child("API Tester"))
                        .child(div().text_color(p.text_placeholder()).child("/"))
                        .child(
                            div()
                                .text_color(p.text_muted())
                                .max_w(px(360.))
                                .truncate()
                                .child(self.tab().title(cx)),
                        ),
                )
                // The whole bar is a window-drag area; on Windows that makes it the OS
                // caption, which swallows clicks. Occluding the picker takes it out of the
                // drag hit-test so it gets its own clicks.
                .child(div().occlude().child(env_picker)),
        )
    }

    /// One tab per open request. The active tab is the pane showing through the bar
    /// (same colour, no bottom rule); each tab leads with its method in that method's
    /// colour, the way Zed leads a file tab with its file-type icon. The close button
    /// shows on hover and on the active tab; an unsaved tab shows a dot there instead
    /// until hovered.
    pub(super) fn render_tab_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let mut strip = div()
            .id("tab-strip")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_row()
            .overflow_x_scroll();
        for (ix, t) in self.tabs.iter().enumerate() {
            let active = ix == self.active;
            let dirty = t.is_dirty(cx);
            let busy = t.cancel.is_some();
            let group: SharedString = format!("tab-{ix}").into();
            let method = t.method.as_str();

            let slot = if t.confirm_close {
                div()
                    .id(("tab-x", ix))
                    .px(px(S1))
                    .rounded(px(3.))
                    .text_size(px(SMALL_TEXT))
                    .text_color(p.status_error())
                    .hover(|s| s.bg(p.element_selected()))
                    .child("Discard?")
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        cx.stop_propagation();
                        this.close_tab(ix, window, cx)
                    }))
                    .into_any_element()
            } else {
                // The ✕ is always present (so the tab never changes width); visibility
                // decides whether it or the unsaved dot is what you see.
                let show_x = active && !dirty;
                div()
                    .relative()
                    .size(px(16.))
                    .flex_none()
                    .children(dirty.then(|| {
                        div()
                            .absolute()
                            .top(px(5.))
                            .left(px(5.))
                            .size(px(6.))
                            .rounded_full()
                            .bg(p.text_muted())
                            .group_hover(group.clone(), |s| s.invisible())
                    }))
                    .child(
                        div()
                            .id(("tab-x", ix))
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .when(!show_x, |d| d.invisible())
                            .group_hover(group.clone(), |s| s.visible())
                            .hover(|s| s.bg(p.element_selected()))
                            .child(
                                Icon::new(IconName::Close)
                                    .xsmall()
                                    .text_color(p.text_muted()),
                            )
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(ix, window, cx)
                            })),
                    )
                    .into_any_element()
            };

            strip = strip.child(
                div()
                    .id(("tab", ix))
                    .group(group)
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(S2))
                    .pl(px(S3))
                    .pr(px(S1 + 2.0))
                    .border_r_1()
                    .border_color(p.border())
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.tab_active_background()))
                    .when(!active, |d| {
                        let hover = p.element_hover();
                        d.border_b_1().hover(move |s| s.bg(hover))
                    })
                    .children(busy.then(|| Spinner::new().xsmall().color(p.text_accent())))
                    .child(
                        div()
                            .font_family(mono())
                            .text_size(px(SMALL_TEXT - 1.0))
                            .text_color(p.method_color(method))
                            .child(method),
                    )
                    .child(
                        div()
                            .max_w(px(220.))
                            .truncate()
                            .text_color(if active { p.text() } else { p.text_muted() })
                            .child(t.title(cx)),
                    )
                    .child(slot)
                    .on_click(
                        cx.listener(move |this, _ev, window, cx| this.activate_tab(ix, window, cx)),
                    ),
            );
        }
        // The rest of the bar keeps the bottom rule the inactive tabs draw.
        strip = strip.child(
            div()
                .flex_1()
                .min_w(px(S2))
                .h_full()
                .border_b_1()
                .border_color(p.border()),
        );

        div()
            .flex_none()
            .h(px(BAR_H))
            .flex()
            .flex_row()
            .bg(p.surface())
            .child(strip)
            .child(
                div()
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .px(px(S1 + 2.0))
                    .border_l_1()
                    .border_b_1()
                    .border_color(p.border())
                    .child(
                        icon_btn("tab-new", IconName::Plus, "New request", cx).on_click(
                            cx.listener(|this, _ev, window, cx| this.add_tab(window, cx)),
                        ),
                    )
                    .child(
                        quiet_btn("tab-curl", "Import cURL".to_string(), cx)
                            .tooltip("Open a request from a cURL command on the clipboard")
                            .on_click(
                                cx.listener(|this, _ev, window, cx| this.on_paste_curl(window, cx)),
                            ),
                    ),
            )
    }

    /// The header shared by the pages that replace the request pane (variables, cookies,
    /// the runner): a back arrow, the page's title and an optional row of actions, on a
    /// bar the height of the tab bar so switching pages does not shift the layout.
    pub(super) fn page_header(
        &self,
        title: &'static str,
        actions: Option<gpui::AnyElement>,
        back: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let p = Palette::of(cx);
        div()
            .flex_none()
            .h(px(BAR_H))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(S2))
            .px(px(S2))
            .bg(p.surface())
            .border_b_1()
            .border_color(p.border())
            .child(
                icon_btn("page-back", IconName::ArrowLeft, "Back to requests", cx)
                    .on_click(cx.listener(move |this, _ev, window, cx| back(this, window, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .text_color(p.text())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            )
            .children(actions)
    }

    /// The bottom bar: the sidebar toggle and app-wide messages on the left, the pages
    /// that are not a request on the right — Zed's place for panel toggles.
    pub(super) fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        // Workspace messages belong to the page that raised them (variables, settings,
        // imports from a page); on the request view the tab's own status says what happened.
        let note = match (
            &self.status,
            self.main_view == crate::sidebar::MainView::Request,
        ) {
            (_, true) => None,
            (Status::Info(m), _) => Some((m.clone(), p.text_muted())),
            (Status::Error(e), _) => Some((e.clone(), p.status_error())),
            _ => None,
        };
        let page = |id: &'static str, label: &'static str, on: bool| {
            let b = quiet_btn(id, label.to_string(), cx);
            if on {
                b.selected(true)
            } else {
                b
            }
        };
        use crate::sidebar::MainView;
        let running = self.runner.cancel.is_some();
        div()
            .flex_none()
            .h(px(STATUS_H))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(S1))
            .bg(p.surface())
            .border_t_1()
            .border_color(p.border())
            .text_size(px(SMALL_TEXT))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(S2))
                    .min_w_0()
                    .child(
                        icon_btn(
                            "toggle-sidebar",
                            if self.sidebar_open {
                                IconName::PanelLeftClose
                            } else {
                                IconName::PanelLeftOpen
                            },
                            "Toggle sidebar",
                            cx,
                        )
                        .xsmall()
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            this.sidebar_open = !this.sidebar_open;
                            cx.notify();
                        })),
                    )
                    .children(note.map(|(m, col)| div().truncate().text_color(col).child(m))),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        page(
                            "open-vars",
                            "Variables",
                            self.main_view == MainView::Environments,
                        )
                        .xsmall()
                        .on_click(cx.listener(|this, _ev, _window, cx| this.open_envs(cx))),
                    )
                    .child(
                        page(
                            "open-cookies",
                            "Cookies",
                            self.main_view == MainView::Cookies,
                        )
                        .xsmall()
                        .on_click(cx.listener(|this, _ev, _window, cx| this.open_cookies(cx))),
                    )
                    .child(
                        page(
                            "open-settings",
                            "Settings",
                            self.main_view == MainView::Settings,
                        )
                        .xsmall()
                        .on_click(
                            cx.listener(|this, _ev, window, cx| this.open_settings(window, cx)),
                        ),
                    )
                    .child(
                        page(
                            "open-runner",
                            if running { "Runner ●" } else { "Runner" },
                            self.main_view == MainView::Runner,
                        )
                        .xsmall()
                        .on_click(cx.listener(|this, _ev, _window, cx| this.open_runner(cx))),
                    ),
            )
    }
}
