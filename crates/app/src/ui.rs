//! The visual vocabulary shared by every view: the monospace metrics the response viewer
//! measures itself with, and the button factories that give the segmented rows and the
//! request tabs their one consistent look. Colour itself lives in `theme`; this module
//! only decides which of its roles a control plays.
//! Deliberately tiny and free of app state, so any render module can lean on it.

use crate::theme::{Palette, S1, S2, SMALL_TEXT};
use gpui::prelude::*;
use gpui::{div, px, App, Div, ElementId, Hsla, Keystroke, SharedString, Stateful, Window};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_component::kbd::Kbd;
use gpui_component::{IconName, Sizable as _};

/// Width of the sticky line-number gutter (px).
pub(crate) const GUTTER: f32 = 64.0;

pub(crate) fn mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

/// Advance width of one monospace character at 13 px (estimate; used for wrapping and scroll limits).
pub(crate) fn char_w() -> f32 {
    if cfg!(target_os = "windows") {
        7.2
    } else {
        7.83
    }
}

/// Pixels available for text in the response pane, given how much of the window the
/// sidebar takes (0 when it is hidden). The 12 px is the vertical scrollbar.
pub(crate) fn avail_width(window: &Window, side_w: f32) -> f32 {
    (f32::from(window.viewport_size().width) - side_w - GUTTER - 12.0).max(50.0)
}

/// How a segmented row marks the chosen segment.
///
/// Selection used to be `Button::primary()`, which fills the segment with the accent
/// colour: six of those in a row read as six call-to-action buttons. Zed marks the chosen
/// one with a quiet surface change and full-strength text, and leaves the rest flat, so
/// the eye lands on the content instead of the control.
fn seg_style(selected: bool, cx: &App) -> ButtonCustomVariant {
    let p = Palette::of(cx);
    // Unselected segments are transparent rather than a colour of their own, so one
    // factory works on the panel, on the tab bar and on the status row alike.
    let clear = Hsla::transparent_black();
    let base = ButtonCustomVariant::new(cx).shadow(false);
    if selected {
        base.color(p.element_selected())
            .border(p.element_selected())
            .foreground(p.text())
            .hover(p.element_selected())
            .active(p.element_selected())
    } else {
        base.color(clear)
            .border(clear)
            .foreground(p.text_muted())
            .hover(p.element_hover())
            .active(p.element_selected())
    }
}

/// `seg_btn` for a row of buttons sharing one id prefix.
pub(crate) fn seg_btn_ix(
    id: &'static str,
    ix: usize,
    label: &str,
    selected: bool,
    cx: &App,
) -> Button {
    Button::new((id, ix))
        .label(label.to_string())
        .small()
        .custom(seg_style(selected, cx))
}

pub(crate) fn seg_btn(id: &'static str, label: String, selected: bool, cx: &App) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .custom(seg_style(selected, cx))
}

/// A control that belongs to the chrome but carries no state of its own: the `✕` on a
/// tab, `+`, the Find stepper. Flat until hovered, so it does not add a second row of
/// boxes next to the tabs.
pub(crate) fn quiet_btn(id: impl Into<ElementId>, label: String, cx: &App) -> Button {
    Button::new(id).label(label).small().custom(quiet_style(cx))
}

fn quiet_style(cx: &App) -> ButtonCustomVariant {
    let p = Palette::of(cx);
    let clear = Hsla::transparent_black();
    ButtonCustomVariant::new(cx)
        .shadow(false)
        .color(clear)
        .border(clear)
        .foreground(p.text_muted())
        .hover(p.element_hover())
        .active(p.element_selected())
}

/// A flat, square icon-only button with a tooltip: the toolbar and status-bar control in
/// Zed. Icons carry no words, so the tooltip is not optional.
pub(crate) fn icon_btn(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: &'static str,
    cx: &App,
) -> Button {
    Button::new(id)
        .icon(icon)
        .small()
        .tooltip(tooltip)
        .custom(quiet_style(cx))
}

/// An editor sub-tab (Params, Headers, Body…): text with an accent underline when
/// chosen, and an optional muted count after it. Zed reserves filled backgrounds for
/// selection in lists; a row of tabs is marked by the line under the chosen one.
pub(crate) fn under_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    badge: Option<String>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let p = Palette::of(cx);
    let (fg, line) = if selected {
        (p.text(), p.text_accent())
    } else {
        (p.text_muted(), Hsla::transparent_black())
    };
    let hover_fg = p.text();
    div()
        .id(id)
        .flex_none()
        .h_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(S1 + 2.0))
        .px(px(S2 + 2.0))
        .border_b_2()
        .border_color(line)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.text_color(hover_fg))
        .child(label.into())
        .children(badge.map(|b| {
            div()
                .text_size(px(SMALL_TEXT - 1.0))
                .px(px(5.0))
                .rounded(px(3.0))
                .bg(p.element_hover())
                .text_color(p.text_muted())
                .child(b)
        }))
}

/// The platform's primary modifier in a binding string (`cmd` on macOS, `ctrl` elsewhere).
pub(crate) fn primary_key(rest: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{rest}")
    } else {
        format!("ctrl-{rest}")
    }
}

/// A binding as words for running text, e.g. `key_label("S")` is "Ctrl+S" (or "⌘S").
pub(crate) fn key_label(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{key}")
    } else {
        format!("Ctrl+{key}")
    }
}

/// A key binding drawn as key caps, e.g. for the empty-pane hints.
pub(crate) fn kbd(binding: &str) -> impl IntoElement {
    let mut row = div().flex().flex_row().gap(px(2.0));
    for part in binding.split(' ') {
        if let Ok(k) = Keystroke::parse(part) {
            row = row.child(Kbd::new(k));
        }
    }
    row
}

pub(crate) enum Status {
    Idle,
    Info(String),
    Loading,
    Done {
        code: u16,
        text: String,
        ms: u128,
        size: u64,
    },
    Error(String),
}
