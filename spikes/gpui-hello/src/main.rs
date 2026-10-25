//! Spike: empty GPUI window. Measures cold start (main -> first render), idle RSS, binary size.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, SharedString, Window,
    WindowBounds, WindowOptions,
};

static START: OnceLock<Instant> = OnceLock::new();
static FIRST: AtomicBool = AtomicBool::new(false);

struct Hello {
    text: SharedString,
}

impl Render for Hello {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if !FIRST.swap(true, Ordering::Relaxed) {
            if let Some(s) = START.get() {
                println!("first render: {:?} after main()", s.elapsed());
            }
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(0x1e1e2e))
            .size_full()
            .justify_center()
            .items_center()
            .text_xl()
            .text_color(rgb(0xffffff))
            .child(format!("Hello, {}!", &self.text))
    }
}

fn rss_mb() -> f64 {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .map(|kb| kb / 1024.0)
        .unwrap_or(-1.0)
}

fn main() {
    START.set(Instant::now()).ok();
    // Report idle memory a few seconds after launch (-1 on Windows: no `ps`; use Task Manager there).
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(4));
        println!("idle RSS after 4s: {:.1} MB", rss_mb());
    });
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.), px(600.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Hello { text: "API Tester".into() }),
        )
        .unwrap();
        cx.activate(true);
    });
}
