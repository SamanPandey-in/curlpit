//! The visual vocabulary shared by every view: the monospace metrics the response viewer
//! measures itself with, and the button factories that give the segmented rows and the
//! request tabs their one consistent look. Colour itself lives in `theme`; this module
//! only decides which of its roles a control plays.
//! Deliberately tiny and free of app state, so any render module can lean on it.

use crate::theme::Palette;
use gpui::{App, ElementId, Hsla, Window};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};

/// Width of the sticky line-number gutter (px).
pub(crate) const GUTTER: f32 = 76.0;

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

/// Pixels available for text in the response pane.
pub(crate) fn avail_width(window: &Window) -> f32 {
    (f32::from(window.viewport_size().width) - GUTTER - 12.0).max(50.0)
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

/// The same treatment for an open request tab, which sits on the tab bar rather than on a
/// panel: the active tab is the pane showing through, as Zed's tabs are.
fn tab_style(selected: bool, cx: &App) -> ButtonCustomVariant {
    let p = Palette::of(cx);
    let base = ButtonCustomVariant::new(cx).shadow(false);
    if selected {
        base.color(p.tab_active_background())
            .border(p.tab_active_background())
            .foreground(p.text())
            .hover(p.tab_active_background())
            .active(p.tab_active_background())
    } else {
        let clear = Hsla::transparent_black();
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
        .custom(seg_style(selected, cx))
}

pub(crate) fn seg_btn(id: &'static str, label: String, selected: bool, cx: &App) -> Button {
    Button::new(id).label(label).custom(seg_style(selected, cx))
}

/// One open request tab (D26).
pub(crate) fn tab_btn(id: (&'static str, usize), label: String, active: bool, cx: &App) -> Button {
    Button::new(id).label(label).custom(tab_style(active, cx))
}

/// A control that belongs to the chrome but carries no state of its own: the `✕` on a
/// tab, `+`, the Find stepper. Flat until hovered, so it does not add a second row of
/// boxes next to the tabs.
pub(crate) fn quiet_btn(id: impl Into<ElementId>, label: String, cx: &App) -> Button {
    let p = Palette::of(cx);
    let clear = Hsla::transparent_black();
    Button::new(id).label(label).custom(
        ButtonCustomVariant::new(cx)
            .shadow(false)
            .color(clear)
            .border(clear)
            .foreground(p.text_muted())
            .hover(p.element_hover())
            .active(p.element_selected()),
    )
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
