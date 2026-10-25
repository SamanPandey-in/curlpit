//! Spike C: can gpui-component's code editor serve as the request-body editor?
//! (multi-line, line numbers, JSON highlighting, typing latency, big paste, undo)
//! Usage: spike-editor-gpui [size_kb=3]
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, size, App, Application, Bounds, Context, Entity, Window, WindowBounds,
    WindowOptions,
};
use gpui_component::input::{Input, InputState};
use gpui_component::Root;

static START: OnceLock<Instant> = OnceLock::new();
static FIRST: AtomicBool = AtomicBool::new(false);

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

fn sample_json(target_bytes: usize) -> String {
    let mut s: Vec<u8> = Vec::new();
    s.extend_from_slice(b"{\n  \"url\": \"{{base_url}}/users\",\n  \"items\": [\n");
    let mut i = 0u64;
    loop {
        if i > 0 {
            s.extend_from_slice(b",\n");
        }
        write!(
            s,
            "    {{ \"id\": {i}, \"name\": \"user-{i}\", \"email\": \"user{i}@example.com\", \"active\": {}, \"tags\": [\"a\", \"b\"] }}",
            i % 2 == 0
        )
        .unwrap();
        i += 1;
        if s.len() >= target_bytes {
            break;
        }
    }
    s.extend_from_slice(b"\n  ]\n}\n");
    String::from_utf8(s).unwrap()
}

struct EditorSpike {
    state: Entity<InputState>,
}

impl EditorSpike {
    fn new(text: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(true)
                .default_value(text)
        });
        Self { state }
    }
}

impl Render for EditorSpike {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if !FIRST.swap(true, Ordering::Relaxed) {
            if let Some(s) = START.get() {
                println!("first render: {:?} after main()", s.elapsed());
            }
        }
        div().size_full().p_2().child(Input::new(&self.state).h_full())
    }
}

fn main() {
    START.set(Instant::now()).ok();
    let kb: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let text = sample_json(kb * 1024);
    println!("document: {:.1} KB, {} lines", text.len() as f64 / 1024.0, text.lines().count());

    std::thread::spawn(|| {
        println!("TYPE / PASTE / UNDO NOW (see the checklist). RSS is printed every 5 s.");
        for _ in 0..8 {
            std::thread::sleep(Duration::from_secs(5));
            println!("RSS: {:.1} MB", rss_mb());
        }
    });

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| EditorSpike::new(text, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
