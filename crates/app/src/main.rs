//! API Tester — one window, request tabs, send a request, read the response.
//! Network work runs on its own tokio threads; results come back over a channel.
//!
//! The window and its state live in `workspace`; this file only starts the runtime,
//! binds the keys and opens the window. Each module below owns one area: `editor`,
//! `response` and `tree` the two halves of a request tab, `theme` the palette every
//! view paints from, `format` and `ui` the shared pure helpers, `script`, `runner`
//! and `sidebar` the state of the features named.

mod actions;
mod commands;
mod editor;
mod format;
mod request_tab;
mod response;
mod runner;
mod script;
mod sidebar;
mod theme;
mod tree;
mod ui;
mod workspace;

use crate::actions::{
    CloseTab, FindInResponse, NewTab, PaletteDismiss, PaletteNext, PalettePrev, ReopenTab,
    SaveRequest, SendRequest, ToggleCommandPalette, ToggleSidebar,
};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{px, size, App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions};
use gpui_component::{Root, TitleBar};

fn main() {
    // Networking lives on its own small thread pool; the UI thread never blocks on it.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to start the network runtime");
    let handle = rt.handle().clone();

    Application::new()
        // Without these the library's icons (checkbox tick, chevrons, window controls) are blank.
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            #[cfg(target_os = "macos")]
            let (send_key, save_key, new_key, close_key, reopen_key, find_key, side_key) = (
                "cmd-enter",
                "cmd-s",
                "cmd-t",
                "cmd-w",
                "cmd-shift-t",
                "cmd-f",
                "cmd-b",
            );
            #[cfg(not(target_os = "macos"))]
            let (send_key, save_key, new_key, close_key, reopen_key, find_key, side_key) = (
                "ctrl-enter",
                "ctrl-s",
                "ctrl-t",
                "ctrl-w",
                "ctrl-shift-t",
                "ctrl-f",
                "ctrl-b",
            );
            #[cfg(target_os = "macos")]
            let (palette_key, palette_key2) = ("cmd-k", "cmd-shift-p");
            #[cfg(not(target_os = "macos"))]
            let (palette_key, palette_key2) = ("ctrl-k", "ctrl-shift-p");
            cx.bind_keys([
                KeyBinding::new(send_key, SendRequest, None),
                KeyBinding::new(save_key, SaveRequest, None),
                KeyBinding::new(new_key, NewTab, None),
                KeyBinding::new(close_key, CloseTab, None),
                KeyBinding::new(reopen_key, ReopenTab, None),
                KeyBinding::new(find_key, FindInResponse, None),
                // Zed's binding for the left dock.
                KeyBinding::new(side_key, ToggleSidebar, None),
                KeyBinding::new(palette_key, ToggleCommandPalette, None),
                KeyBinding::new(palette_key2, ToggleCommandPalette, None),
                KeyBinding::new("up", PalettePrev, Some("Palette")),
                KeyBinding::new("down", PaletteNext, Some("Palette")),
                KeyBinding::new("escape", PaletteDismiss, Some("Palette")),
            ]);
            // A comfortable default, but never larger than the screen: a fixed size taller than
            // a 768 px laptop display put the title bar above the top edge, out of reach.
            let (w, h) = cx
                .primary_display()
                .map(|d| {
                    let s = d.bounds().size;
                    (
                        (f32::from(s.width) * 0.92).min(1280.),
                        (f32::from(s.height) * 0.86).min(820.),
                    )
                })
                .unwrap_or((1280., 820.));
            let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // We draw the title bar ourselves, as Zed does, so it can carry the
                    // environment picker instead of being dead space.
                    titlebar: Some(TitleBar::title_bar_options()),
                    window_min_size: Some(size(px(760.), px(480.))),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| Workspace::new(handle.clone(), window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
    drop(rt);
}
