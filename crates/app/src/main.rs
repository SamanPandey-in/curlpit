//! API Tester — one window, request tabs, send a request, read the response.
//! Network work runs on its own tokio threads; results come back over a channel.
use std::path::PathBuf;
use std::sync::Arc;

use at_core::lines::{build_rows, count_chars, RowIndex, CONT, MAX_ROW_CHARS};
use at_core::{
    build, pretty::pretty_json, to_map, Body, EnvVar, Environment, KeyValue, Method, Request,
    Variables,
};
use at_http::{send, EngineConfig, HttpError, ResponseBody};
use at_store::{Node, Settings, Store};
use gpui::{
    actions, div, prelude::*, px, rgb, size, uniform_list, App, Application, Bounds, ClipboardItem,
    Context, Entity, FocusHandle, Hsla, KeyBinding, ScrollWheelEvent, Subscription, Window,
    WindowBounds, WindowOptions,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Root};
use tokio_util::sync::CancellationToken;

/// Width of the sticky line-number gutter (px).
const GUTTER: f32 = 76.0;

actions!(api_tester, [SendRequest, SaveRequest, NewTab, CloseTab]);

/// Longest tab title, in characters; longer names are cut with an ellipsis.
const TAB_TITLE_MAX: usize = 24;

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

/// Tab button text for a request name: trimmed, "Untitled" when empty, cut with an ellipsis.
fn tab_title(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "Untitled".into();
    }
    if name.chars().count() > TAB_TITLE_MAX {
        let cut: String = name.chars().take(TAB_TITLE_MAX - 1).collect();
        format!("{cut}…")
    } else {
        name.to_string()
    }
}

/// Which tab is active after `closed` is removed, leaving `remaining` tabs: the same tab if it
/// is still there, else its right neighbour, else the last one.
fn active_after_close(active: usize, closed: usize, remaining: usize) -> usize {
    if remaining == 0 {
        0
    } else if closed < active {
        active - 1
    } else {
        active.min(remaining - 1)
    }
}

/// One editable key/value row (params or headers).
struct KvRow {
    enabled: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
}

/// Rows -> key/values. Completely empty rows (the spare row at the bottom) are skipped so they
/// never get written into saved files.
fn collect(rows: &[KvRow], cx: &App) -> Vec<KeyValue> {
    rows.iter()
        .filter_map(|r| {
            let key = r.key.read(cx).value().to_string();
            let value = r.value.read(cx).value().to_string();
            if key.trim().is_empty() && value.is_empty() {
                return None;
            }
            Some(KeyValue {
                key,
                value,
                description: String::new(),
                enabled: r.enabled,
            })
        })
        .collect()
}

/// One editable environment/global variable row.
struct EnvRow {
    enabled: bool,
    secret: bool,
    /// Secret values are masked unless the user clicked Show.
    revealed: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
}

