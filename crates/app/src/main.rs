//! API Tester — Phase 2a: one window, send a request, read the response.
//! Network work runs on its own tokio threads; results come back over a channel.
use std::path::PathBuf;
use std::sync::Arc;

use at_core::lines::{build_rows, count_chars, RowIndex, CONT, MAX_ROW_CHARS};
use at_core::{build, pretty::pretty_json, Body, KeyValue, Method, Request, Variables};
use at_http::{send, EngineConfig, HttpError, ResponseBody};
use at_store::{Node, Store};
use gpui::{
    actions, div, prelude::*, px, rgb, size, uniform_list, App, Application, Bounds, Context,
    Entity, FocusHandle, Hsla, KeyBinding, ScrollWheelEvent, Subscription, Window, WindowBounds,
    WindowOptions,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Root};
use tokio_util::sync::CancellationToken;

/// Width of the sticky line-number gutter (px).
const GUTTER: f32 = 76.0;

actions!(api_tester, [SendRequest, SaveRequest]);

fn mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

/// Advance width of one monospace character at 13 px (estimate; used for wrapping and scroll limits).
fn char_w() -> f32 {
    if cfg!(target_os = "windows") {
        7.2
    } else {
        7.83
    }
}

/// Pixels available for text in the response pane.
fn avail_width(window: &Window) -> f32 {
    (f32::from(window.viewport_size().width) - GUTTER - 12.0).max(50.0)
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

#[derive(Clone, Copy, PartialEq)]
enum ReqTab {
    Params,
    Headers,
    Body,
}

#[derive(Clone, Copy, PartialEq)]
enum ResTab {
    Body,
    Headers,
}

#[derive(Clone, Copy)]
enum Which {
    Params,
    Headers,
}

/// One editable key/value row (params or headers).
struct KvRow {
    enabled: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
}

fn collect(rows: &[KvRow], cx: &App) -> Vec<KeyValue> {
    rows.iter()
        .map(|r| KeyValue {
            key: r.key.read(cx).value().to_string(),
            value: r.value.read(cx).value().to_string(),
            description: String::new(),
            enabled: r.enabled,
        })
        .collect()
}

fn seg_btn(id: &'static str, label: String, selected: bool) -> Button {
    let b = Button::new(id).label(label);
    if selected {
        b.primary()
    } else {
        b
    }
}

#[derive(Clone)]
enum ItemKind {
    Collection,
    Folder,
    Request,
    Broken(String),
}

/// One row of the flattened sidebar tree.
#[derive(Clone)]
struct SidebarItem {
    depth: usize,
    label: String,
    kind: ItemKind,
    /// Collection directory name this item belongs to.
    col: String,
    /// Collection dir, folder dir, or request file.
    path: PathBuf,
}

fn push_nodes(items: &mut Vec<SidebarItem>, nodes: &[Node], depth: usize, col: &str) {
    for n in nodes {
        match n {
            Node::Folder {
                name,
                path,
                children,
            } => {
                items.push(SidebarItem {
                    depth,
                    label: name.clone(),
                    kind: ItemKind::Folder,
                    col: col.into(),
                    path: path.clone(),
                });
                push_nodes(items, children, depth + 1, col);
            }
            Node::Request { name, path } => items.push(SidebarItem {
                depth,
                label: name.clone(),
                kind: ItemKind::Request,
                col: col.into(),
                path: path.clone(),
            }),
            Node::Broken { name, path, error } => items.push(SidebarItem {
                depth,
                label: name.clone(),
                kind: ItemKind::Broken(error.clone()),
                col: col.into(),
                path: path.clone(),
            }),
        }
    }
}

enum Status {
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

struct ViewData {
    data: Arc<Vec<u8>>,
    /// Rows without wrapping (hard lines, plus a safety split for monstrous lines).
    base: Arc<RowIndex>,
    /// Rows wrapped to a column count; rebuilt when the window width changes.
    wrapped: Option<(usize, Arc<RowIndex>)>,
    /// Characters in the longest row; sizes the horizontal scroll range.
    longest_chars: usize,
}

struct Workspace {
    rt: tokio::runtime::Handle,
    method: Method,
    url: Entity<InputState>,
    body: Entity<InputState>,
    status: Status,
    view: Option<ViewData>,
    cancel: Option<CancellationToken>,
    params: Vec<KvRow>,
    headers: Vec<KvRow>,
    req_tab: ReqTab,
    res_tab: ResTab,
    resp_headers: Vec<(String, String)>,
    _subs: Vec<Subscription>,
    name: Entity<InputState>,
    store: Option<Store>,
    items: Vec<SidebarItem>,
    current_col: Option<String>,
    current_path: Option<PathBuf>,
    save_dir: Option<PathBuf>,
    focus: FocusHandle,
    focused_once: bool,
    wrap: bool,
    /// Horizontal scroll of the response text in px (0 = left edge). The gutter never moves.
    scroll_x: f32,
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
        let name = cx.new(|cx| InputState::new(window, cx).default_value("New request"));
        let (store, store_err) = match Store::open_default() {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let params = vec![Self::new_row(window, cx, "hello", "world")];
        let headers = vec![Self::new_row(window, cx, "", "")];
        // Enter in the URL box sends (but never cancels an in-flight request).
        let sub = cx.subscribe_in(
            &url,
            window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) && this.cancel.is_none() {
                    this.on_send(cx);
                }
            },
        );
        let mut me = Self {
            rt,
            method: Method::Get,
            url,
            body,
            status: store_err.map(Status::Error).unwrap_or(Status::Idle),
            view: None,
            cancel: None,
            params,
            headers,
            req_tab: ReqTab::Params,
            res_tab: ResTab::Body,
            resp_headers: Vec::new(),
            _subs: vec![sub],
            name,
            store,
            items: Vec::new(),
            current_col: None,
            current_path: None,
            save_dir: None,
            focus: cx.focus_handle(),
            focused_once: false,
            wrap: false,
            scroll_x: 0.0,
        };
        me.refresh_sidebar();
        me
    }

    fn new_row(window: &mut Window, cx: &mut Context<Self>, k: &str, v: &str) -> KvRow {
        let key = cx.new(|cx| InputState::new(window, cx).default_value(k.to_string()));
        let value = cx.new(|cx| InputState::new(window, cx).default_value(v.to_string()));
        KvRow {
            enabled: true,
            key,
            value,
        }
    }

    fn rows_mut(&mut self, which: Which) -> &mut Vec<KvRow> {
        match which {
            Which::Params => &mut self.params,
            Which::Headers => &mut self.headers,
        }
    }

    fn render_kv(&self, which: Which, cx: &Context<Self>) -> impl IntoElement {
        let (rows, en_id, del_id, add_id) = match which {
            Which::Params => (&self.params, "p-en", "p-del", "p-add"),
            Which::Headers => (&self.headers, "h-en", "h-del", "h-add"),
        };
        let muted = cx.theme().muted_foreground;
        let mut col = div().flex().flex_col().gap_1().p_2().child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .text_color(muted)
                .child(div().w(px(24.)).flex_none())
                .child(div().flex_1().child("Key"))
                .child(div().flex_1().child("Value"))
                .child(div().w(px(36.)).flex_none()),
        );
        for (ix, row) in rows.iter().enumerate() {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div().w(px(24.)).flex_none().child(
                            Checkbox::new((en_id, ix))
                                .checked(row.enabled)
                                .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                                    if let Some(r) = this.rows_mut(which).get_mut(ix) {
                                        r.enabled = *checked;
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(div().flex_1().child(Input::new(&row.key)))
                    .child(div().flex_1().child(Input::new(&row.value)))
                    .child(div().w(px(36.)).flex_none().child(
                        Button::new((del_id, ix)).label("✕").on_click(cx.listener(
                            move |this, _ev, _window, cx| {
                                let rows = this.rows_mut(which);
                                if ix < rows.len() {
                                    rows.remove(ix);
                                }
                                cx.notify();
                            },
                        )),
                    )),
            );
        }
        col.child(
            div().child(Button::new(add_id).label("+ Add").on_click(cx.listener(
                move |this, _ev, window, cx| {
                    let row = Self::new_row(window, cx, "", "");
                    this.rows_mut(which).push(row);
                    cx.notify();
                },
            ))),
        )
    }

    fn current_request(&self, cx: &App) -> Request {
        let mut req = Request {
            name: self.name.read(cx).value().to_string(),
            method: self.method,
            url: self.url.read(cx).value().to_string().trim().to_string(),
            ..Default::default()
        };
        req.params = collect(&self.params, cx);
        req.headers = collect(&self.headers, cx);
        let text = self.body.read(cx).value().to_string();
        if !text.trim().is_empty() {
            req.body = Body::Json { raw: text };
        }
        req
    }

    fn apply_request(&mut self, req: Request, window: &mut Window, cx: &mut Context<Self>) {
        self.method = req.method;
        let url = req.url.clone();
        self.url.update(cx, |s, cx| s.set_value(url, window, cx));
        let name = req.name.clone();
        self.name.update(cx, |s, cx| s.set_value(name, window, cx));
        let body_text = match &req.body {
            Body::Json { raw } | Body::Text { raw } | Body::Xml { raw } | Body::Html { raw } => {
                raw.clone()
            }
            _ => String::new(),
        };
        self.body
            .update(cx, |s, cx| s.set_value(body_text, window, cx));
        let mut params: Vec<KvRow> = req
            .params
            .iter()
            .map(|kv| {
                let mut r = Self::new_row(window, cx, &kv.key, &kv.value);
                r.enabled = kv.enabled;
                r
            })
            .collect();
        if params.is_empty() {
            params.push(Self::new_row(window, cx, "", ""));
        }
        let mut headers: Vec<KvRow> = req
            .headers
            .iter()
            .map(|kv| {
                let mut r = Self::new_row(window, cx, &kv.key, &kv.value);
                r.enabled = kv.enabled;
                r
            })
            .collect();
        if headers.is_empty() {
            headers.push(Self::new_row(window, cx, "", ""));
        }
        self.params = params;
        self.headers = headers;
    }

    fn refresh_sidebar(&mut self) {
        self.items.clear();
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.load_all() {
            Ok(trees) => {
                for t in &trees {
                    self.items.push(SidebarItem {
                        depth: 0,
                        label: t.name.clone(),
                        kind: ItemKind::Collection,
                        col: t.dir_name.clone(),
                        path: t.dir.clone(),
                    });
                    push_nodes(&mut self.items, &t.children, 1, &t.dir_name);
                }
            }
            Err(e) => self.status = Status::Error(e.to_string()),
        }
    }

    fn on_sidebar_click(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(ix).cloned() else {
            return;
        };
        match item.kind {
            ItemKind::Collection | ItemKind::Folder => {
                self.current_col = Some(item.col);
                self.save_dir = Some(item.path);
                self.current_path = None;
                self.status = Status::Info("New requests will be saved here".into());
            }
            ItemKind::Broken(e) => self.status = Status::Error(e),
            ItemKind::Request => match Store::load_request(&item.path) {
                Ok(req) => {
                    self.apply_request(req, window, cx);
                    self.current_col = Some(item.col);
                    self.save_dir = item.path.parent().map(|d| d.to_path_buf());
                    self.current_path = Some(item.path);
                    self.status = Status::Info("Opened".into());
                }
                Err(e) => self.status = Status::Error(e.to_string()),
            },
        }
        cx.notify();
    }

    fn on_new_collection(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let n = self
            .items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Collection))
            .count()
            + 1;
        match store.create_collection(&format!("Collection {n}")) {
            Ok(c) => {
                self.save_dir = Some(c.dir);
                self.current_col = Some(c.dir_name);
                self.current_path = None;
                self.refresh_sidebar();
                self.status = Status::Info("Collection created; Save puts requests in it".into());
            }
            Err(e) => self.status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    fn on_save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            self.status = Status::Error("Storage is unavailable on this system".into());
            cx.notify();
            return;
        };
        let mut req = self.current_request(cx);
        if req.name.trim().is_empty() {
            req.name = "Untitled request".into();
        }
        let dir = match (&self.current_path, &self.save_dir) {
            (Some(p), _) => p.parent().map(|d| d.to_path_buf()),
            (None, Some(d)) => Some(d.clone()),
            _ => None,
        };
        let dir = match dir {
            Some(d) => d,
            None => match store.load_all() {
                Ok(all) if !all.is_empty() => all[0].dir.clone(),
                _ => match store.create_collection("My Collection") {
                    Ok(c) => c.dir,
                    Err(e) => {
                        self.status = Status::Error(e.to_string());
                        cx.notify();
                        return;
                    }
                },
            },
        };
        match Store::save_request(&dir, &req, self.current_path.as_deref()) {
            Ok(path) => {
                self.save_dir = path.parent().map(|d| d.to_path_buf());
                self.status = Status::Info(format!("Saved to {}", path.display()));
                self.current_path = Some(path);
                self.refresh_sidebar();
            }
            Err(e) => self.status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    fn set_view(&mut self, data: Vec<u8>) {
        let base = build_rows(&data, MAX_ROW_CHARS);
        let longest = base.longest_row(&data);
        let longest_chars = count_chars(base.row(&data, longest));
        self.view = Some(ViewData {
            data: Arc::new(data),
            base: Arc::new(base),
            wrapped: None,
            longest_chars,
        });
        self.scroll_x = 0.0;
    }

    fn max_scroll_x(&self, avail: f32) -> f32 {
        match &self.view {
            Some(v) => (v.longest_chars as f32 * char_w() + 24.0 - avail).max(0.0),
            None => 0.0,
        }
    }

    /// Send, or cancel if a request is already in flight.
    fn on_send(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
            return;
        }
        let mut req = self.current_request(cx);
        if !req.url.contains("://") && !req.url.is_empty() {
            req.url = format!("https://{}", req.url);
        }
        if !has_body(self.method) {
            req.body = Body::None;
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
                let mut hs = resp.headers.clone();
                hs.sort_by(|a, b| a.0.cmp(&b.0));
                self.resp_headers = hs;
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
                            "The response body is {} and was saved to:\n{}\n\n(The viewer for large spilled bodies arrives in a later slice.)",
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            window.focus(&self.focus);
            self.focused_once = true;
        }
        // ---- viewer geometry (depends on the current window width) ----
        let avail = avail_width(window);
        let cols = (((avail / char_w()).floor() as usize).saturating_sub(1)).max(10);
        if self.wrap {
            if let Some(v) = self.view.as_mut() {
                let stale = match &v.wrapped {
                    Some((c, _)) => *c != cols,
                    None => true,
                };
                if stale {
                    let rows = build_rows(&v.data, cols);
                    v.wrapped = Some((cols, Arc::new(rows)));
                }
            }
        }
        let overflow = self
            .view
            .as_ref()
            .map(|v| v.longest_chars as f32 * char_w() > avail)
            .unwrap_or(false);
        let max_x = self.max_scroll_x(avail);
        if self.scroll_x > max_x {
            self.scroll_x = max_x;
        }
        let scroll_x = if self.wrap { 0.0 } else { self.scroll_x };

        let theme = cx.theme();
        let (bg, fg, muted, border, accent) = (
            theme.background,
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.accent,
        );
        let in_flight = self.cancel.is_some();

        let top = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .p_2()
            .child(div().w(px(180.)).flex_none().child(Input::new(&self.name)))
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
            )
            .child(
                Button::new("save")
                    .label("Save")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_save(window, cx))),
            );

        // ---- request editor: tabs + panel ----
        let pcount = self
            .params
            .iter()
            .filter(|r| r.enabled && !r.key.read(cx).value().is_empty())
            .count();
        let hcount = self
            .headers
            .iter()
            .filter(|r| r.enabled && !r.key.read(cx).value().is_empty())
            .count();
        let req_tabs = div()
            .flex()
            .flex_row()
            .gap_1()
            .px_2()
            .child(
                seg_btn(
                    "t-params",
                    format!("Params ({pcount})"),
                    self.req_tab == ReqTab::Params,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.req_tab = ReqTab::Params;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-headers",
                    format!("Headers ({hcount})"),
                    self.req_tab == ReqTab::Headers,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.req_tab = ReqTab::Headers;
                    cx.notify();
                })),
            )
            .child(
                seg_btn("t-body", "Body".to_string(), self.req_tab == ReqTab::Body).on_click(
                    cx.listener(|this, _ev, _window, cx| {
                        this.req_tab = ReqTab::Body;
                        cx.notify();
                    }),
                ),
            );
        let req_panel =
            div()
                .id("req-panel")
                .h(px(210.))
                .overflow_y_scroll()
                .child(match self.req_tab {
                    ReqTab::Params => self.render_kv(Which::Params, cx).into_any_element(),
                    ReqTab::Headers => self.render_kv(Which::Headers, cx).into_any_element(),
                    ReqTab::Body => div()
                        .p_2()
                        .h_full()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children((!has_body(self.method)).then(|| {
                            div().text_color(muted).child(format!(
                                "{} requests don't send a body.",
                                self.method.as_str()
                            ))
                        }))
                        .child(div().flex_1().child(Input::new(&self.body).h_full()))
                        .into_any_element(),
                });

        let (status_text, status_col): (String, Hsla) = match &self.status {
            Status::Idle => ("Ready".into(), muted),
            Status::Loading => ("Sending…".into(), muted),
            Status::Info(m) => (m.clone(), muted),
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

        // The wrap toggle appears only when text overflows the window (or while wrap is on).
        let wrap_btn = (self.res_tab == ResTab::Body && (self.wrap || overflow)).then(|| {
            Button::new("wrap")
                .label(if self.wrap { "Wrap: on" } else { "Wrap: off" })
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.wrap = !this.wrap;
                    this.scroll_x = 0.0;
                    if !this.wrap {
                        if let Some(v) = this.view.as_mut() {
                            v.wrapped = None;
                        }
                    }
                    cx.notify();
                }))
        });

        let status_row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_3()
            .py_1()
            .child(div().text_color(status_col).child(status_text))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(
                        seg_btn("r-body", "Body".to_string(), self.res_tab == ResTab::Body)
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.res_tab = ResTab::Body;
                                cx.notify();
                            })),
                    )
                    .child(
                        seg_btn(
                            "r-headers",
                            format!("Headers ({})", self.resp_headers.len()),
                            self.res_tab == ResTab::Headers,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            this.res_tab = ResTab::Headers;
                            cx.notify();
                        })),
                    )
                    .children(wrap_btn),
            );

        let viewer = match &self.view {
            Some(v) => {
                let rows = if self.wrap {
                    v.wrapped
                        .as_ref()
                        .map(|(_, r)| r.clone())
                        .unwrap_or_else(|| v.base.clone())
                } else {
                    v.base.clone()
                };
                let data = v.data.clone();
                let n = rows.len();
                uniform_list("response", n, move |range, _window, _cx| {
                    let mut items = Vec::new();
                    for ix in range {
                        let mut text = String::from_utf8_lossy(rows.row(&data, ix))
                            .trim_end()
                            .to_string();
                        if text.contains('\t') {
                            text = text.replace('\t', "    ");
                        }
                        // Continuation rows of a wrapped line get an empty gutter, so numbers
                        // stay attached to the first row of their line.
                        let label = if rows.lines[ix] & CONT != 0 {
                            String::new()
                        } else {
                            rows.line_no(ix).to_string()
                        };
                        items.push(
                            div()
                                .flex()
                                .flex_row()
                                .h(px(18.))
                                // sticky gutter: lives outside the horizontally shifted text
                                .child(
                                    div()
                                        .w(px(GUTTER))
                                        .flex_none()
                                        .px_2()
                                        .text_color(muted)
                                        .border_r_1()
                                        .border_color(border)
                                        .child(label),
                                )
                                .child(
                                    div().flex_1().overflow_hidden().child(
                                        div()
                                            .relative()
                                            .left(px(-scroll_x))
                                            .flex_none()
                                            .whitespace_nowrap()
                                            .child(text),
                                    ),
                                ),
                        );
                    }
                    items
                })
                .h_full()
                .into_any_element()
            }
            None => div()
                .p_4()
                .text_color(muted)
                .child("Send a request to see the response")
                .into_any_element(),
        };

        let resp_area = match self.res_tab {
            ResTab::Body => viewer,
            ResTab::Headers => div()
                .id("resp-headers")
                .size_full()
                .overflow_y_scroll()
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .children(self.resp_headers.iter().map(|(k, v)| {
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(
                            div()
                                .w(px(240.))
                                .flex_none()
                                .text_color(muted)
                                .child(k.clone()),
                        )
                        .child(div().flex_1().child(v.clone()))
                }))
                .into_any_element(),
        };

        let mut side_list = div()
            .id("sidebar-list")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if self.items.is_empty() {
            side_list = side_list.child(
                div()
                    .p_2()
                    .text_color(muted)
                    .child("No collections yet. Press + or just Save a request."),
            );
        }
        for (ix, item) in self.items.iter().enumerate() {
            let selected = match item.kind {
                ItemKind::Request => self.current_path.as_ref() == Some(&item.path),
                ItemKind::Collection | ItemKind::Folder => {
                    self.current_path.is_none() && self.save_dir.as_ref() == Some(&item.path)
                }
                ItemKind::Broken(_) => false,
            };
            let (prefix, color) = match item.kind {
                ItemKind::Collection => ("▾ ", fg),
                ItemKind::Folder => ("▸ ", fg),
                ItemKind::Request => ("", fg),
                ItemKind::Broken(_) => ("⚠ ", c(0xf38ba8)),
            };
            side_list = side_list.child(
                div()
                    .id(("side", ix))
                    .w_full()
                    .py_1()
                    .pl(px(8.0 + item.depth as f32 * 14.0))
                    .pr_2()
                    .text_color(color)
                    .cursor_pointer()
                    .bg(if selected { accent } else { bg })
                    .child(format!("{prefix}{}", item.label))
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.on_sidebar_click(ix, window, cx)
                    })),
            );
        }
        let sidebar = div()
            .w(px(260.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .p_2()
                    .child(div().text_color(muted).child("COLLECTIONS"))
                    .child(Button::new("new-col").label("+").on_click(
                        cx.listener(|this, _ev, _window, cx| this.on_new_collection(cx)),
                    )),
            )
            .child(side_list);

        let main_col = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(top)
            .child(req_tabs)
            .child(req_panel)
            .child(status_row)
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .font_family(mono())
                    .text_size(px(13.))
                    // horizontal scrolling is ours (so the gutter can stay put); vertical is the list's
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, window, cx| {
                        if this.wrap || this.res_tab != ResTab::Body {
                            return;
                        }
                        let d = ev.delta.pixel_delta(px(18.));
                        let mut dx = f32::from(d.x);
                        if dx == 0.0 && ev.modifiers.shift {
                            dx = f32::from(d.y);
                        }
                        if dx == 0.0 {
                            return;
                        }
                        let max = this.max_scroll_x(avail_width(window));
                        this.scroll_x = (this.scroll_x - dx).clamp(0.0, max);
                        cx.notify();
                    }))
                    .child(resp_area),
            );

        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(bg)
            .text_color(fg)
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SendRequest, _window, cx| this.on_send(cx)))
            .on_action(cx.listener(|this, _: &SaveRequest, window, cx| this.on_save(window, cx)))
            .child(sidebar)
            .child(main_col)
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
        #[cfg(target_os = "macos")]
        let (send_key, save_key) = ("cmd-enter", "cmd-s");
        #[cfg(not(target_os = "macos"))]
        let (send_key, save_key) = ("ctrl-enter", "ctrl-s");
        cx.bind_keys([
            KeyBinding::new(send_key, SendRequest, None),
            KeyBinding::new(save_key, SaveRequest, None),
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
