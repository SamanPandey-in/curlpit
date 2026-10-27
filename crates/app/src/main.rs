//! API Tester — Phase 2a: one window, send a request, read the response.
//! Network work runs on its own tokio threads; results come back over a channel.
use std::sync::Arc;

use at_core::lines::{index_lines, longest_line};
use at_core::{build, pretty::pretty_json, Body, Method, Request, Variables};
use at_http::{send, EngineConfig, HttpError, ResponseBody};
use gpui::{
    div, prelude::*, px, rgb, size, uniform_list, App, Application, Bounds, Context, Entity, Hsla,
    ListHorizontalSizingBehavior, Window, WindowBounds, WindowOptions,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, Root};
use tokio_util::sync::CancellationToken;

fn mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn next_method(m: Method) -> Method {
    match m {
        Method::Get => Method::Post,
        Method::Post => Method::Put,
        Method::Put => Method::Patch,
        Method::Patch => Method::Delete,
        Method::Delete => Method::Head,
        Method::Head => Method::Options,
        _ => Method::Get,
    }
}

fn has_body(m: Method) -> bool {
    matches!(
        m,
        Method::Post | Method::Put | Method::Patch | Method::Delete
    )
}

fn status_color(code: u16) -> Hsla {
    match code {
        200..=299 => c(0xa6e3a1), // green
        300..=399 => c(0xf9e2af), // amber
        _ => c(0xf38ba8),         // red
    }
}

fn human(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1048576.0)
    }
}

enum Status {
    Idle,
    Loading,
    Done {
        code: u16,
        text: String,
        ms: u128,
        size: u64,
    },
    Error(String),
}

struct ViewData {
    data: Arc<Vec<u8>>,
    idx: Arc<Vec<u32>>,
    /// Index of the longest line; sizes the horizontal scroll area.
    longest: usize,
}

struct Workspace {
    rt: tokio::runtime::Handle,
    method: Method,
    url: Entity<InputState>,
    body: Entity<InputState>,
    status: Status,
    view: Option<ViewData>,
    cancel: Option<CancellationToken>,
}

impl Workspace {
    fn new(rt: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url = cx.new(|cx| InputState::new(window, cx).default_value("https://httpbin.org/get"));
        let body = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(true)
                .default_value("{\n  \"hello\": \"world\"\n}")
        });
        Self {
            rt,
            method: Method::Get,
            url,
            body,
            status: Status::Idle,
            view: None,
            cancel: None,
        }
    }

    fn set_view(&mut self, data: Vec<u8>) {
        let idx = index_lines(&data);
        let longest = longest_line(&data, &idx);
        self.view = Some(ViewData {
            data: Arc::new(data),
            idx: Arc::new(idx),
            longest,
        });
    }

    /// Send, or cancel if a request is already in flight.
    fn on_send(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
            return;
        }
        let mut url = self.url.read(cx).value().to_string().trim().to_string();
        if !url.contains("://") && !url.is_empty() {
            url = format!("https://{url}");
        }
        let mut req = Request {
            method: self.method,
            url,
            ..Default::default()
        };
        if has_body(self.method) {
            let text = self.body.read(cx).value().to_string();
            if !text.trim().is_empty() {
                req.body = Body::Json { raw: text };
            }
        }
        let built = match build(&req, &Variables::default()) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Error(e.to_string());
                cx.notify();
                return;
            }
        };

        let token = CancellationToken::new();
        self.cancel = Some(token.clone());
        self.status = Status::Loading;
        cx.notify();

        let (tx, rx) = async_channel::bounded(1);
        let settings = req.settings.clone();
        self.rt.spawn(async move {
            let r = send(&built, &settings, &EngineConfig::default(), &token).await;
            let _ = tx.send(r).await;
        });

        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.recv().await {
                this.update(cx, |this, cx| {
                    this.cancel = None;
                    this.apply(result);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn apply(&mut self, result: Result<at_http::HttpResponse, HttpError>) {
        match result {
            Ok(resp) => {
                let ct = resp
                    .headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                match resp.body {
                    ResponseBody::Memory(b) => {
                        let first = b.iter().find(|ch| !ch.is_ascii_whitespace()).copied();
                        let looks_json =
                            ct.contains("json") || matches!(first, Some(b'{') | Some(b'['));
                        let data = if looks_json {
                            pretty_json(&b, 2)
                        } else {
                            b.to_vec()
                        };
                        self.set_view(data);
                    }
                    ResponseBody::File { path, len } => {
                        let msg = format!(
                            "The response body is {} and was saved to:\n{}\n\n(The viewer for large spilled bodies arrives in the next slice.)",
                            human(len),
                            path.display()
                        );
                        self.set_view(msg.into_bytes());
                    }
                }
                self.status = Status::Done {
                    code: resp.status,
                    text: resp.status_text,
                    ms: resp.elapsed.as_millis(),
                    size: resp.size,
                };
            }
            Err(HttpError::Cancelled) => self.status = Status::Error("Cancelled".into()),
            Err(e) => self.status = Status::Error(e.to_string()),
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, muted) = (theme.background, theme.foreground, theme.muted_foreground);
        let in_flight = self.cancel.is_some();

        let top = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .p_2()
            .child(
                Button::new("method")
                    .label(self.method.as_str())
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.method = next_method(this.method);
                        cx.notify();
                    })),
            )
            .child(div().flex_1().child(Input::new(&self.url)))
            .child(
                Button::new("send")
                    .primary()
                    .label(if in_flight { "Cancel" } else { "Send" })
                    .on_click(cx.listener(|this, _ev, _window, cx| this.on_send(cx))),
            );

        let body_editor = has_body(self.method).then(|| {
            div()
                .h(px(160.))
                .px_2()
                .child(Input::new(&self.body).h_full())
        });

        let (status_text, status_col): (String, Hsla) = match &self.status {
            Status::Idle => ("Ready".into(), muted),
            Status::Loading => ("Sending…".into(), muted),
            Status::Done {
                code,
                text,
                ms,
                size,
            } => (
                format!("{code} {text}   ·   {ms} ms   ·   {}", human(*size)),
                status_color(*code),
            ),
            Status::Error(e) => (e.clone(), c(0xf38ba8)),
        };

        let viewer = match &self.view {
            Some(v) => {
                let data = v.data.clone();
                let idx = v.idx.clone();
                let longest = v.longest;
                let n = idx.len();
                uniform_list("response", n, move |range, _window, _cx| {
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
                                        .text_color(muted)
                                        .child(format!("{}", ix + 1)),
                                )
                                .child(div().whitespace_nowrap().child(text)),
                        );
                    }
                    items
                })
                .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                .with_width_from_item(Some(longest))
                .h_full()
                .into_any_element()
            }
            None => div()
                .p_4()
                .text_color(muted)
                .child("Send a request to see the response")
                .into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .child(top)
            .children(body_editor)
            .child(
                div()
                    .px_3()
                    .py_1()
                    .text_color(status_col)
                    .child(status_text),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .font_family(mono())
                    .text_size(px(13.))
                    .child(viewer),
            )
    }
}

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