/// Variables from the editor rows; rows without a key are skipped.
fn collect_env(rows: &[EnvRow], cx: &App) -> Vec<EnvVar> {
    rows.iter()
        .filter_map(|r| {
            let key = r.key.read(cx).value().trim().to_string();
            if key.is_empty() {
                return None;
            }
            Some(EnvVar {
                key,
                value: r.value.read(cx).value().to_string(),
                secret: r.secret,
                enabled: r.enabled,
            })
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum MainView {
    Request,
    Environments,
}

/// What the environments page is editing.
#[derive(Clone, Copy, PartialEq)]
enum Scope {
    Globals,
    Env(usize),
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
    /// Collection dir, folder dir, or request file.
    path: PathBuf,
}

fn push_nodes(items: &mut Vec<SidebarItem>, nodes: &[Node], depth: usize) {
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
                    path: path.clone(),
                });
                push_nodes(items, children, depth + 1);
            }
            Node::Request { name, path } => items.push(SidebarItem {
                depth,
                label: name.clone(),
                kind: ItemKind::Request,
                path: path.clone(),
            }),
            Node::Broken { name, path, error } => items.push(SidebarItem {
                depth,
                label: name.clone(),
                kind: ItemKind::Broken(error.clone()),
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

/// Everything that belongs to one open request: its editor, where it is saved, and the last
/// response. Tabs are independent, so a request can be in flight in one while you edit another.
struct RequestTab {
    /// Stable identity (indices shift when tabs close); in-flight results find their tab by it.
    id: u64,
    name: Entity<InputState>,
    method: Method,
    url: Entity<InputState>,
    body: Entity<InputState>,
    params: Vec<KvRow>,
    headers: Vec<KvRow>,
    req_tab: ReqTab,
    /// The file this tab is saved to, if any.
    current_path: Option<PathBuf>,
    /// Folder the next Save goes to when the tab has no file yet.
    save_dir: Option<PathBuf>,
    /// The editor contents as of the last open/save; the tab is "unsaved" when they differ.
    saved: Request,
    /// The close button was clicked once on an unsaved tab; a second click discards it.
    confirm_close: bool,
    status: Status,
    view: Option<ViewData>,
    cancel: Option<CancellationToken>,
    res_tab: ResTab,
    resp_headers: Vec<(String, String)>,
    wrap: bool,
    /// Horizontal scroll of the response text in px (0 = left edge). The gutter never moves.
    scroll_x: f32,
    /// Shown next to the status, e.g. unresolved `{{variables}}` in the last request.
    warning: Option<String>,
    copied: bool,
    _sub: Subscription,
}

impl RequestTab {
    /// A new tab. `blank` gives an empty request; otherwise the httpbin starter example.
    fn create(
        id: u64,
        blank: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> RequestTab {
        let (url_text, body_text, params) = if blank {
            ("", "", vec![Workspace::new_row(window, cx, "", "")])
        } else {
            (
                "https://httpbin.org/get",
                "{\n  \"hello\": \"world\"\n}",
                vec![Workspace::new_row(window, cx, "hello", "world")],
            )
        };
        let url = cx.new(|cx| InputState::new(window, cx).default_value(url_text));
        let body = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(true)
                .default_value(body_text)
        });
        let name = cx.new(|cx| InputState::new(window, cx).default_value("New request"));
        let headers = vec![Workspace::new_row(window, cx, "", "")];
        // Enter in the URL box sends (but never cancels an in-flight request).
        let sub = cx.subscribe_in(
            &url,
            window,
            move |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && this.tabs.iter().any(|t| t.id == id && t.cancel.is_none())
                {
                    this.send_tab(id, cx);
                }
            },
        );
        let mut tab = RequestTab {
            id,
            name,
            method: Method::Get,
            url,
            body,
            params,
            headers,
            req_tab: ReqTab::Params,
            current_path: None,
            save_dir: None,
            saved: Request::default(),
            confirm_close: false,
            status: Status::Idle,
            view: None,
            cancel: None,
            res_tab: ResTab::Body,
            resp_headers: Vec::new(),
            wrap: false,
            scroll_x: 0.0,
            warning: None,
            copied: false,
            _sub: sub,
        };
        tab.saved = tab.current_request(cx);
        tab
    }

    fn rows_mut(&mut self, which: Which) -> &mut Vec<KvRow> {
        match which {
            Which::Params => &mut self.params,
            Which::Headers => &mut self.headers,
        }
    }

    /// The request as currently typed.
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

    fn is_dirty(&self, cx: &App) -> bool {
        self.current_request(cx) != self.saved
    }

    /// Never edited, saved or sent: safe to reuse for opening a request from the sidebar.
    fn is_pristine(&self, cx: &App) -> bool {
        self.current_path.is_none() && self.view.is_none() && !self.is_dirty(cx)
    }

    /// Text for the tab button (without the unsaved marker).
    fn title(&self, cx: &App) -> String {
        tab_title(self.name.read(cx).value().as_ref())
    }

    /// Put a loaded request into the editor and remember it as the saved state.
    fn apply_request(&mut self, req: Request, window: &mut Window, cx: &mut Context<Workspace>) {
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
                let mut r = Workspace::new_row(window, cx, &kv.key, &kv.value);
                r.enabled = kv.enabled;
                r
            })
            .collect();
        if params.is_empty() {
            params.push(Workspace::new_row(window, cx, "", ""));
        }
        let mut headers: Vec<KvRow> = req
            .headers
            .iter()
            .map(|kv| {
                let mut r = Workspace::new_row(window, cx, &kv.key, &kv.value);
                r.enabled = kv.enabled;
                r
            })
            .collect();
        if headers.is_empty() {
            headers.push(Workspace::new_row(window, cx, "", ""));
        }
        self.params = params;
        self.headers = headers;
        self.saved = self.current_request(cx);
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

struct Workspace {
    rt: tokio::runtime::Handle,
    tabs: Vec<RequestTab>,
    active: usize,
    next_tab_id: u64,
    /// Messages for the environments page (and startup storage errors); each tab has its own
    /// status for request messages.
    status: Status,
    store: Option<Store>,
    items: Vec<SidebarItem>,
    focus: FocusHandle,
    focused_once: bool,
    // ---- environments (Phase 3b) ----
    main_view: MainView,
    globals: Vec<EnvVar>,
    /// Environments with the file each one lives in.
    envs: Vec<(Environment, PathBuf)>,
    active_env: Option<usize>,
    env_scope: Scope,
    env_name: Entity<InputState>,
    env_rows: Vec<EnvRow>,
    confirm_delete: bool,
}

impl Workspace {
    fn new(rt: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let env_name = cx.new(|cx| InputState::new(window, cx));
        let (store, store_err) = match Store::open_default() {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let mut first = RequestTab::create(0, false, window, cx);
        if let Some(e) = &store_err {
            first.status = Status::Error(e.clone());
        }
        let mut me = Self {
            rt,
            tabs: vec![first],
            active: 0,
            next_tab_id: 1,
            status: store_err.map(Status::Error).unwrap_or(Status::Idle),
            store,
            items: Vec::new(),
            focus: cx.focus_handle(),
            focused_once: false,
            main_view: MainView::Request,
            globals: Vec::new(),
            envs: Vec::new(),
            active_env: None,
            env_scope: Scope::Globals,
            env_name,
            env_rows: Vec::new(),
            confirm_delete: false,
        };
        me.refresh_sidebar();
        me.reload_envs();
        me.load_scope(Scope::Globals, window, cx);
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

    // ---------------- request tabs ----------------

    fn tab(&self) -> &RequestTab {
        &self.tabs[self.active]
    }

    fn tab_mut(&mut self) -> &mut RequestTab {
        &mut self.tabs[self.active]
    }

    /// Keep keyboard shortcuts working after a button click moved focus away.
    fn refocus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = RequestTab::create(id, true, window, cx);
        // New requests go to the folder the current tab was using.
        let dir = self.tab().save_dir.clone();
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.tab_mut().save_dir = dir;
        self.refocus(window);
        cx.notify();
    }

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        for (i, t) in self.tabs.iter_mut().enumerate() {
            if i != ix {
                t.confirm_close = false;
            }
        }
        self.active = ix;
        self.refocus(window);
        cx.notify();
    }

    /// Close a tab. An unsaved tab needs a second click (or Cmd+W again) to discard it.
    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        if self.tabs[ix].is_dirty(cx) && !self.tabs[ix].confirm_close {
            self.tabs[ix].confirm_close = true;
            self.tabs[ix].status = Status::Info(
                "Unsaved changes: Save first, or click the tab's Discard? to close it anyway"
                    .into(),
            );
            cx.notify();
            return;
        }
        if let Some(token) = self.tabs[ix].cancel.take() {
            token.cancel();
        }
        self.tabs.remove(ix);
        if self.tabs.is_empty() {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            self.tabs.push(RequestTab::create(id, true, window, cx));
        }
        self.active = active_after_close(self.active, ix, self.tabs.len());
        self.refocus(window);
        cx.notify();
    }

    // ---------------- environments (FR-ENV-01..05) ----------------

    /// Variables in effect: globals, overridden by the active environment.
    fn variables(&self) -> Variables {
        let environment = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| to_map(&e.variables))
            .unwrap_or_default();
        Variables {
            global: to_map(&self.globals),
            environment,
            ..Default::default()
        }
    }

    fn reload_envs(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.load_globals() {
            Ok(g) => self.globals = g,
            Err(e) => self.status = Status::Error(format!("Global variables not loaded: {e}")),
        }
        self.envs = store
            .list_environments()
            .into_iter()
            .map(|e| (e.env, e.path))
            .collect();
        let want = store.load_settings().active_environment;
        self.active_env = want.and_then(|n| self.envs.iter().position(|(e, _)| e.name == n));
    }

    fn persist_active(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        let settings = Settings {
            active_environment: self
                .active_env
                .and_then(|i| self.envs.get(i))
                .map(|(e, _)| e.name.clone()),
            ..Default::default()
        };
        if let Err(e) = store.save_settings(&settings) {
            self.status = Status::Error(e.to_string());
        }
    }

    /// Top-bar picker: none -> first environment -> ... -> none.
    fn cycle_env(&mut self, cx: &mut Context<Self>) {
        self.active_env = match self.active_env {
            None if !self.envs.is_empty() => Some(0),
            Some(i) if i + 1 < self.envs.len() => Some(i + 1),
            _ => None,
        };
        self.persist_active();
        cx.notify();
    }

    fn new_env_row(window: &mut Window, cx: &mut Context<Self>, v: &EnvVar) -> EnvRow {
        let key = cx.new(|cx| InputState::new(window, cx).default_value(v.key.clone()));
        let secret = v.secret;
        let value = cx.new(|cx| {
            let s = InputState::new(window, cx).default_value(v.value.clone());
            if secret {
                s.masked(true)
            } else {
                s
            }
        });
        EnvRow {
            enabled: v.enabled,
            secret,
            revealed: false,
            key,
            value,
        }
    }

    /// Fill the editor with the saved variables of `scope` (plus one spare empty row).
    fn load_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let (name, vars) = match scope {
            Scope::Globals => (String::new(), self.globals.clone()),
            Scope::Env(ix) => match self.envs.get(ix) {
                Some((e, _)) => (e.name.clone(), e.variables.clone()),
                None => (String::new(), Vec::new()),
            },
        };
        let mut rows: Vec<EnvRow> = vars
            .iter()
            .map(|v| Self::new_env_row(window, cx, v))
            .collect();
        rows.push(Self::new_env_row(window, cx, &EnvVar::new("", "")));
        self.env_rows = rows;
        self.env_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.env_scope = scope;
        self.confirm_delete = false;
    }

    fn env_has_changes(&self, cx: &App) -> bool {
        let vars = collect_env(&self.env_rows, cx);
        match self.env_scope {
            Scope::Globals => vars != self.globals,
            Scope::Env(ix) => match self.envs.get(ix) {
                Some((e, _)) => {
                    vars != e.variables || self.env_name.read(cx).value().trim() != e.name
                }
                None => false,
            },
        }
    }

    /// Save what the editor shows. Returns false (with the reason in the status) on failure.
    fn save_env_scope(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(store) = self.store.clone() else {
            self.status = Status::Error("Storage is unavailable on this system".into());
            return false;
        };
        let vars = collect_env(&self.env_rows, cx);
        match self.env_scope {
            Scope::Globals => match store.save_globals(&vars) {
                Ok(()) => {
                    self.globals = vars;
                    self.status = Status::Info("Global variables saved".into());
                    true
                }
                Err(e) => {
                    self.status = Status::Error(e.to_string());
                    false
                }
            },
            Scope::Env(ix) => {
                let name = self.env_name.read(cx).value().trim().to_string();
                if name.is_empty() {
                    self.status = Status::Error("Give the environment a name".into());
                    return false;
                }
                if self
                    .envs
                    .iter()
                    .enumerate()
                    .any(|(i, (e, _))| i != ix && e.name.eq_ignore_ascii_case(&name))
                {
                    self.status =
                        Status::Error(format!("An environment named \"{name}\" already exists"));
                    return false;
                }
                let old = self.envs.get(ix).map(|(_, p)| p.clone());
                let env = Environment {
                    name,
                    variables: vars,
                };
                match store.save_environment(&env, old.as_deref()) {
                    Ok(path) => {
                        if ix < self.envs.len() {
                            self.envs[ix] = (env, path);
                        }
                        self.persist_active();
                        self.status = Status::Info("Environment saved".into());
                        true
                    }
                    Err(e) => {
                        self.status = Status::Error(e.to_string());
                        false
                    }
                }
            }
        }
    }

    fn select_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        if self.env_scope == scope {
            return;
        }
        // Switching never loses edits: save first; if that fails, stay where we are.
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        self.load_scope(scope, window, cx);
        cx.notify();
    }

    fn open_envs(&mut self, cx: &mut Context<Self>) {
        self.main_view = MainView::Environments;
        cx.notify();
    }

    fn close_envs(&mut self, cx: &mut Context<Self>) {
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        self.main_view = MainView::Request;
        cx.notify();
    }

    fn on_env_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        let Some(store) = self.store.clone() else {
            self.status = Status::Error("Storage is unavailable on this system".into());
            cx.notify();
            return;
        };
        let mut n = self.envs.len() + 1;
        let name = loop {
            let cand = format!("Environment {n}");
            if !self.envs.iter().any(|(e, _)| e.name == cand) {
                break cand;
            }
            n += 1;
        };
        let env = Environment {
            name,
            variables: Vec::new(),
        };
        match store.save_environment(&env, None) {
            Ok(path) => {
                self.envs.push((env, path));
                let ix = self.envs.len() - 1;
                self.load_scope(Scope::Env(ix), window, cx);
                self.status =
                    Status::Info("Environment created; rename it and add variables".into());
            }
            Err(e) => self.status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    fn on_env_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Scope::Env(ix) = self.env_scope else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let Some((_, path)) = self.envs.get(ix).cloned() else {
            return;
        };
        match store.delete_environment(&path) {
            Ok(()) => {
                self.envs.remove(ix);
                self.active_env = match self.active_env {
                    Some(a) if a == ix => None,
                    Some(a) if a > ix => Some(a - 1),
                    other => other,
                };
                self.persist_active();
                self.load_scope(Scope::Globals, window, cx);
                self.status = Status::Info("Environment deleted".into());
            }
            Err(e) => self.status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    fn render_env_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .p_2()
            .child(div().text_color(muted).child("ENVIRONMENTS & VARIABLES"))
            .child(
                Button::new("env-back")
                    .label("← Back to requests")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.close_envs(cx))),
            );

        // scope picker: Globals, each environment (● marks the active one), + New
        let mut scopes = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .pb_2();
        let g = Button::new("env-scope-g").label("Globals");
        let g = if self.env_scope == Scope::Globals {
            g.primary()
        } else {
            g
        };
        scopes = scopes.child(g.on_click(
            cx.listener(|this, _ev, window, cx| this.select_scope(Scope::Globals, window, cx)),
        ));
        for (ix, (e, _)) in self.envs.iter().enumerate() {
            let label = if self.active_env == Some(ix) {
                format!("● {}", e.name)
            } else {
                e.name.clone()
            };
            let b = Button::new(("env-scope", ix)).label(label);
            let b = if self.env_scope == Scope::Env(ix) {
                b.primary()
            } else {
                b
            };
            scopes = scopes.child(b.on_click(cx.listener(move |this, _ev, window, cx| {
                this.select_scope(Scope::Env(ix), window, cx)
            })));
        }
        scopes = scopes.child(
            Button::new("env-new")
                .label("+ New")
                .on_click(cx.listener(|this, _ev, window, cx| this.on_env_new(window, cx))),
        );

        let detail_head = match self.env_scope {
            Scope::Globals => div()
                .px_2()
                .pb_2()
                .text_color(muted)
                .child("Globals apply everywhere. The active environment overrides them."),
            Scope::Env(ix) => {
                let is_active = self.active_env == Some(ix);
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .pb_2()
                    .child(
                        div()
                            .w(px(240.))
                            .flex_none()
                            .child(Input::new(&self.env_name)),
                    )
                    .child(
                        Button::new("env-active")
                            .label(if is_active {
                                "Active ✓ (click to deactivate)"
                            } else {
                                "Make active"
                            })
                            .on_click(cx.listener(move |this, _ev, _window, cx| {
                                this.active_env = if this.active_env == Some(ix) {
                                    None
                                } else {
                                    Some(ix)
                                };
                                this.persist_active();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("env-del")
                            .label(if self.confirm_delete {
                                "Click again to delete"
                            } else {
                                "Delete"
                            })
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                if this.confirm_delete {
                                    this.confirm_delete = false;
                                    this.on_env_delete(window, cx);
                                } else {
                                    this.confirm_delete = true;
                                    cx.notify();
                                }
                            })),
                    )
            }
        };

        let mut table = div()
            .id("env-table")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .text_color(muted)
                    .child(div().w(px(24.)).flex_none())
                    .child(div().flex_1().child("Variable"))
                    .child(div().flex_1().child("Value"))
                    .child(div().w(px(150.)).flex_none())
                    .child(div().w(px(36.)).flex_none()),
            );
        for (ix, row) in self.env_rows.iter().enumerate() {
            let sec = Button::new(("env-sec", ix)).label("Secret");
            let sec = if row.secret { sec.primary() } else { sec };
            let reveal = row.secret.then(|| {
                Button::new(("env-show", ix))
                    .label(if row.revealed { "Hide" } else { "Show" })
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        if let Some(r) = this.env_rows.get_mut(ix) {
                            r.revealed = !r.revealed;
                            let masked = r.secret && !r.revealed;
                            r.value.update(cx, |s, cx| s.set_masked(masked, window, cx));
                        }
                        cx.notify();
                    }))
            });
            table = table.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div().w(px(24.)).flex_none().child(
                            Checkbox::new(("env-en", ix)).checked(row.enabled).on_click(
                                cx.listener(move |this, checked: &bool, _window, cx| {
                                    if let Some(r) = this.env_rows.get_mut(ix) {
                                        r.enabled = *checked;
                                    }
                                    cx.notify();
                                }),
                            ),
                        ),
                    )
                    .child(div().flex_1().child(Input::new(&row.key)))
                    .child(div().flex_1().child(Input::new(&row.value)))
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .child(sec.on_click(cx.listener(move |this, _ev, window, cx| {
                                if let Some(r) = this.env_rows.get_mut(ix) {
                                    r.secret = !r.secret;
                                    r.revealed = false;
                                    let masked = r.secret;
                                    r.value.update(cx, |s, cx| s.set_masked(masked, window, cx));
                                }
                                cx.notify();
                            })))
                            .children(reveal),
                    )
                    .child(div().w(px(36.)).flex_none().child(
                        Button::new(("env-rm", ix)).label("✕").on_click(cx.listener(
                            move |this, _ev, _window, cx| {
                                if ix < this.env_rows.len() {
                                    this.env_rows.remove(ix);
                                }
                                cx.notify();
                            },
                        )),
                    )),
            );
        }

        let footer = div()
            .flex()
            .flex_row()
            .gap_2()
            .p_2()
            .child(
                Button::new("env-add")
                    .label("+ Add variable")
                    .on_click(cx.listener(|this, _ev, window, cx| {
                        let row = Self::new_env_row(window, cx, &EnvVar::new("", ""));
                        this.env_rows.push(row);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("env-save")
                    .primary()
                    .label("Save")
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.save_env_scope(cx);
                        cx.notify();
                    })),
            );

        let (msg, msg_col): (String, Hsla) = match &self.status {
            Status::Info(m) => (m.clone(), muted),
            Status::Error(e) => (e.clone(), c(0xf38ba8)),
            _ => (String::new(), muted),
        };

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(scopes)
            .child(detail_head)
            .child(table)
            .child(footer)
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(border)
                    .text_color(msg_col)
                    .child(if msg.is_empty() {
                        "Use {{name}} in the URL, params, headers, body and auth. Secret values are masked here but stored as plain text on disk for now.".to_string()
                    } else {
                        msg
                    }),
            )
    }

    fn render_kv(&self, tab: &RequestTab, which: Which, cx: &Context<Self>) -> impl IntoElement {
        let (rows, en_id, del_id, add_id) = match which {
            Which::Params => (&tab.params, "p-en", "p-del", "p-add"),
            Which::Headers => (&tab.headers, "h-en", "h-del", "h-add"),
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
                                    if let Some(r) = this.tab_mut().rows_mut(which).get_mut(ix) {
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
                                let rows = this.tab_mut().rows_mut(which);
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
                    this.tab_mut().rows_mut(which).push(row);
                    cx.notify();
                },
            ))),
        )
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
                        path: t.dir.clone(),
                    });
                    push_nodes(&mut self.items, &t.children, 1);
                }
            }
            Err(e) => self.tab_mut().status = Status::Error(e.to_string()),
        }
    }

    fn on_sidebar_click(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(ix).cloned() else {
            return;
        };
        match item.kind {
            ItemKind::Collection | ItemKind::Folder => {
                let tab = self.tab_mut();
                tab.save_dir = Some(item.path);
                tab.current_path = None;
                tab.status = Status::Info("New requests will be saved here".into());
            }
            ItemKind::Broken(e) => self.tab_mut().status = Status::Error(e),
            ItemKind::Request => self.open_request(item.path, window, cx),
        }
        cx.notify();
    }

    /// Open a saved request: focus its tab if it is already open, reuse an untouched blank tab,
    /// otherwise open a new tab.
    fn open_request(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // Leaving the environments page autosaves it; if that fails, stay there.
        if self.main_view == MainView::Environments {
            self.close_envs(cx);
            if self.main_view == MainView::Environments {
                return;
            }
        }
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| t.current_path.as_ref() == Some(&path))
        {
            self.activate_tab(ix, window, cx);
            return;
        }
        let req = match Store::load_request(&path) {
            Ok(r) => r,
            Err(e) => {
                self.tab_mut().status = Status::Error(e.to_string());
                return;
            }
        };
        if !self.tab().is_pristine(cx) {
            self.add_tab(window, cx);
        }
        let tab = self.tab_mut();
        tab.apply_request(req, window, cx);
        tab.save_dir = path.parent().map(|d| d.to_path_buf());
        tab.current_path = Some(path);
        tab.confirm_close = false;
        tab.status = Status::Info("Opened".into());
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
                self.refresh_sidebar();
                let tab = self.tab_mut();
                tab.save_dir = Some(c.dir);
                tab.current_path = None;
                tab.status = Status::Info("Collection created; Save puts requests in it".into());
            }
            Err(e) => self.tab_mut().status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    fn on_save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // On the environments page, Cmd/Ctrl+S saves the variables being edited.
        if self.main_view == MainView::Environments {
            self.save_env_scope(cx);
            cx.notify();
            return;
        }
        let Some(store) = self.store.clone() else {
            self.tab_mut().status = Status::Error("Storage is unavailable on this system".into());
            cx.notify();
            return;
        };
        let mut req = self.tab().current_request(cx);
        if req.name.trim().is_empty() {
            req.name = "Untitled request".into();
        }
        let dir = match (&self.tab().current_path, &self.tab().save_dir) {
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
                        self.tab_mut().status = Status::Error(e.to_string());
                        cx.notify();
                        return;
                    }
                },
            },
        };
        match Store::save_request(&dir, &req, self.tab().current_path.as_deref()) {
            Ok(path) => {
                let saved = self.tab().current_request(cx);
                let tab = self.tab_mut();
                tab.save_dir = path.parent().map(|d| d.to_path_buf());
                tab.status = Status::Info(format!("Saved to {}", path.display()));
                tab.current_path = Some(path);
                tab.saved = saved;
                tab.confirm_close = false;
                self.refresh_sidebar();
            }
            Err(e) => self.tab_mut().status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    /// Send the active tab's request, or cancel it if one is already in flight.
    fn send_active(&mut self, cx: &mut Context<Self>) {
        let id = self.tab().id;
        self.send_tab(id, cx);
    }

    /// Send a tab's request, or cancel it if one is already in flight.
    fn send_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let vars = self.variables();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if let Some(token) = tab.cancel.take() {
            token.cancel();
            return;
        }
        let mut req = tab.current_request(cx);
        // Decide on the default scheme after substitution, so `{{base}}/users` works when
        // `base` already contains `https://`.
        if !req.url.is_empty() && !vars.resolve(&req.url).text.contains("://") {
            req.url = format!("https://{}", req.url);
        }
        if !has_body(tab.method) {
            req.body = Body::None;
        }
        let built = match build(&req, &vars) {
            Ok(b) => b,
            Err(e) => {
                tab.status = Status::Error(e.to_string());
                tab.warning = None;
                cx.notify();
                return;
            }
        };
        tab.warning = if built.unresolved.is_empty() {
            None
        } else {
            Some(format!(
                "Unresolved variables: {}",
                built.unresolved.join(", ")
            ))
        };
        tab.copied = false;

        let token = CancellationToken::new();
        tab.cancel = Some(token.clone());
        tab.status = Status::Loading;
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
                    // The tab may have been closed while the request was running.
                    if let Some(t) = this.tabs.iter_mut().find(|t| t.id == id) {
                        t.cancel = None;
                        t.apply(result);
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
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
        {
            let tab = self.tab_mut();
            if tab.wrap {
                if let Some(v) = tab.view.as_mut() {
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
            let max_x = tab.max_scroll_x(avail);
            if tab.scroll_x > max_x {
                tab.scroll_x = max_x;
            }
        }
        let tab = self.tab();
        let overflow = tab
            .view
            .as_ref()
            .map(|v| v.longest_chars as f32 * char_w() > avail)
            .unwrap_or(false);
        let scroll_x = if tab.wrap { 0.0 } else { tab.scroll_x };

        let theme = cx.theme();
        let (bg, fg, muted, border, accent) = (
            theme.background,
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.accent,
        );
        let in_flight = tab.cancel.is_some();

        // ---- tab bar: one button per open request, ● marks unsaved changes ----
        let mut tab_bar = div()
            .id("tab-bar")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .pt_2()
            .overflow_x_scroll();
        for (ix, t) in self.tabs.iter().enumerate() {
            let label = format!(
                "{}{}{}",
                if t.cancel.is_some() { "… " } else { "" },
                if t.is_dirty(cx) { "● " } else { "" },
                t.title(cx)
            );
            let b = Button::new(("tab", ix)).label(label);
            let b = if ix == self.active { b.primary() } else { b };
            tab_bar = tab_bar.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .child(b.on_click(
                        cx.listener(move |this, _ev, window, cx| this.activate_tab(ix, window, cx)),
                    ))
                    .child(
                        Button::new(("tab-x", ix))
                            .label(if t.confirm_close { "Discard?" } else { "✕" })
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.close_tab(ix, window, cx)
                            })),
                    ),
            );
        }
        tab_bar = tab_bar.child(
            div().flex_none().child(
                Button::new("tab-new")
                    .label("+")
                    .on_click(cx.listener(|this, _ev, window, cx| this.add_tab(window, cx))),
            ),
        );

        let top = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .p_2()
            .child(div().w(px(180.)).flex_none().child(Input::new(&tab.name)))
            .child(
                Button::new("method")
                    .label(tab.method.as_str())
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        let tab = this.tab_mut();
                        tab.method = next_method(tab.method);
                        cx.notify();
                    })),
            )
            .child(div().flex_1().child(Input::new(&tab.url)))
            .child(
                Button::new("send")
                    .primary()
                    .label(if in_flight { "Cancel" } else { "Send" })
                    .on_click(cx.listener(|this, _ev, _window, cx| this.send_active(cx))),
            )
            .child(
                Button::new("save")
                    .label("Save")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_save(window, cx))),
            )
            .child(
                Button::new("env-pick")
                    .label(format!(
                        "Env: {}",
                        self.active_env
                            .and_then(|i| self.envs.get(i))
                            .map(|(e, _)| e.name.as_str())
                            .unwrap_or("none")
                    ))
                    .on_click(cx.listener(|this, _ev, _window, cx| this.cycle_env(cx))),
            )
            .child(
                Button::new("env-manage")
                    .label("Variables…")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.open_envs(cx))),
            );

        // ---- request editor: tabs + panel ----
        let pcount = tab
            .params
            .iter()
            .filter(|r| r.enabled && !r.key.read(cx).value().is_empty())
            .count();
        let hcount = tab
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
                    tab.req_tab == ReqTab::Params,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Params;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-headers",
                    format!("Headers ({hcount})"),
                    tab.req_tab == ReqTab::Headers,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Headers;
                    cx.notify();
                })),
            )
            .child(
                seg_btn("t-body", "Body".to_string(), tab.req_tab == ReqTab::Body).on_click(
                    cx.listener(|this, _ev, _window, cx| {
                        this.tab_mut().req_tab = ReqTab::Body;
                        cx.notify();
                    }),
                ),
            );
        let req_panel =
            div()
                .id("req-panel")
                .h(px(210.))
                .overflow_y_scroll()
                .child(match tab.req_tab {
                    ReqTab::Params => self.render_kv(tab, Which::Params, cx).into_any_element(),
                    ReqTab::Headers => self.render_kv(tab, Which::Headers, cx).into_any_element(),
                    ReqTab::Body => div()
                        .p_2()
                        .h_full()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children((!has_body(tab.method)).then(|| {
                            div().text_color(muted).child(format!(
                                "{} requests don't send a body.",
                                tab.method.as_str()
                            ))
                        }))
                        .child(div().flex_1().child(Input::new(&tab.body).h_full()))
                        .into_any_element(),
                });

        let (status_text, status_col): (String, Hsla) = match &tab.status {
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
        let wrap_btn = (tab.res_tab == ResTab::Body && (tab.wrap || overflow)).then(|| {
            Button::new("wrap")
                .label(if tab.wrap { "Wrap: on" } else { "Wrap: off" })
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    let tab = this.tab_mut();
                    tab.wrap = !tab.wrap;
                    tab.scroll_x = 0.0;
                    if !tab.wrap {
                        if let Some(v) = tab.view.as_mut() {
                            v.wrapped = None;
                        }
                    }
                    cx.notify();
                }))
        });

        // Copy the (pretty-printed) response body to the clipboard (FR-RES-01).
        let copy_btn = (tab.res_tab == ResTab::Body && tab.view.is_some()).then(|| {
            Button::new("copy")
                .label(if tab.copied { "Copied ✓" } else { "Copy" })
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    let tab = this.tab_mut();
                    if let Some(v) = &tab.view {
                        let text = String::from_utf8_lossy(&v.data).into_owned();
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        tab.copied = true;
                        cx.notify();
                    }
                }))
        });

        let status_row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_3()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .child(div().text_color(status_col).child(status_text))
                    .children(
                        tab.warning
                            .clone()
                            .map(|w| div().text_color(c(0xf9e2af)).child(w)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(
                        seg_btn("r-body", "Body".to_string(), tab.res_tab == ResTab::Body)
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.tab_mut().res_tab = ResTab::Body;
                                cx.notify();
                            })),
                    )
                    .child(
                        seg_btn(
                            "r-headers",
                            format!("Headers ({})", tab.resp_headers.len()),
                            tab.res_tab == ResTab::Headers,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            this.tab_mut().res_tab = ResTab::Headers;
                            cx.notify();
                        })),
                    )
                    .children(copy_btn)
                    .children(wrap_btn),
            );

        let viewer = match &tab.view {
            Some(v) => {
                let rows = if tab.wrap {
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

        let resp_area = match tab.res_tab {
            ResTab::Body => viewer,
            ResTab::Headers => div()
                .id("resp-headers")
                .size_full()
                .overflow_y_scroll()
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .children(tab.resp_headers.iter().map(|(k, v)| {
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
                ItemKind::Request => tab.current_path.as_ref() == Some(&item.path),
                ItemKind::Collection | ItemKind::Folder => {
                    tab.current_path.is_none() && tab.save_dir.as_ref() == Some(&item.path)
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
            .child(tab_bar)
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
                        let tab = this.tab_mut();
                        if tab.wrap || tab.res_tab != ResTab::Body {
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
                        let max = tab.max_scroll_x(avail_width(window));
                        tab.scroll_x = (tab.scroll_x - dx).clamp(0.0, max);
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
            .on_action(cx.listener(|this, _: &SendRequest, _window, cx| this.send_active(cx)))
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                if this.main_view == MainView::Request {
                    this.add_tab(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                if this.main_view == MainView::Request {
                    let ix = this.active;
                    this.close_tab(ix, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SaveRequest, window, cx| this.on_save(window, cx)))
            .child(sidebar)
            .child(if self.main_view == MainView::Environments {
                self.render_env_page(cx).into_any_element()
            } else {
                main_col.into_any_element()
            })
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
        let (send_key, save_key, new_key, close_key) = ("cmd-enter", "cmd-s", "cmd-t", "cmd-w");
        #[cfg(not(target_os = "macos"))]
        let (send_key, save_key, new_key, close_key) = ("ctrl-enter", "ctrl-s", "ctrl-t", "ctrl-w");
        cx.bind_keys([
            KeyBinding::new(send_key, SendRequest, None),
            KeyBinding::new(save_key, SaveRequest, None),
            KeyBinding::new(new_key, NewTab, None),
            KeyBinding::new(close_key, CloseTab, None),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_titles() {
        assert_eq!(tab_title("Get users"), "Get users");
        assert_eq!(tab_title("  padded  "), "padded");
        assert_eq!(tab_title(""), "Untitled");
        assert_eq!(tab_title("   "), "Untitled");
        let long = "a".repeat(TAB_TITLE_MAX + 10);
        let t = tab_title(&long);
        assert_eq!(t.chars().count(), TAB_TITLE_MAX);
        assert!(t.ends_with('…'));
        // exactly at the limit is left alone
        let exact = "b".repeat(TAB_TITLE_MAX);
        assert_eq!(tab_title(&exact), exact);
        // multi-byte characters are counted as characters, never split
        let wide = "é".repeat(TAB_TITLE_MAX + 5);
        assert_eq!(tab_title(&wide).chars().count(), TAB_TITLE_MAX);
    }

    #[test]
    fn active_tab_after_closing() {
        // closing a tab left of the active one shifts the active index down
        assert_eq!(active_after_close(2, 0, 3), 1);
        // closing the active tab selects its right neighbour (same index)
        assert_eq!(active_after_close(1, 1, 3), 1);
        // closing the active last tab selects the new last one
        assert_eq!(active_after_close(2, 2, 2), 1);
        // closing a tab right of the active one changes nothing
        assert_eq!(active_after_close(0, 2, 2), 0);
        // closing the only tab (a fresh one is added by the caller)
        assert_eq!(active_after_close(0, 0, 1), 0);
        assert_eq!(active_after_close(0, 0, 0), 0);
    }
}
