//! Spike A (UI half): a virtualized GPUI viewer over a big JSON text held as raw bytes + line index.
//! Only the visible lines are turned into elements. Scroll it hard and watch RSS in the terminal.
//! Usage: spike-viewer-gpui [size_mb=10]
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, rgb, size, uniform_list, App, Application, Bounds, Context, Window,
    WindowBounds, WindowOptions,
};

static START: OnceLock<Instant> = OnceLock::new();
static FIRST: AtomicBool = AtomicBool::new(false);

fn mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
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

fn generate(target: usize) -> Vec<u8> {
    let mut s: Vec<u8> = Vec::with_capacity(target + 1024);
    s.extend_from_slice(b"[\n");
    let mut i: u64 = 0;
    while s.len() < target {
        if i > 0 {
            s.extend_from_slice(b",\n");
        }
        write!(
            s,
            "  {{\n    \"id\": {i},\n    \"name\": \"user-{i}\",\n    \"email\": \"user{i}@example.com\",\n    \"active\": {},\n    \"score\": {}.{},\n    \"tags\": [\"alpha\", \"beta\", \"needle-{}\"],\n    \"address\": null\n  }}",
            i % 2 == 0,
            i % 100,
            i % 10,
            i % 1000
        )
        .unwrap();
        i += 1;
    }
    s.extend_from_slice(b"\n]\n");
    s
}

fn index_lines(b: &[u8]) -> Vec<u32> {
    let mut v = Vec::with_capacity(b.len() / 30);
    v.push(0u32);
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' && i + 1 < b.len() {
            v.push((i + 1) as u32);
        }
    }
    v
}

struct Viewer {
    data: Arc<Vec<u8>>,
    idx: Arc<Vec<u32>>,
}

impl Render for Viewer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if !FIRST.swap(true, Ordering::Relaxed) {
            if let Some(s) = START.get() {
                println!("first render: {:?} after main() (includes generating + indexing the data)", s.elapsed());
            }
        }
        let count = self.idx.len();
        let data = self.data.clone();
        let idx = self.idx.clone();
        div()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .font_family(mono())
            .text_size(px(13.))
            .child(
                uniform_list("lines", count, move |range, _window, _cx| {
                    let mut items = Vec::new();
                    for ix in range {
                        let start = idx[ix] as usize;
                        let end = idx.get(ix + 1).map(|&e| e as usize).unwrap_or(data.len());
                        let text = String::from_utf8_lossy(&data[start..end])
                            .trim_end()
                            .to_string();
                        items.push(
                            div()
                                .flex()
                                .flex_row()
                                .h(px(18.))
                                .child(
                                    div()
                                        .w(px(76.))
                                        .px_2()
                                        .text_color(rgb(0x6c7086))
                                        .child(format!("{}", ix + 1)),
                                )
                                .child(div().child(text)),
                        );
                    }
                    items
                })
                .h_full(),
            )
    }
}

fn main() {
    START.set(Instant::now()).ok();
    let mb: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let data = generate(mb * 1024 * 1024);
    let idx = index_lines(&data);
    println!("{} lines, {:.1} MB of text. RSS before window: {:.1} MB", idx.len(), data.len() as f64 / 1048576.0, rss_mb());
    let data = Arc::new(data);
    let idx = Arc::new(idx);

    std::thread::spawn(|| {
        println!("SCROLL HARD NOW (trackpad flick, drag scrollbar, hold arrow keys). RSS is printed every 5 s.");
        for _ in 0..8 {
            std::thread::sleep(Duration::from_secs(5));
            println!("RSS: {:.1} MB", rss_mb());
        }
    });

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| cx.new(|_| Viewer { data: data.clone(), idx: idx.clone() }),
        )
        .unwrap();
        cx.activate(true);
    });
}
