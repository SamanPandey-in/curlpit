//! API Tester — one window, request tabs, send a request, read the response.
//! Network work runs on its own tokio threads; results come back over a channel.
//!
//! The window and its state live in `workspace`; this file only starts the runtime,
//! binds the keys and opens the window. Each module below owns one area: `editor`,
//! `response` and `tree` the two halves of a request tab, `theme` the palette every
//! view paints from, `format` and `ui` the shared pure helpers, `script`, `runner`
//! and `sidebar` the state of the features named.

mod actions;
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

use crate::actions::{CloseTab, FindInResponse, NewTab, ReopenTab, SaveRequest, SendRequest};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{px, size, App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions};
use gpui_component::Root;

fn main() {
    // Networking lives on its own small thread pool; the UI thread never blocks on it.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to start the network runtime");
    let handle = rt.handle().clone();

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        #[cfg(target_os = "macos")]
        let (send_key, save_key, new_key, close_key, reopen_key, find_key) = (
            "cmd-enter",
            "cmd-s",
            "cmd-t",
            "cmd-w",
            "cmd-shift-t",
            "cmd-f",
        );
        #[cfg(not(target_os = "macos"))]
        let (send_key, save_key, new_key, close_key, reopen_key, find_key) = (
            "ctrl-enter",
            "ctrl-s",
            "ctrl-t",
            "ctrl-w",
            "ctrl-shift-t",
            "ctrl-f",
        );
        cx.bind_keys([
            KeyBinding::new(send_key, SendRequest, None),
            KeyBinding::new(save_key, SaveRequest, None),
            KeyBinding::new(new_key, NewTab, None),
            KeyBinding::new(close_key, CloseTab, None),
            KeyBinding::new(reopen_key, ReopenTab, None),
            KeyBinding::new(find_key, FindInResponse, None),
        ]);
        let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
