//! The visual vocabulary shared by every view: the fixed colour helpers, the monospace
//! metrics the response viewer measures itself with, and the segmented-button factory.
//! Deliberately tiny and free of app state, so any render module can lean on it.

use at_core::json_tree::Kind;
use gpui::{rgb, Hsla, Window};
use gpui_component::button::{Button, ButtonVariants};

/// Width of the sticky line-number gutter (px).
pub(crate) const GUTTER: f32 = 76.0;

pub(crate) fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

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

pub(crate) fn status_color(code: u16) -> Hsla {
    match code {
        200..=299 => c(0xa6e3a1), // green
        300..=399 => c(0xf9e2af), // amber
        _ => c(0xf38ba8),         // red
    }
}

/// FR-RES-03 asks for a colour per JSON type. Fixed values, like the Find highlight, so a
/// type reads the same in the light and the dark theme.
pub(crate) fn kind_color(kind: Kind) -> Hsla {
    match kind {
        Kind::String => c(0xa6e3a1),
        Kind::Number => c(0xfab387),
        Kind::Bool => c(0xcba6f7),
        Kind::Null | Kind::Object | Kind::Array => c(0x9399b2),
    }
}

/// `seg_btn` for a row of buttons sharing one id prefix.
pub(crate) fn seg_btn_ix(id: &'static str, ix: usize, label: &str, selected: bool) -> Button {
    let b = Button::new((id, ix)).label(label.to_string());
    if selected {
        b.primary()
    } else {
        b
    }
}

pub(crate) fn seg_btn(id: &'static str, label: String, selected: bool) -> Button {
    let b = Button::new(id).label(label);
    if selected {
        b.primary()
    } else {
        b
    }
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
