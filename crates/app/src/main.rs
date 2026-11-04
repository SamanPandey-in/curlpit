//! API Tester — one window, request tabs, send a request, read the response.
//! Network work runs on its own tokio threads; results come back over a channel.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use at_codegen::{generate, parse_curl, Lang};
use at_core::lines::{build_rows, count_chars, find_all, RowIndex, CONT, MAX_ROW_CHARS};
use at_core::{
    build, build_template, pretty::pretty_json, to_map, ApiKeyLocation, Auth, Body, EnvVar,
    Environment, FormField, FormValue, KeyValue, Method, Request, RequestSettings, Variables,
};
use at_history::{ago, now_ms, History, Record, Summary};
use at_http::{send, CookieJar, EngineConfig, HttpError, ResponseBody};
use at_interop::{
    export_collection, export_environment, import_postman, read_collection, write_collection,
    Imported,
};
use at_script::{
    LogLevel, Phase, ScriptInput, ScriptOutput, ScriptRequest, ScriptResponse, VarMap,
};
use at_store::{slug, Node, Session, SessionTab, Settings, Store, StoreError};
use gpui::{
    actions, div, prelude::*, px, rgb, size, uniform_list, App, Application, Bounds, ClipboardItem,
    Context, Entity, FocusHandle, Hsla, KeyBinding, PathPromptOptions, ScrollStrategy,
    ScrollWheelEvent, Subscription, UniformListScrollHandle, Window, WindowBounds, WindowOptions,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Root};
use tokio_util::sync::CancellationToken;

/// Width of the sticky line-number gutter (px).
const GUTTER: f32 = 76.0;

actions!(
    api_tester,
    [
        SendRequest,
        SaveRequest,
        NewTab,
        CloseTab,
        ReopenTab,
        FindInResponse
    ]
);

/// How often open tabs are written to `session.toml` (only when something changed).
const SESSION_SAVE_EVERY: Duration = Duration::from_secs(3);

/// Closed tabs remembered for Cmd/Ctrl+Shift+T.
const CLOSED_TABS_MAX: usize = 20;

/// A closed tab, kept so it can be reopened with its edits.
struct ClosedTab {
    path: Option<PathBuf>,
    save_dir: Option<PathBuf>,
    draft: Request,
    saved: Request,
}

/// Longest tab title, in characters; longer names are cut with an ellipsis.
const TAB_TITLE_MAX: usize = 24;

/// Most history rows shown in the sidebar at once (the file keeps up to 1,000).
const HISTORY_LIST_MAX: usize = 500;

/// Bytes to show for a response body: pretty-printed when it looks like JSON.
fn display_body(bytes: &[u8], content_type: &str) -> Vec<u8> {
    let first = bytes.iter().find(|ch| !ch.is_ascii_whitespace()).copied();
    if content_type.contains("json") || matches!(first, Some(b'{') | Some(b'[')) {
        pretty_json(bytes, 2)
    } else {
        bytes.to_vec()
    }
}

fn content_type(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// What history keeps of one send. `request` is the request as typed (variables unresolved).
fn history_record(
    request: Request,
    result: &Result<at_http::HttpResponse, HttpError>,
    elapsed: Duration,
) -> Record {
    match result {
        Ok(resp) => Record {
            request,
            status: Some(resp.status),
            status_text: resp.status_text.clone(),
            elapsed_ms: resp.elapsed.as_millis() as u64,
            size: resp.size,
            error: None,
            resp_headers: resp.headers.clone(),
            resp_body: match &resp.body {
                // Copy at most one byte more than history keeps, so it can tell the body was cut.
                ResponseBody::Memory(b) => {
                    Some(b[..b.len().min(at_history::DEFAULT_MAX_BODY + 1)].to_vec())
                }
                ResponseBody::File { .. } => None,
            },
        },
        Err(e) => Record {
            request,
            elapsed_ms: elapsed.as_millis() as u64,
            error: Some(e.to_string()),
            ..Default::default()
        },
    }
}

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
    Auth,
    Scripts,
    Settings,
}

/// What the Body tab edits.
#[derive(Clone, Copy, PartialEq, Debug)]
enum BodyMode {
    None,
    Raw,
    UrlEncoded,
    FormData,
}

/// Flavour of a raw body; decides the default Content-Type.
#[derive(Clone, Copy, PartialEq, Debug)]
enum RawKind {
    Json,
    Xml,
    Html,
    Text,
}

impl RawKind {
    const ALL: [RawKind; 4] = [RawKind::Json, RawKind::Xml, RawKind::Html, RawKind::Text];

    fn label(self) -> &'static str {
        match self {
            RawKind::Json => "JSON",
            RawKind::Xml => "XML",
            RawKind::Html => "HTML",
            RawKind::Text => "Text",
        }
    }

    fn body(self, raw: String) -> Body {
        match self {
            RawKind::Json => Body::Json { raw },
            RawKind::Xml => Body::Xml { raw },
            RawKind::Html => Body::Html { raw },
            RawKind::Text => Body::Text { raw },
        }
    }
}

/// What the Auth tab edits. `Kept` is an auth type without an editor yet (JWT, custom
/// headers) that came from a file: it is sent and saved unchanged.
#[derive(Clone, Copy, PartialEq, Debug)]
enum AuthKind {
    None,
    Basic,
    Bearer,
    ApiKey,
    Kept,
}

impl AuthKind {
    const EDITABLE: [AuthKind; 4] = [
        AuthKind::None,
        AuthKind::Basic,
        AuthKind::Bearer,
        AuthKind::ApiKey,
    ];

    fn label(self) -> &'static str {
        match self {
            AuthKind::None => "None",
            AuthKind::Basic => "Basic",
            AuthKind::Bearer => "Bearer token",
            AuthKind::ApiKey => "API key",
            AuthKind::Kept => "From file",
        }
    }
}

/// Short tab-label hint for the body: "Body", "Body (XML)", "Body (form)".
fn body_label(mode: BodyMode, kind: RawKind) -> String {
    match mode {
        BodyMode::None => "Body".into(),
        BodyMode::Raw => format!("Body ({})", kind.label()),
        BodyMode::UrlEncoded => "Body (urlencoded)".into(),
        BodyMode::FormData => "Body (form-data)".into(),
    }
}

/// Timeout box text -> milliseconds. Blank means the default; anything unparsable keeps
/// `fallback` so a half-typed value never silently becomes "no timeout".
fn parse_timeout(text: &str, fallback: u64) -> u64 {
    let t = text.trim();
    if t.is_empty() {
        return RequestSettings::default().timeout_ms;
    }
    t.parse().unwrap_or(fallback)
}

#[derive(Clone, Copy, PartialEq)]
enum ResTab {
    Body,
    Headers,
    Tests,
    Code,
}

/// What the scripts of the last send did, for the Tests view.
#[derive(Clone, Default)]
struct ScriptReport {
    pre: Option<ScriptOutput>,
    test: Option<ScriptOutput>,
    /// e.g. "pm.environment changes were ignored: no environment is active"
    notes: Vec<String>,
}

impl ScriptReport {
    fn label(&self) -> String {
        let errors = [&self.pre, &self.test]
            .iter()
            .any(|o| o.as_ref().is_some_and(|o| o.error.is_some()));
        match &self.test {
            Some(t) if !t.tests.is_empty() => {
                let counted = t.tests.iter().filter(|x| !x.skipped).count();
                format!("Tests ({}/{counted})", t.passed())
            }
            _ if errors => "Tests (!)".into(),
            _ => "Tests".into(),
        }
    }
}

/// Response body text handed to test scripts (big bodies are cut; spilled ones aren't read).
const SCRIPT_BODY_MAX: usize = 10 * 1024 * 1024;

/// The request as a script sees it: as typed (variables unresolved), enabled headers only.
fn script_request(req: &Request) -> ScriptRequest {
    ScriptRequest {
        method: req.method.as_str().to_string(),
        url: req.url.clone(),
        headers: req
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.is_empty())
            .map(|h| (h.key.clone(), h.value.clone()))
            .collect(),
        body: match &req.body {
            Body::Json { raw } | Body::Xml { raw } | Body::Html { raw } | Body::Text { raw } => {
                raw.clone()
            }
            _ => String::new(),
        },
    }
}

fn to_var_map(m: &std::collections::HashMap<String, String>) -> VarMap {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn script_input(
    req: &Request,
    vars: &Variables,
    env_name: Option<String>,
    response: Option<ScriptResponse>,
) -> ScriptInput {
    ScriptInput {
        request_name: req.name.clone(),
        request: script_request(req),
        response,
        environment_name: env_name,
        environment: to_var_map(&vars.environment),
        globals: to_var_map(&vars.global),
        variables: to_var_map(&vars.runtime),
    }
}

/// Take a script's variable changes into the variables used for this send.
fn take_script_vars(vars: &mut Variables, o: &ScriptOutput) {
    vars.environment = o.environment.clone().into_iter().collect();
    vars.global = o.globals.clone().into_iter().collect();
    vars.runtime = o.variables.clone().into_iter().collect();
}

/// Take a pre-request script's changes to headers, URL and raw body into the request.
fn take_script_request(req: &mut Request, before: &ScriptRequest, after: &ScriptRequest) {
    if after.headers != before.headers {
        req.headers = after
            .headers
            .iter()
            .map(|(k, v)| KeyValue::new(k, v))
            .collect();
    }
    if after.url != before.url {
        req.url = after.url.clone();
    }
    if after.body != before.body {
        req.body = match &req.body {
            Body::Xml { .. } => Body::Xml {
                raw: after.body.clone(),
            },
            Body::Html { .. } => Body::Html {
                raw: after.body.clone(),
            },
            Body::Text { .. } => Body::Text {
                raw: after.body.clone(),
            },
            // a script can give a body-less request a JSON body; form bodies stay as they are
            Body::UrlEncoded { .. } | Body::FormData { .. } => req.body.clone(),
            _ => Body::Json {
                raw: after.body.clone(),
            },
        };
    }
}

fn script_response(resp: &at_http::HttpResponse) -> ScriptResponse {
    ScriptResponse {
        code: resp.status,
        status: resp.status_text.clone(),
        headers: resp.headers.clone(),
        body: match &resp.body {
            ResponseBody::Memory(b) => {
                String::from_utf8_lossy(&b[..b.len().min(SCRIPT_BODY_MAX)]).into_owned()
            }
            ResponseBody::File { .. } => String::new(),
        },
        time_ms: resp.elapsed.as_millis() as u64,
        size: resp.size,
    }
}

/// Apply what scripts changed (`before` -> `after`, enabled variables only) to a stored list.
/// Works as a diff, so two tabs whose scripts ran at the same time don't undo each other, and
/// secret flags and disabled variables are kept. `None` when nothing changed.
fn merge_script_vars(existing: &[EnvVar], before: &VarMap, after: &VarMap) -> Option<Vec<EnvVar>> {
    if before == after {
        return None;
    }
    let mut out = existing.to_vec();
    for k in before.keys().filter(|k| !after.contains_key(*k)) {
        out.retain(|v| !(v.enabled && &v.key == k));
    }
    for (k, v) in after {
        if before.get(k) == Some(v) {
            continue;
        }
        match out.iter_mut().find(|e| e.enabled && &e.key == k) {
            Some(e) => e.value = v.clone(),
            None => out.push(EnvVar::new(k, v)),
        }
    }
    Some(out)
}

/// One request through the whole pipeline, off the UI thread: pre-request script -> build ->
/// send -> test script (scripts on the blocking pool so a slow one never stalls networking).
/// Returns what happened plus the variables after its scripts, so a run can chain them.
/// `history: None` skips recording (the collection runner doesn't fill History).
async fn execute(
    mut req: Request,
    mut vars: Variables,
    env_name: Option<String>,
    token: CancellationToken,
    history: Option<Arc<Mutex<History>>>,
    cookies: Arc<CookieJar>,
) -> (SendOutcome, Variables) {
    let env_before = to_var_map(&vars.environment);
    let globals_before = to_var_map(&vars.global);
    let run = |src: String, phase: Phase, input: ScriptInput| {
        tokio::task::spawn_blocking(move || {
            at_script::run(&src, phase, &input, at_script::Limits::default())
        })
    };

    let pre = if req.pre_request_script.trim().is_empty() {
        None
    } else {
        let input = script_input(&req, &vars, env_name.clone(), None);
        let before = input.request.clone();
        let out = run(req.pre_request_script.clone(), Phase::PreRequest, input)
            .await
            .ok();
        if let Some(o) = &out {
            take_script_vars(&mut vars, o);
            take_script_request(&mut req, &before, &o.request);
        }
        out
    };

    let req = sendable(req, &vars);
    let built = match build(&req, &vars) {
        Ok(b) => b,
        Err(e) => {
            let out = SendOutcome {
                sent: Err(e.to_string()),
                unresolved: Vec::new(),
                pre,
                test: None,
                env_before,
                globals_before,
                hist_err: None,
            };
            return (out, vars);
        }
    };
    let started = Instant::now();
    let engine = EngineConfig {
        cookies: Some(cookies),
        ..Default::default()
    };
    let r = send(&built, &req.settings, &engine, &token).await;

    let test = match &r {
        Ok(resp) if !req.test_script.trim().is_empty() => {
            let input = script_input(&req, &vars, env_name, Some(script_response(resp)));
            let out = run(req.test_script.clone(), Phase::Test, input).await.ok();
            if let Some(o) = &out {
                take_script_vars(&mut vars, o);
            }
            out
        }
        _ => None,
    };

    // Record on the network thread so the UI never waits for the disk.
    let hist_err = history.and_then(|h| {
        let rec = history_record(req, &r, started.elapsed());
        match h.lock() {
            Ok(mut h) => h.add(&rec, now_ms()).err().map(|e| e.to_string()),
            Err(_) => Some("history is unavailable".to_string()),
        }
    });
    let out = SendOutcome {
        sent: Ok(r),
        unresolved: built.unresolved,
        pre,
        test,
        env_before,
        globals_before,
        hist_err,
    };
    (out, vars)
}

// ---------------- collection runner (FR-RUN) ----------------

/// Requests of a collection/folder in run order (the sidebar's order: folders first, then by
/// name), each with its file or why it can't run.
fn run_order(nodes: &[Node], out: &mut Vec<(String, Result<PathBuf, String>)>) {
    for n in nodes {
        match n {
            Node::Folder { children, .. } => run_order(children, out),
            Node::Request { name, path } => out.push((name.clone(), Ok(path.clone()))),
            Node::Broken { name, error, .. } => out.push((name.clone(), Err(error.clone()))),
        }
    }
}

/// Whole number from an options box, clamped; blank or invalid gives `default`.
fn parse_count(text: &str, default: u64, min: u64, max: u64) -> u64 {
    text.trim()
        .parse::<u64>()
        .map(|n| n.clamp(min, max))
        .unwrap_or(default)
}

#[derive(Clone, Debug, PartialEq)]
enum RunResult {
    Response {
        code: u16,
        text: String,
        ms: u128,
        size: u64,
    },
    /// No response: network error, build error, unreadable file, cancelled.
    Failed(String),
}

#[derive(Clone, Debug)]
struct RunRow {
    iteration: usize,
    name: String,
    path: Option<PathBuf>,
    method: Method,
    result: RunResult,
    tests: Vec<at_script::TestResult>,
    /// Pre-request / test script errors.
    errors: Vec<String>,
}

impl RunRow {
    /// A row counts as failed when there was no response, a script broke, or a test failed.
    /// (An HTTP error status alone isn't a failure: write a test for that.)
    fn failed(&self) -> bool {
        matches!(self.result, RunResult::Failed(_))
            || !self.errors.is_empty()
            || self.tests.iter().any(|t| !t.passed)
    }

    fn from_outcome(
        iteration: usize,
        name: String,
        path: PathBuf,
        method: Method,
        out: &SendOutcome,
    ) -> RunRow {
        let result = match &out.sent {
            Err(e) => RunResult::Failed(e.clone()),
            Ok(Err(e)) => RunResult::Failed(e.to_string()),
            Ok(Ok(resp)) => RunResult::Response {
                code: resp.status,
                text: resp.status_text.clone(),
                ms: resp.elapsed.as_millis(),
                size: resp.size,
            },
        };
        let mut errors = Vec::new();
        for (what, o) in [("pre-request", &out.pre), ("tests", &out.test)] {
            if let Some(e) = o.as_ref().and_then(|o| o.error.as_ref()) {
                errors.push(format!("{what} script: {e}"));
            }
        }
        RunRow {
            iteration,
            name,
            path: Some(path),
            method,
            result,
            tests: out
                .test
                .as_ref()
                .map(|t| t.tests.clone())
                .unwrap_or_default(),
            errors,
        }
    }
}

/// Run progress from the network task to the UI.
enum RunMsg {
    Row {
        row: Box<RunRow>,
        /// (before, after) of the environment and globals, to save script changes.
        env: (VarMap, VarMap),
        globals: (VarMap, VarMap),
    },
    Done,
}

/// The runner page's state.
struct Runner {
    /// Collection or folder being run: (display name, directory).
    target: Option<(String, PathBuf)>,
    iterations: Entity<InputState>,
    delay: Entity<InputState>,
    stop_on_fail: bool,
    rows: Vec<RunRow>,
    planned: usize,
    cancel: Option<CancellationToken>,
    started: Option<Instant>,
    elapsed: Option<Duration>,
    notes: Vec<String>,
    copied: bool,
}

impl Runner {
    /// (requests run, requests failed, tests passed, tests failed)
    fn counts(&self) -> (usize, usize, usize, usize) {
        let failed = self.rows.iter().filter(|r| r.failed()).count();
        let tests = self.rows.iter().flat_map(|r| r.tests.iter());
        let (mut pass, mut fail) = (0, 0);
        for t in tests {
            if t.skipped {
                continue;
            }
            if t.passed {
                pass += 1;
            } else {
                fail += 1;
            }
        }
        (self.rows.len(), failed, pass, fail)
    }

    /// Plain-text report for the clipboard.
    fn summary_text(&self) -> String {
        let (n, failed, pass, fail) = self.counts();
        let name = self.target.as_ref().map(|(n, _)| n.as_str()).unwrap_or("");
        let mut s = format!(
            "Run \"{name}\": {n} request(s), {failed} failed; tests {pass} passed, {fail} failed\n"
        );
        for r in &self.rows {
            let status = match &r.result {
                RunResult::Response { code, ms, .. } => format!("{code} ({ms} ms)"),
                RunResult::Failed(e) => format!("ERROR {e}"),
            };
            s.push_str(&format!(
                "{} #{} {} {} -> {status}\n",
                if r.failed() { "FAIL" } else { "ok  " },
                r.iteration + 1,
                r.method.as_str(),
                r.name
            ));
            for t in r.tests.iter().filter(|t| !t.passed) {
                s.push_str(&format!(
                    "      ✗ {}: {}\n",
                    t.name,
                    t.error.as_deref().unwrap_or("")
                ));
            }
            for e in &r.errors {
                s.push_str(&format!("      ! {e}\n"));
            }
        }
        s
    }
}

/// What a send produced; sent from the network task back to the UI thread.
struct SendOutcome {
    sent: Result<Result<at_http::HttpResponse, HttpError>, String>,
    unresolved: Vec<String>,
    pre: Option<ScriptOutput>,
    test: Option<ScriptOutput>,
    env_before: VarMap,
    globals_before: VarMap,
    hist_err: Option<String>,
}

#[derive(Clone, Copy)]
enum Which {
    Params,
    Headers,
    /// urlencoded body fields
    Form,
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

/// The request as it goes out: `https://` is added when the URL has no scheme *after*
/// variable substitution (so `{{base}}/users` works when `base` contains one), and methods
/// that don't take a body send none.
fn sendable(mut req: Request, vars: &Variables) -> Request {
    if !req.url.is_empty() && !vars.resolve(&req.url).text.contains("://") {
        req.url = format!("https://{}", req.url);
    }
    if !has_body(req.method) {
        req.body = Body::None;
    }
    req
}

/// Import/export notes appended to a status message: ". 2 notes: a; b".
fn notes(warnings: &[String]) -> String {
    if warnings.is_empty() {
        String::new()
    } else {
        format!(". {} note(s): {}", warnings.len(), warnings.join("; "))
    }
}

/// `name`, or `name 2`, `name 3`, ... — the first one `taken` says is free.
fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = if name.trim().is_empty() {
        "Imported"
    } else {
        name.trim()
    };
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !taken(n))
        .unwrap_or_else(|| base.to_string())
}

/// Merge imported variables into existing ones: same key (exact) replaces, new keys are added.
/// Returns the merged list and how many were replaced.
fn merge_vars(existing: &[EnvVar], incoming: Vec<EnvVar>) -> (Vec<EnvVar>, usize) {
    let mut out = existing.to_vec();
    let mut replaced = 0;
    for v in incoming {
        match out.iter_mut().find(|e| e.key == v.key) {
            Some(e) => {
                *e = v;
                replaced += 1;
            }
            None => out.push(v),
        }
    }
    (out, replaced)
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

/// One editable key/value row (params, headers, urlencoded fields).
struct KvRow {
    enabled: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
}

/// One form-data row: a text value or a file path.
struct FormRow {
    enabled: bool,
    is_file: bool,
    key: Entity<InputState>,
    /// The text value, or the file path when `is_file`.
    value: Entity<InputState>,
}

/// Form-data rows -> fields; completely empty rows are skipped like in `collect`.
fn collect_form(rows: &[FormRow], cx: &App) -> Vec<FormField> {
    rows.iter()
        .filter_map(|r| {
            let key = r.key.read(cx).value().to_string();
            let value = r.value.read(cx).value().to_string();
            if key.trim().is_empty() && value.is_empty() {
                return None;
            }
            Some(FormField {
                key,
                value: if r.is_file {
                    FormValue::File { path: value }
                } else {
                    FormValue::Text { value }
                },
                enabled: r.enabled,
            })
        })
        .collect()
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
    Runner,
    Cookies,
}

/// "in 5 min", "in 3 h", "in 2 d", "session" for the cookie list.
fn expires_label(now_secs: i64, expires: Option<i64>) -> String {
    let Some(at) = expires else {
        return "session".into();
    };
    let left = at - now_secs;
    match left {
        i64::MIN..=0 => "expired".into(),
        1..=59 => "in <1 min".into(),
        60..=3599 => format!("in {} min", left / 60),
        3600..=86_399 => format!("in {} h", left / 3600),
        _ => format!("in {} d", left / 86_400),
    }
}

/// What the sidebar lists.
#[derive(Clone, Copy, PartialEq)]
enum SideView {
    Collections,
    History,
}

/// What the environments page is editing.
#[derive(Clone, Copy, PartialEq)]
enum Scope {
    Globals,
    Env(usize),
}

/// `seg_btn` for a row of buttons sharing one id prefix.
fn seg_btn_ix(id: &'static str, ix: usize, label: &str, selected: bool) -> Button {
    let b = Button::new((id, ix)).label(label.to_string());
    if selected {
        b.primary()
    } else {
        b
    }
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

impl ViewData {
    /// Index a body for the viewer. Pure, so big bodies can be indexed off the UI thread.
    fn new(data: Vec<u8>) -> ViewData {
        let base = build_rows(&data, MAX_ROW_CHARS);
        let longest = base.longest_row(&data);
        let longest_chars = count_chars(base.row(&data, longest));
        ViewData {
            data: Arc::new(data),
            base: Arc::new(base),
            wrapped: None,
            longest_chars,
        }
    }

    /// The rows currently on screen (wrapped or not).
    fn rows(&self, wrap: bool) -> Arc<RowIndex> {
        match (&self.wrapped, wrap) {
            (Some((_, r)), true) => r.clone(),
            _ => self.base.clone(),
        }
    }
}

/// Largest spilled body loaded into the viewer (the viewer was measured at ~130 MB of RAM for
/// 50 MB); bigger ones can still be saved with "Save…".
const VIEW_MAX_BYTES: u64 = 200 * 1024 * 1024;

/// Most matches counted by Find (searching stops there).
const FIND_MAX: usize = 10_000;

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
    // ---- body editor ----
    body_mode: BodyMode,
    raw_kind: RawKind,
    form: Vec<KvRow>,
    multipart: Vec<FormRow>,
    // ---- auth editor (one input per field, so switching type keeps what was typed) ----
    auth_kind: AuthKind,
    auth_user: Entity<InputState>,
    auth_pass: Entity<InputState>,
    auth_token: Entity<InputState>,
    auth_key: Entity<InputState>,
    auth_value: Entity<InputState>,
    auth_in_query: bool,
    /// Secret auth fields are shown in clear text.
    auth_reveal: bool,
    // ---- settings ----
    timeout: Entity<InputState>,
    follow_redirects: bool,
    verify_tls: bool,
    // ---- scripts ----
    pre_script: Entity<InputState>,
    test_script: Entity<InputState>,
    /// The Scripts tab shows the test script (else the pre-request one).
    scripts_show_tests: bool,
    report: Option<ScriptReport>,
    /// The file this tab is saved to, if any.
    current_path: Option<PathBuf>,
    /// Folder the next Save goes to when the tab has no file yet.
    save_dir: Option<PathBuf>,
    /// The editor contents as of the last open/save; the tab is "unsaved" when they differ.
    saved: Request,
    /// The request as last loaded; supplies the id and any auth type without an editor yet.
    base: Request,
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
    // ---- big bodies and Find ----
    /// The spilled file holding the full body, when it was too big for memory.
    body_file: Option<PathBuf>,
    /// A spilled body waiting to be loaded into the viewer.
    loading_file: Option<PathBuf>,
    /// Bumped per response, so a slow background load never replaces a newer response.
    view_gen: u64,
    list_scroll: UniformListScrollHandle,
    find: Entity<InputState>,
    find_open: bool,
    /// Byte offsets of matches in the viewer's data.
    matches: Vec<usize>,
    match_ix: usize,
    /// (data pointer, query) the matches belong to.
    matches_key: (usize, String),
    scroll_to_match: bool,
    _sub: Subscription,
    _find_sub: Subscription,
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
        let form = vec![Workspace::new_row(window, cx, "", "")];
        let multipart = vec![Workspace::new_form_row(
            window,
            cx,
            &FormField {
                key: String::new(),
                value: FormValue::Text {
                    value: String::new(),
                },
                enabled: true,
            },
        )];
        let field = |window: &mut Window, cx: &mut Context<Workspace>, hint: &str, secret: bool| {
            let hint = hint.to_string();
            cx.new(|cx| {
                let s = InputState::new(window, cx).placeholder(hint);
                if secret {
                    s.masked(true)
                } else {
                    s
                }
            })
        };
        let auth_user = field(window, cx, "Username", false);
        let auth_pass = field(window, cx, "Password", true);
        let auth_token = field(window, cx, "Token", true);
        let auth_key = field(
            window,
            cx,
            "Header or query parameter name, e.g. X-API-Key",
            false,
        );
        let auth_value = field(window, cx, "Value", true);
        let timeout = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("30000")
                .default_value(RequestSettings::default().timeout_ms.to_string())
        });
        let script_editor = |window: &mut Window, cx: &mut Context<Workspace>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("javascript")
                    .line_number(true)
            })
        };
        let pre_script = script_editor(window, cx);
        let test_script = script_editor(window, cx);
        let find = cx.new(|cx| InputState::new(window, cx).placeholder("Find in response"));
        // Enter: next match; the secondary Enter (Cmd/Ctrl+Enter): previous.
        let find_sub = cx.subscribe_in(
            &find,
            window,
            move |this, _state, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { secondary } = event {
                    this.find_step(id, if *secondary { -1 } else { 1 }, cx);
                }
            },
        );
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
            body_mode: BodyMode::Raw,
            raw_kind: RawKind::Json,
            form,
            multipart,
            auth_kind: AuthKind::None,
            auth_user,
            auth_pass,
            auth_token,
            auth_key,
            auth_value,
            auth_in_query: false,
            auth_reveal: false,
            timeout,
            follow_redirects: true,
            verify_tls: true,
            pre_script,
            test_script,
            scripts_show_tests: false,
            report: None,
            current_path: None,
            save_dir: None,
            saved: Request::default(),
            base: Request::default(),
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
            body_file: None,
            loading_file: None,
            view_gen: 0,
            list_scroll: UniformListScrollHandle::new(),
            find,
            find_open: false,
            matches: Vec::new(),
            match_ix: 0,
            matches_key: (0, String::new()),
            scroll_to_match: false,
            _sub: sub,
            _find_sub: find_sub,
        };
        tab.saved = tab.current_request(cx);
        tab
    }

    fn rows_mut(&mut self, which: Which) -> &mut Vec<KvRow> {
        match which {
            Which::Params => &mut self.params,
            Which::Headers => &mut self.headers,
            Which::Form => &mut self.form,
        }
    }

    fn text(input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().to_string()
    }

    /// The request as currently shown in the editor.
    fn current_request(&self, cx: &App) -> Request {
        let auth = match self.auth_kind {
            AuthKind::None => Auth::None,
            AuthKind::Basic => Auth::Basic {
                username: Self::text(&self.auth_user, cx),
                password: Self::text(&self.auth_pass, cx),
            },
            AuthKind::Bearer => Auth::Bearer {
                token: Self::text(&self.auth_token, cx),
            },
            AuthKind::ApiKey => Auth::ApiKey {
                key: Self::text(&self.auth_key, cx),
                value: Self::text(&self.auth_value, cx),
                location: if self.auth_in_query {
                    ApiKeyLocation::Query
                } else {
                    ApiKeyLocation::Header
                },
            },
            AuthKind::Kept => self.base.auth.clone(),
        };
        let body = match self.body_mode {
            BodyMode::None => Body::None,
            BodyMode::Raw => {
                let text = Self::text(&self.body, cx);
                if text.trim().is_empty() {
                    Body::None
                } else {
                    self.raw_kind.body(text)
                }
            }
            BodyMode::UrlEncoded => Body::UrlEncoded {
                fields: collect(&self.form, cx),
            },
            BodyMode::FormData => Body::FormData {
                fields: collect_form(&self.multipart, cx),
            },
        };
        Request {
            id: self.base.id.clone(),
            name: Self::text(&self.name, cx),
            method: self.method,
            url: Self::text(&self.url, cx).trim().to_string(),
            pre_request_script: Self::text(&self.pre_script, cx),
            test_script: Self::text(&self.test_script, cx),
            params: collect(&self.params, cx),
            headers: collect(&self.headers, cx),
            body,
            auth,
            settings: RequestSettings {
                timeout_ms: parse_timeout(
                    &Self::text(&self.timeout, cx),
                    self.base.settings.timeout_ms,
                ),
                follow_redirects: self.follow_redirects,
                verify_tls: self.verify_tls,
            },
        }
    }

    /// Describes an auth type that has no editor yet (kept as loaded).
    fn kept_auth(&self) -> Option<&'static str> {
        match (&self.auth_kind, &self.base.auth) {
            (AuthKind::Kept, Auth::Jwt { .. }) => Some("JWT"),
            (AuthKind::Kept, Auth::Custom { .. }) => Some("custom auth headers"),
            _ => None,
        }
    }

    fn set_auth_masked(&self, window: &mut Window, cx: &mut Context<Workspace>) {
        let masked = !self.auth_reveal;
        for f in [&self.auth_pass, &self.auth_token, &self.auth_value] {
            f.update(cx, |s, cx| s.set_masked(masked, window, cx));
        }
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
        self.base = req.clone();
        self.method = req.method;
        let url = req.url.clone();
        self.url.update(cx, |s, cx| s.set_value(url, window, cx));
        let name = req.name.clone();
        self.name.update(cx, |s, cx| s.set_value(name, window, cx));
        // body: mode, raw text, urlencoded rows, form-data rows
        let (mode, kind, body_text) = match &req.body {
            Body::None => (BodyMode::Raw, RawKind::Json, String::new()),
            Body::Json { raw } => (BodyMode::Raw, RawKind::Json, raw.clone()),
            Body::Xml { raw } => (BodyMode::Raw, RawKind::Xml, raw.clone()),
            Body::Html { raw } => (BodyMode::Raw, RawKind::Html, raw.clone()),
            Body::Text { raw } => (BodyMode::Raw, RawKind::Text, raw.clone()),
            Body::UrlEncoded { .. } => (BodyMode::UrlEncoded, RawKind::Json, String::new()),
            Body::FormData { .. } => (BodyMode::FormData, RawKind::Json, String::new()),
        };
        self.body_mode = mode;
        self.raw_kind = kind;
        self.body
            .update(cx, |s, cx| s.set_value(body_text, window, cx));
        let mut form: Vec<KvRow> = match &req.body {
            Body::UrlEncoded { fields } => fields
                .iter()
                .map(|kv| {
                    let mut r = Workspace::new_row(window, cx, &kv.key, &kv.value);
                    r.enabled = kv.enabled;
                    r
                })
                .collect(),
            _ => Vec::new(),
        };
        form.push(Workspace::new_row(window, cx, "", ""));
        let blank = FormField {
            key: String::new(),
            value: FormValue::Text {
                value: String::new(),
            },
            enabled: true,
        };
        let mut multipart: Vec<FormRow> = match &req.body {
            Body::FormData { fields } => fields
                .iter()
                .map(|f| Workspace::new_form_row(window, cx, f))
                .collect(),
            _ => Vec::new(),
        };
        multipart.push(Workspace::new_form_row(window, cx, &blank));
        self.form = form;
        self.multipart = multipart;

        // auth: fill the matching fields, clear the rest
        let (kind, user, pass, token, key, value, in_query) = match &req.auth {
            Auth::None => (AuthKind::None, "", "", "", "", "", false),
            Auth::Basic { username, password } => (
                AuthKind::Basic,
                username.as_str(),
                password.as_str(),
                "",
                "",
                "",
                false,
            ),
            Auth::Bearer { token } => (AuthKind::Bearer, "", "", token.as_str(), "", "", false),
            Auth::ApiKey {
                key,
                value,
                location,
            } => (
                AuthKind::ApiKey,
                "",
                "",
                "",
                key.as_str(),
                value.as_str(),
                *location == ApiKeyLocation::Query,
            ),
            Auth::Jwt { .. } | Auth::Custom { .. } => (AuthKind::Kept, "", "", "", "", "", false),
        };
        self.auth_kind = kind;
        self.auth_in_query = in_query;
        self.auth_reveal = false;
        for (input, v) in [
            (&self.auth_user, user),
            (&self.auth_pass, pass),
            (&self.auth_token, token),
            (&self.auth_key, key),
            (&self.auth_value, value),
        ] {
            let v = v.to_string();
            input.update(cx, |s, cx| s.set_value(v, window, cx));
        }
        self.set_auth_masked(window, cx);

        // settings
        let t = req.settings.timeout_ms.to_string();
        self.timeout.update(cx, |s, cx| s.set_value(t, window, cx));
        self.follow_redirects = req.settings.follow_redirects;
        self.verify_tls = req.settings.verify_tls;

        // scripts
        let (pre, test) = (req.pre_request_script.clone(), req.test_script.clone());
        self.pre_script
            .update(cx, |s, cx| s.set_value(pre, window, cx));
        self.test_script
            .update(cx, |s, cx| s.set_value(test, window, cx));
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
        self.set_view_data(ViewData::new(data));
    }

    fn set_view_data(&mut self, v: ViewData) {
        self.view = Some(v);
        self.scroll_x = 0.0;
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    /// Recompute Find matches when the query or the body changed.
    fn refresh_matches(&mut self, cx: &App) {
        let query = self.find.read(cx).value().to_string();
        let ptr = self
            .view
            .as_ref()
            .map(|v| Arc::as_ptr(&v.data) as usize)
            .unwrap_or(0);
        if self.matches_key.0 == ptr && self.matches_key.1 == query {
            return;
        }
        self.matches = match &self.view {
            Some(v) if !query.is_empty() => find_all(&v.data, &query, FIND_MAX),
            _ => Vec::new(),
        };
        self.match_ix = 0;
        self.scroll_to_match = !self.matches.is_empty();
        self.matches_key = (ptr, query);
    }

    /// Bring the current match into view: scroll the list to its row and, without wrap, the
    /// text sideways so the match isn't off to the right.
    fn reveal_match(&mut self, avail: f32) {
        self.scroll_to_match = false;
        let (Some(v), Some(&off)) = (self.view.as_ref(), self.matches.get(self.match_ix)) else {
            return;
        };
        let rows = v.rows(self.wrap);
        let row = rows.row_of(off);
        self.list_scroll.scroll_to_item(row, ScrollStrategy::Center);
        if !self.wrap {
            let start = rows.starts[row] as usize;
            let x = count_chars(&v.data[start..off]) as f32 * char_w();
            if x < self.scroll_x || x > self.scroll_x + avail - 60.0 {
                self.scroll_x = (x - avail / 3.0).max(0.0);
            }
        }
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
                let ct = content_type(&resp.headers);
                self.view_gen += 1;
                match resp.body {
                    ResponseBody::Memory(b) => {
                        self.body_file = None;
                        self.set_view(display_body(&b, &ct));
                    }
                    ResponseBody::File { path, len } => {
                        // Large bodies were streamed to disk; load them off the UI thread.
                        self.body_file = Some(path.clone());
                        if len <= VIEW_MAX_BYTES {
                            self.loading_file = Some(path);
                            self.set_view(
                                format!("Loading the {} response body…", human(len)).into_bytes(),
                            );
                        } else {
                            self.set_view(
                                format!(
                                    "The response body is {}, too large to show here.\nIt was saved to:\n{}\n\nUse Save… to keep a copy.",
                                    human(len),
                                    path.display()
                                )
                                .into_bytes(),
                            );
                        }
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
    // ---- history ----
    side_view: SideView,
    /// Shared with the network threads, which record each send as it finishes.
    history: Option<Arc<Mutex<History>>>,
    hist_items: Vec<Summary>,
    hist_search: Entity<InputState>,
    /// Why history is unavailable or the last history operation failed.
    hist_error: Option<String>,
    confirm_clear: bool,
    /// Environment export includes secret values (off by default, FR-ENV-04).
    export_secrets: bool,
    // ---- code generation ----
    code_lang: Lang,
    /// Snippets keep `{{variables}}` instead of their values (safe to share).
    code_keep_vars: bool,
    code_copied: bool,
    // ---- session (hot exit) ----
    /// False when the session file can't be written, or is from a newer app (never overwrite).
    session_enabled: bool,
    /// What was last written, so unchanged sessions aren't rewritten every few seconds.
    last_session: Option<Session>,
    closed: Vec<ClosedTab>,
    runner: Runner,
    // ---- cookie jar ----
    cookies: Arc<CookieJar>,
    /// Why cookies couldn't be loaded or saved.
    cookie_note: Option<String>,
    confirm_clear_cookies: bool,
    _subs: Vec<Subscription>,
}

impl Workspace {
    fn new(rt: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let env_name = cx.new(|cx| InputState::new(window, cx));
        let (store, store_err) = match Store::open_default() {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let (cookies, cookie_note) = match &store {
            Some(s) => match CookieJar::load(&s.data_dir().join("cookies.json")) {
                Ok(j) => (j, None),
                Err(e) => (
                    CookieJar::new(),
                    Some(format!(
                        "Saved cookies could not be read, starting empty: {e}"
                    )),
                ),
            },
            None => (CookieJar::new(), None),
        };
        let (history, hist_error) = match &store {
            Some(s) => match History::open(&s.data_dir().join("history.db")) {
                Ok(h) => (Some(Arc::new(Mutex::new(h))), None),
                Err(e) => (None, Some(format!("History is off: {e}"))),
            },
            None => (
                None,
                Some("History is off: storage is unavailable".to_string()),
            ),
        };
        let hist_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search URL or name"));
        let sub = cx.subscribe_in(
            &hist_search,
            window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_history(cx);
                    cx.notify();
                }
            },
        );
        let (session, session_note, session_enabled) = match &store {
            Some(s) => match s.load_session() {
                Ok(x) => (x, None, true),
                Err(e @ StoreError::TooNew { .. }) => {
                    (None, Some(format!("Last session not restored: {e}")), false)
                }
                Err(e) => (
                    None,
                    Some(format!(
                        "Last session could not be read, starting fresh: {e}"
                    )),
                    true,
                ),
            },
            None => (None, None, false),
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
            side_view: SideView::Collections,
            history,
            hist_items: Vec::new(),
            hist_search,
            hist_error,
            confirm_clear: false,
            export_secrets: false,
            code_lang: Lang::Curl,
            code_keep_vars: false,
            code_copied: false,
            session_enabled,
            last_session: None,
            closed: Vec::new(),
            cookies: Arc::new(cookies),
            cookie_note,
            confirm_clear_cookies: false,
            runner: Runner {
                target: None,
                iterations: cx.new(|cx| InputState::new(window, cx).default_value("1")),
                delay: cx.new(|cx| InputState::new(window, cx).default_value("0")),
                stop_on_fail: false,
                rows: Vec::new(),
                planned: 0,
                cancel: None,
                started: None,
                elapsed: None,
                notes: Vec::new(),
                copied: false,
            },
            _subs: vec![sub],
        };
        me.refresh_sidebar();
        me.reload_envs();
        me.load_scope(Scope::Globals, window, cx);
        if let Some(s) = session {
            me.restore_session(s, window, cx);
        }
        if let Some(note) = session_note {
            me.tab_mut().status = Status::Error(note);
        }

        // Keep the session file current: on a timer (cheap: written only when it changed),
        // when the window closes, and when the app quits.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(SESSION_SAVE_EVERY).await;
            if this
                .update(cx, |this, cx| this.persist_session(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_window, cx| {
            if let Some(ws) = weak.upgrade() {
                ws.update(cx, |this, cx| this.persist_session(cx));
            }
            true
        });
        me._subs.push(cx.on_app_quit(|this, cx| {
            this.persist_session(cx);
            async {}
        }));
        me
    }

    // ---------------- session restore (hot exit) ----------------

    fn session(&self, cx: &App) -> Session {
        Session {
            active: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|t| SessionTab {
                    path: t.current_path.clone(),
                    save_dir: t.save_dir.clone(),
                    unsaved: t.is_dirty(cx),
                    draft: t.current_request(cx),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn persist_session(&mut self, cx: &App) {
        if !self.session_enabled {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        let s = self.session(cx);
        if self.last_session.as_ref() == Some(&s) {
            return;
        }
        match store.save_session(&s) {
            Ok(()) => self.last_session = Some(s),
            Err(e) => {
                // Say it once and stop trying, rather than every few seconds.
                self.session_enabled = false;
                self.tab_mut().status =
                    Status::Error(format!("Open tabs won't be restored next time: {e}"));
            }
        }
    }

    /// Rebuild the tabs from the last session. Each tab shows what its editor showed; a tab
    /// that had unsaved edits is still marked unsaved against its file.
    fn restore_session(&mut self, s: Session, window: &mut Window, cx: &mut Context<Self>) {
        let mut tabs = Vec::new();
        for st in s.tabs {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            let mut tab = RequestTab::create(id, true, window, cx);
            tab.save_dir = st.save_dir;
            match st.path {
                Some(path) => match Store::load_request(&path) {
                    Ok(file) => {
                        tab.apply_request(file, window, cx);
                        tab.current_path = Some(path);
                        if st.unsaved {
                            let saved = tab.saved.clone();
                            tab.apply_request(st.draft, window, cx);
                            tab.saved = saved;
                        }
                    }
                    Err(_) => {
                        // Moved or deleted since: keep the user's last version, unsaved.
                        tab.apply_request(st.draft, window, cx);
                        tab.saved = Request::default();
                        tab.status = Status::Info(format!(
                            "{} was moved or deleted; this is your last version (not saved)",
                            path.display()
                        ));
                    }
                },
                None => {
                    tab.apply_request(st.draft, window, cx);
                    if st.unsaved {
                        tab.saved = Request::default();
                    }
                }
            }
            tabs.push(tab);
        }
        if tabs.is_empty() {
            return;
        }
        self.active = s.active.min(tabs.len() - 1);
        self.tabs = tabs;
    }

    /// Cmd/Ctrl+Shift+T: bring back the most recently closed tab, edits included.
    fn reopen_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.closed.pop() else {
            self.tab_mut().status = Status::Info("No closed tabs to reopen".into());
            cx.notify();
            return;
        };
        if let Some(ix) = c.path.as_ref().and_then(|p| {
            self.tabs
                .iter()
                .position(|t| t.current_path.as_ref() == Some(p))
        }) {
            self.activate_tab(ix, window, cx);
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let mut tab = RequestTab::create(id, true, window, cx);
        tab.apply_request(c.draft, window, cx);
        tab.saved = c.saved;
        tab.current_path = c.path.filter(|p| p.exists());
        tab.save_dir = c.save_dir;
        tab.status = Status::Info("Reopened".into());
        if self.tab().is_pristine(cx) && self.tabs.len() == 1 {
            self.tabs[0] = tab;
            self.active = 0;
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
        self.refocus(window);
        cx.notify();
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

    fn new_form_row(window: &mut Window, cx: &mut Context<Self>, f: &FormField) -> FormRow {
        let (is_file, v) = match &f.value {
            FormValue::Text { value } => (false, value.clone()),
            FormValue::File { path } => (true, path.clone()),
        };
        let key = cx.new(|cx| InputState::new(window, cx).default_value(f.key.clone()));
        let value = cx.new(|cx| InputState::new(window, cx).default_value(v));
        FormRow {
            enabled: f.enabled,
            is_file,
            key,
            value,
        }
    }

    /// Pick a file for form-data row `ix` of tab `id`.
    fn choose_form_file(
        &mut self,
        id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(p) = paths.into_iter().next() {
                    this.update_in(cx, |this, window, cx| {
                        if let Some(row) = this
                            .tabs
                            .iter()
                            .find(|t| t.id == id)
                            .and_then(|t| t.multipart.get(ix))
                        {
                            let p = p.to_string_lossy().into_owned();
                            row.value.update(cx, |s, cx| s.set_value(p, window, cx));
                        }
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
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
        let t = &self.tabs[ix];
        // Untouched blank tabs aren't worth reopening.
        if !t.is_pristine(cx) {
            self.closed.push(ClosedTab {
                path: t.current_path.clone(),
                save_dir: t.save_dir.clone(),
                draft: t.current_request(cx),
                saved: t.saved.clone(),
            });
            if self.closed.len() > CLOSED_TABS_MAX {
                self.closed.remove(0);
            }
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
            )
            .child(div().flex_1())
            .child(
                Button::new("env-import")
                    .label("Import…")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_import(window, cx))),
            )
            .child(
                seg_btn(
                    "env-secrets",
                    if self.export_secrets {
                        "Include secrets: on".to_string()
                    } else {
                        "Include secrets: off".to_string()
                    },
                    self.export_secrets,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.export_secrets = !this.export_secrets;
                    cx.notify();
                })),
            )
            .child(
                Button::new("env-export")
                    .label("Export…")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_export_env(window, cx))),
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

    /// The Code view: language picker, values-or-placeholders toggle, Copy, and the snippet.
    fn render_code(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut bar = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_2();
        for (i, lang) in Lang::ALL.into_iter().enumerate() {
            let b = Button::new(("code-lang", i)).label(lang.label());
            let b = if self.code_lang == lang {
                b.primary()
            } else {
                b
            };
            bar = bar.child(b.on_click(cx.listener(move |this, _ev, _window, cx| {
                this.code_lang = lang;
                this.code_copied = false;
                cx.notify();
            })));
        }
        bar = bar
            .child(div().flex_1())
            .child(
                Button::new("code-vars")
                    .label(if self.code_keep_vars {
                        "Variables: {{kept}}"
                    } else {
                        "Variables: values"
                    })
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.code_keep_vars = !this.code_keep_vars;
                        this.code_copied = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("code-copy")
                    .label(if self.code_copied {
                        "Copied ✓"
                    } else {
                        "Copy"
                    })
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        if let Ok((code, _)) = this.code_snippet(cx) {
                            cx.write_to_clipboard(ClipboardItem::new_string(code));
                            this.code_copied = true;
                            cx.notify();
                        }
                    })),
            );

        let (lines, warn): (Vec<String>, Option<String>) = match self.code_snippet(cx) {
            Ok((code, secret)) => (
                code.lines().map(|l| l.replace('\t', "    ")).collect(),
                secret.then(|| {
                    "This snippet contains secret values. Switch to \"Variables: {{kept}}\" before sharing it."
                        .to_string()
                }),
            ),
            Err(e) => (Vec::new(), Some(format!("Can't generate code yet: {e}"))),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bar)
            .children(warn.map(|w| div().px_2().pb_1().text_color(c(0xf9e2af)).child(w)))
            .child(
                div()
                    .id("code-view")
                    .flex_1()
                    .overflow_scroll()
                    .px_2()
                    .children(
                        lines
                            .into_iter()
                            .map(|l| div().h(px(18.)).flex_none().whitespace_nowrap().child(l)),
                    ),
            )
    }

    fn render_body(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mut modes = div().flex().flex_row().flex_wrap().gap_1();
        for (i, (mode, label)) in [
            (BodyMode::None, "None"),
            (BodyMode::Raw, "Raw"),
            (BodyMode::UrlEncoded, "urlencoded"),
            (BodyMode::FormData, "form-data"),
        ]
        .into_iter()
        .enumerate()
        {
            modes = modes.child(
                seg_btn_ix("body-mode", i, label, tab.body_mode == mode).on_click(cx.listener(
                    move |this, _ev, _window, cx| {
                        this.tab_mut().body_mode = mode;
                        cx.notify();
                    },
                )),
            );
        }
        if tab.body_mode == BodyMode::Raw {
            modes = modes.child(div().w(px(12.)));
            for (i, kind) in RawKind::ALL.into_iter().enumerate() {
                modes = modes.child(
                    seg_btn_ix("raw-kind", i, kind.label(), tab.raw_kind == kind).on_click(
                        cx.listener(move |this, _ev, _window, cx| {
                            this.tab_mut().raw_kind = kind;
                            cx.notify();
                        }),
                    ),
                );
            }
        }
        let note = (!has_body(tab.method)).then(|| {
            div().text_color(muted).child(format!(
                "{} requests don't send a body.",
                tab.method.as_str()
            ))
        });
        let editor = match tab.body_mode {
            BodyMode::None => div().text_color(muted).child("No body.").into_any_element(),
            BodyMode::Raw => div()
                .flex_1()
                .min_h(px(120.))
                .child(Input::new(&tab.body).h_full())
                .into_any_element(),
            BodyMode::UrlEncoded => self.render_kv(tab, Which::Form, cx).into_any_element(),
            BodyMode::FormData => self.render_multipart(tab, cx).into_any_element(),
        };
        div()
            .p_2()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(modes)
            .children(note)
            .child(editor)
    }

    fn render_multipart(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let id = tab.id;
        let mut col = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .text_color(muted)
                .child(div().w(px(24.)).flex_none())
                .child(div().flex_1().child("Key"))
                .child(div().w(px(64.)).flex_none().child("Type"))
                .child(div().flex_1().child("Value / file"))
                .child(div().w(px(36.)).flex_none()),
        );
        for (ix, row) in tab.multipart.iter().enumerate() {
            let choose = row.is_file.then(|| {
                Button::new(("fd-choose", ix))
                    .label("Choose…")
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.choose_form_file(id, ix, window, cx)
                    }))
            });
            col =
                col.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(24.)).flex_none().child(
                            Checkbox::new(("fd-en", ix)).checked(row.enabled).on_click(
                                cx.listener(move |this, checked: &bool, _window, cx| {
                                    if let Some(r) = this.tab_mut().multipart.get_mut(ix) {
                                        r.enabled = *checked;
                                    }
                                    cx.notify();
                                }),
                            ),
                        ))
                        .child(div().flex_1().child(Input::new(&row.key)))
                        .child(
                            div().w(px(64.)).flex_none().child(
                                Button::new(("fd-type", ix))
                                    .label(if row.is_file { "File" } else { "Text" })
                                    .on_click(cx.listener(move |this, _ev, _window, cx| {
                                        if let Some(r) = this.tab_mut().multipart.get_mut(ix) {
                                            r.is_file = !r.is_file;
                                        }
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .child(div().flex_1().child(Input::new(&row.value)))
                                .children(choose),
                        )
                        .child(div().w(px(36.)).flex_none().child(
                            Button::new(("fd-del", ix)).label("✕").on_click(cx.listener(
                                move |this, _ev, _window, cx| {
                                    let rows = &mut this.tab_mut().multipart;
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
            div().child(Button::new("fd-add").label("+ Add").on_click(cx.listener(
                |this, _ev, window, cx| {
                    let row = Self::new_form_row(
                        window,
                        cx,
                        &FormField {
                            key: String::new(),
                            value: FormValue::Text {
                                value: String::new(),
                            },
                            enabled: true,
                        },
                    );
                    this.tab_mut().multipart.push(row);
                    cx.notify();
                },
            ))),
        )
    }

    fn render_auth(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mut kinds = div().flex().flex_row().flex_wrap().gap_1();
        for (i, kind) in AuthKind::EDITABLE.into_iter().enumerate() {
            kinds = kinds.child(
                seg_btn_ix("auth-kind", i, kind.label(), tab.auth_kind == kind).on_click(
                    cx.listener(move |this, _ev, _window, cx| {
                        this.tab_mut().auth_kind = kind;
                        cx.notify();
                    }),
                ),
            );
        }
        if tab.kept_auth().is_some() {
            kinds = kinds.child(seg_btn_ix(
                "auth-kind",
                AuthKind::EDITABLE.len(),
                AuthKind::Kept.label(),
                tab.auth_kind == AuthKind::Kept,
            ));
        }
        let row = |label: &'static str, input: &Entity<InputState>| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().w(px(110.)).flex_none().text_color(muted).child(label))
                .child(div().flex_1().child(Input::new(input)))
        };
        let reveal = Button::new("auth-reveal")
            .label(if tab.auth_reveal { "Hide" } else { "Show" })
            .on_click(cx.listener(|this, _ev, window, cx| {
                let tab = this.tab_mut();
                tab.auth_reveal = !tab.auth_reveal;
                tab.set_auth_masked(window, cx);
                cx.notify();
            }));
        let fields = match tab.auth_kind {
            AuthKind::None => div()
                .text_color(muted)
                .child("No authorization is added to the request.")
                .into_any_element(),
            AuthKind::Basic => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(row("Username", &tab.auth_user))
                .child(row("Password", &tab.auth_pass))
                .child(div().child(reveal))
                .child(
                    div()
                        .text_color(muted)
                        .child("Sent as Authorization: Basic <base64 of username:password>."),
                )
                .into_any_element(),
            AuthKind::Bearer => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(row("Token", &tab.auth_token))
                .child(div().child(reveal))
                .child(
                    div()
                        .text_color(muted)
                        .child("Sent as Authorization: Bearer <token>."),
                )
                .into_any_element(),
            AuthKind::ApiKey => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(row("Key", &tab.auth_key))
                .child(row("Value", &tab.auth_value))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .child(
                            seg_btn_ix("auth-in", 0, "Header", !tab.auth_in_query).on_click(
                                cx.listener(|this, _ev, _window, cx| {
                                    this.tab_mut().auth_in_query = false;
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            seg_btn_ix("auth-in", 1, "Query param", tab.auth_in_query).on_click(
                                cx.listener(|this, _ev, _window, cx| {
                                    this.tab_mut().auth_in_query = true;
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(reveal),
                )
                .into_any_element(),
            AuthKind::Kept => div()
                .text_color(muted)
                .child(format!(
                    "This request uses {} from its file. It is sent and saved unchanged; pick another type above to replace it.",
                    tab.kept_auth().unwrap_or("an auth type")
                ))
                .into_any_element(),
        };
        div()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(kinds)
            .child(fields)
            .child(
                div()
                    .text_color(muted)
                    .child("{{variables}} work in every field. A header you add yourself with the same name wins."),
            )
    }

    fn has_scripts(tab: &RequestTab, cx: &App) -> bool {
        !tab.pre_script.read(cx).value().trim().is_empty()
            || !tab.test_script.read(cx).value().trim().is_empty()
    }

    fn render_scripts(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let filled = |e: &Entity<InputState>| !e.read(cx).value().trim().is_empty();
        let label = |name: &str, e: &Entity<InputState>| {
            if filled(e) {
                format!("{name} ●")
            } else {
                name.to_string()
            }
        };
        let show_tests = tab.scripts_show_tests;
        let snippets: &[(&str, &str)] = if show_tests {
            &[
                (
                    "Status is 200",
                    "pm.test(\"Status is 200\", () => pm.response.to.have.status(200));\n",
                ),
                (
                    "Fast response",
                    "pm.test(\"Response is fast\", () => pm.expect(pm.response.responseTime).to.be.below(500));\n",
                ),
                (
                    "JSON field",
                    "pm.test(\"Has an id\", () => pm.expect(pm.response.json()).to.have.property(\"id\"));\n",
                ),
                (
                    "Save to env",
                    "const json = pm.response.json();\npm.environment.set(\"id\", json.id);\n",
                ),
            ]
        } else {
            &[
                (
                    "Set variable",
                    "pm.variables.set(\"ts\", String(Date.now()));\n",
                ),
                (
                    "Add header",
                    "pm.request.headers.upsert({ key: \"X-Request-Id\", value: String(Date.now()) });\n",
                ),
                ("Log", "console.log(pm.request.url);\n"),
            ]
        };
        let mut bar = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(
                seg_btn_ix(
                    "script-kind",
                    0,
                    &label("Pre-request", &tab.pre_script),
                    !show_tests,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().scripts_show_tests = false;
                    cx.notify();
                })),
            )
            .child(
                seg_btn_ix(
                    "script-kind",
                    1,
                    &label("Tests", &tab.test_script),
                    show_tests,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().scripts_show_tests = true;
                    cx.notify();
                })),
            )
            .child(div().w(px(12.)))
            .child(div().text_color(muted).child("Insert:"));
        for (i, (name, code)) in snippets.iter().enumerate() {
            let code = code.to_string();
            bar = bar.child(
                Button::new(("snippet", i))
                    .label(name.to_string())
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        let tab = this.tab_mut();
                        let editor = if tab.scripts_show_tests {
                            tab.test_script.clone()
                        } else {
                            tab.pre_script.clone()
                        };
                        let cur = editor.read(cx).value().to_string();
                        let text = if cur.is_empty() || cur.ends_with('\n') {
                            format!("{cur}{code}")
                        } else {
                            format!("{cur}\n{code}")
                        };
                        editor.update(cx, |s, cx| s.set_value(text, window, cx));
                        cx.notify();
                    })),
            );
        }
        let editor = if show_tests {
            &tab.test_script
        } else {
            &tab.pre_script
        };
        div()
            .p_2()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h(px(120.))
                    .child(Input::new(editor).h_full()),
            )
            .child(div().text_color(muted).child(if show_tests {
                "Runs after the response arrives: pm.response, pm.test(name, fn), pm.expect(value)…"
            } else {
                "Runs before sending: pm.environment / pm.globals / pm.variables .get/.set, pm.request.headers…"
            }))
    }

    fn render_tests(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let (green, red, amber) = (c(0xa6e3a1), c(0xf38ba8), c(0xf9e2af));
        let mut col = div()
            .id("tests-view")
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        let Some(rep) = &tab.report else {
            return col.child(div().text_color(muted).child(
                "No scripts ran. Write tests on the request's Scripts tab, e.g. pm.test(\"ok\", () => pm.response.to.be.ok), then Send.",
            ));
        };
        for (what, out) in [("Pre-request script", &rep.pre), ("Test script", &rep.test)] {
            if let Some(o) = out {
                if let Some(e) = &o.error {
                    col = col.child(div().text_color(red).child(format!("{what}: {e}")));
                }
            }
        }
        for n in &rep.notes {
            col = col.child(div().text_color(amber).child(n.clone()));
        }
        if let Some(t) = &rep.test {
            for r in &t.tests {
                let (mark, colour) = if r.skipped {
                    ("○", muted)
                } else if r.passed {
                    ("✓", green)
                } else {
                    ("✗", red)
                };
                col = col.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(div().w(px(16.)).text_color(colour).child(mark))
                        .child(r.name.clone()),
                );
                if let Some(e) = &r.error {
                    if !r.passed {
                        col = col.child(div().pl(px(24.)).text_color(muted).child(e.clone()));
                    }
                }
            }
        }
        let logs: Vec<_> = [&rep.pre, &rep.test]
            .into_iter()
            .flatten()
            .flat_map(|o| o.logs.iter())
            .collect();
        if !logs.is_empty() {
            col = col.child(div().pt_2().text_color(muted).child("CONSOLE"));
            for l in logs {
                let colour = match l.level {
                    LogLevel::Warn => amber,
                    LogLevel::Error => red,
                    LogLevel::Log | LogLevel::Info => cx.theme().foreground,
                };
                col = col.child(
                    div()
                        .font_family(mono())
                        .text_color(colour)
                        .child(l.text.clone()),
                );
            }
        }
        let timing: Vec<String> = [("pre-request", &rep.pre), ("tests", &rep.test)]
            .into_iter()
            .filter_map(|(n, o)| {
                o.as_ref()
                    .map(|o| format!("{n} {} ms", o.elapsed.as_millis()))
            })
            .collect();
        col.child(div().pt_2().text_color(muted).child(timing.join(" · ")))
    }

    fn render_settings(&self, tab: &RequestTab, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        div()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .text_color(muted)
                            .child("Timeout (ms)"),
                    )
                    .child(div().w(px(140.)).flex_none().child(Input::new(&tab.timeout)))
                    .child(div().text_color(muted).child("0 = no timeout")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        seg_btn_ix(
                            "set-redirect",
                            0,
                            if tab.follow_redirects {
                                "Follow redirects: on"
                            } else {
                                "Follow redirects: off"
                            },
                            tab.follow_redirects,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            let tab = this.tab_mut();
                            tab.follow_redirects = !tab.follow_redirects;
                            cx.notify();
                        })),
                    )
                    .child(
                        seg_btn_ix(
                            "set-tls",
                            0,
                            if tab.verify_tls {
                                "Verify TLS certificates: on"
                            } else {
                                "Verify TLS certificates: off"
                            },
                            tab.verify_tls,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            let tab = this.tab_mut();
                            tab.verify_tls = !tab.verify_tls;
                            cx.notify();
                        })),
                    ),
            )
            .children((!tab.verify_tls).then(|| {
                div()
                    .text_color(c(0xf9e2af))
                    .child("Certificate checks are off for this request: only use this for local or test servers.")
            }))
    }

    fn render_kv(&self, tab: &RequestTab, which: Which, cx: &Context<Self>) -> impl IntoElement {
        let (rows, en_id, del_id, add_id) = match which {
            Which::Params => (&tab.params, "p-en", "p-del", "p-add"),
            Which::Headers => (&tab.headers, "h-en", "h-del", "h-add"),
            Which::Form => (&tab.form, "f-en", "f-del", "f-add"),
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
        if !self.leave_env_page(cx) {
            return;
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
        let req = tab.current_request(cx);
        tab.copied = false;
        let token = CancellationToken::new();
        tab.cancel = Some(token.clone());
        tab.status = Status::Loading;
        cx.notify();

        let env_name = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.name.clone());
        let (tx, rx) = async_channel::bounded(1);
        let history = self.history.clone();
        let cookies = self.cookies.clone();
        self.rt.spawn(async move {
            let (out, _) = execute(req, vars, env_name, token, history, cookies).await;
            let _ = tx.send(out).await;
        });

        cx.spawn(async move |this, cx| {
            if let Ok(out) = rx.recv().await {
                this.update(cx, |this, cx| this.finish_send(id, out, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Show a finished send in its tab and keep what its scripts changed.
    fn finish_send(&mut self, id: u64, out: SendOutcome, cx: &mut Context<Self>) {
        let SendOutcome {
            sent,
            unresolved,
            pre,
            test,
            env_before,
            globals_before,
            hist_err,
        } = out;
        let mut notes = Vec::new();
        // The last script that ran holds the final variables.
        let last = test.as_ref().or(pre.as_ref());
        if let Some(o) = last {
            if let Some(n) =
                self.keep_script_vars(&env_before, &o.environment, &globals_before, &o.globals)
            {
                notes.push(n);
            }
        }
        // The tab may have been closed while the request was running.
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
            t.cancel = None;
            match sent {
                Ok(result) => {
                    t.apply(result);
                    t.warning = (!unresolved.is_empty())
                        .then(|| format!("Unresolved variables: {}", unresolved.join(", ")));
                }
                Err(e) => {
                    t.status = Status::Error(e);
                    t.warning = None;
                }
            }
            t.report =
                (pre.is_some() || test.is_some()).then_some(ScriptReport { pre, test, notes });
            if let Some(path) = t.loading_file.take() {
                let generation = t.view_gen;
                self.load_body_file(id, generation, path, cx);
            }
        }
        self.save_cookies_if_changed();
        if let Some(e) = hist_err {
            self.hist_error = Some(format!("Not saved to history: {e}"));
        }
        if self.side_view == SideView::History {
            self.refresh_history(cx);
        }
        cx.notify();
    }

    /// Save the variable changes scripts made to the active environment and the globals.
    /// Returns a note when something couldn't be kept.
    fn keep_script_vars(
        &mut self,
        env_before: &VarMap,
        env_after: &VarMap,
        globals_before: &VarMap,
        globals_after: &VarMap,
    ) -> Option<String> {
        let store = self.store.clone()?;
        let mut note = None;
        if let Some(new) = merge_script_vars(&self.globals, globals_before, globals_after) {
            match store.save_globals(&new) {
                Ok(()) => self.globals = new,
                Err(e) => note = Some(format!("Global variables not saved: {e}")),
            }
        }
        if env_before != env_after {
            match self
                .active_env
                .and_then(|i| self.envs.get(i).map(|x| (i, x.clone())))
            {
                Some((ix, (env, path))) => {
                    if let Some(vars) = merge_script_vars(&env.variables, env_before, env_after) {
                        let env = Environment {
                            name: env.name,
                            variables: vars,
                        };
                        match store.save_environment(&env, Some(&path)) {
                            Ok(p) => self.envs[ix] = (env, p),
                            Err(e) => note = Some(format!("Environment not saved: {e}")),
                        }
                    }
                }
                None => {
                    note = Some(
                        "pm.environment changes were not kept: no environment is active".into(),
                    )
                }
            }
        }
        note
    }

    // ---------------- big bodies, Save…, Find ----------------

    /// Read a spilled body and index it on a background thread, then show it, unless the tab
    /// got a newer response meanwhile.
    fn load_body_file(&mut self, id: u64, generation: u64, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { std::fs::read(&path).map(ViewData::new) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(t) = this
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == id && t.view_gen == generation)
                {
                    match loaded {
                        Ok(v) => t.set_view_data(v),
                        Err(e) => t.set_view(
                            format!("Could not read the saved response body: {e}").into_bytes(),
                        ),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Save the response body: the original bytes for big (spilled) bodies, else what's shown.
    fn on_save_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab();
        let Some(view) = tab.view.as_ref() else {
            return;
        };
        let ct = content_type(&tab.resp_headers).to_ascii_lowercase();
        let ext = if ct.contains("json") {
            "json"
        } else if ct.contains("xml") {
            "xml"
        } else if ct.contains("html") {
            "html"
        } else {
            "txt"
        };
        let source = tab.body_file.clone();
        let data = view.data.clone();
        let rx = cx.prompt_for_new_path(&self.export_dir(), Some(&format!("response.{ext}")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let written = match &source {
                Some(src) => std::fs::copy(src, &path).map(|_| ()),
                None => std::fs::write(&path, data.as_slice()),
            };
            this.update(cx, |this, cx| {
                this.tab_mut().status = match written {
                    Ok(()) => Status::Info(format!("Response body saved to {}", path.display())),
                    Err(e) => Status::Error(format!("Could not write {}: {e}", path.display())),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        tab.find_open = true;
        tab.res_tab = ResTab::Body;
        tab.scroll_to_match = !tab.matches.is_empty();
        tab.find.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn find_step(&mut self, id: u64, delta: isize, cx: &mut Context<Self>) {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
            let n = t.matches.len();
            if n > 0 {
                t.match_ix = (t.match_ix as isize + delta).rem_euclid(n as isize) as usize;
                t.scroll_to_match = true;
                cx.notify();
            }
        }
    }

    // ---------------- history (FR-HIS) ----------------

    fn refresh_history(&mut self, cx: &App) {
        let Some(h) = self.history.clone() else {
            self.hist_items.clear();
            return;
        };
        let filter = self.hist_search.read(cx).value().to_string();
        let listed = match h.lock() {
            Ok(h) => h.list(&filter, HISTORY_LIST_MAX).map_err(|e| e.to_string()),
            Err(_) => Err("history is unavailable".to_string()),
        };
        match listed {
            Ok(items) => self.hist_items = items,
            Err(e) => self.hist_error = Some(e),
        }
    }

    fn show_side(&mut self, view: SideView, cx: &mut Context<Self>) {
        self.side_view = view;
        self.confirm_clear = false;
        if view == SideView::History {
            self.refresh_history(cx);
        }
        cx.notify();
    }

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.confirm_clear = false;
        if let Some(h) = self.history.clone() {
            let cleared = match h.lock() {
                Ok(mut h) => h.clear().map_err(|e| e.to_string()),
                Err(_) => Err("history is unavailable".to_string()),
            };
            match cleared {
                Ok(()) => self.hist_error = None,
                Err(e) => self.hist_error = Some(e),
            }
        }
        self.refresh_history(cx);
        cx.notify();
    }

    /// Open a history entry in a tab: the request as it was sent plus the stored response.
    /// The tab is not linked to any file; Save puts it into a collection as a new request.
    fn open_history(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.leave_env_page(cx) {
            return;
        }
        let Some(h) = self.history.clone() else {
            return;
        };
        let entry = match h.lock() {
            Ok(h) => h.get(id),
            Err(_) => return,
        };
        let entry = match entry {
            Ok(Some(e)) => e,
            Ok(None) => {
                self.hist_error = Some("That history entry no longer exists".into());
                self.refresh_history(cx);
                cx.notify();
                return;
            }
            Err(e) => {
                self.hist_error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        if !self.tab().is_pristine(cx) {
            self.add_tab(window, cx);
        }
        let s = entry.summary;
        let ct = content_type(&entry.resp_headers);
        let mut headers = entry.resp_headers;
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        let body = match entry.resp_body {
            // A cut-off JSON body can't be pretty-printed reliably; show it as it arrived.
            Some(mut b) if entry.body_truncated => {
                b.extend_from_slice(
                    format!(
                        "\n\n[… history kept only the first {} of {}]",
                        human(b.len() as u64),
                        human(s.size)
                    )
                    .as_bytes(),
                );
                b
            }
            Some(b) => display_body(&b, &ct),
            None if s.status.is_some() => format!(
                "The response body ({}) was too large to keep in history.",
                human(s.size)
            )
            .into_bytes(),
            None => format!(
                "No response: {}",
                s.error.clone().unwrap_or_else(|| "unknown error".into())
            )
            .into_bytes(),
        };
        let tab = self.tab_mut();
        tab.apply_request(entry.request, window, cx);
        tab.current_path = None;
        tab.confirm_close = false;
        tab.copied = false;
        tab.resp_headers = headers;
        tab.set_view(body);
        tab.status = match s.status {
            Some(code) => Status::Done {
                code,
                text: entry.status_text,
                ms: s.elapsed_ms as u128,
                size: s.size,
            },
            None => Status::Error(s.error.unwrap_or_else(|| "No response".into())),
        };
        tab.warning = Some(format!("From history, sent {}", ago(now_ms(), s.ts_ms)));
        cx.notify();
    }

    // ---------------- code generation (FR-GEN) ----------------

    /// Names of enabled secret variables currently in effect (globals + active environment).
    fn secret_names(&self) -> Vec<String> {
        let env = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.variables.as_slice())
            .unwrap_or_default();
        self.globals
            .iter()
            .chain(env)
            .filter(|v| v.secret && v.enabled)
            .map(|v| v.key.clone())
            .collect()
    }

    /// The active tab's request as code in the chosen language, plus whether the snippet
    /// contains resolved secret values.
    fn code_snippet(&self, cx: &App) -> Result<(String, bool), String> {
        let vars = self.variables();
        let req = sendable(self.tab().current_request(cx), &vars);
        let none = Variables::default();
        let built = if self.code_keep_vars {
            build_template(&req, &none)
        } else {
            build(&req, &vars)
        }
        .map_err(|e| e.to_string())?;
        let has_secret = !self.code_keep_vars && {
            let used = build_template(&req, &none)
                .map(|b| b.unresolved)
                .unwrap_or_default();
            self.secret_names().iter().any(|s| used.contains(s))
        };
        Ok((generate(&built, self.code_lang), has_secret))
    }

    /// Open the cURL command on the clipboard as a new (unsaved) request.
    fn on_paste_curl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
        match parse_curl(&text) {
            Ok((req, warnings)) => {
                if !self.tab().is_pristine(cx) {
                    self.add_tab(window, cx);
                }
                let tab = self.tab_mut();
                tab.apply_request(req, window, cx);
                tab.current_path = None;
                // Not saved anywhere yet: show it as unsaved so closing asks first.
                tab.saved = Request::default();
                tab.status = Status::Info(format!(
                    "Imported from the cURL command on the clipboard{}",
                    notes(&warnings)
                ));
            }
            Err(e) => self.tab_mut().status = Status::Error(format!("Clipboard: {e}")),
        }
        cx.notify();
    }

    // ---------------- import / export (FR-IMP) ----------------

    /// Show a message where the user is looking: the environments page or the active tab.
    fn notice(&mut self, s: Status) {
        if self.main_view == MainView::Environments {
            self.status = s;
        } else {
            self.tab_mut().status = s;
        }
    }

    /// The collection the user is working in: the one holding the active tab's file, or the
    /// collection/folder last clicked in the sidebar.
    fn selected_collection(&self) -> Option<(String, PathBuf)> {
        let tab = self.tab();
        let here = tab.current_path.as_ref().or(tab.save_dir.as_ref())?;
        self.items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Collection))
            .find(|i| here.starts_with(&i.path))
            .map(|i| (i.label.clone(), i.path.clone()))
    }

    /// Where the save dialog starts: ~/Downloads if there is one, else the home folder.
    fn export_dir(&self) -> PathBuf {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        match home {
            Some(h) if h.join("Downloads").is_dir() => h.join("Downloads"),
            Some(h) => h,
            None => self
                .store
                .as_ref()
                .map(|s| s.data_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    fn on_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.into_iter().next() {
                    this.update_in(cx, |this, window, cx| this.import_file(&path, window, cx))
                        .ok();
                }
            }
        })
        .detach();
    }

    /// Import a Postman collection, environment or globals file (the kind is detected).
    fn import_file(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        let imported = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| import_postman(&t).map_err(|e| e.to_string()));
        let imported = match imported {
            Ok(i) => i,
            Err(e) => {
                self.notice(Status::Error(format!(
                    "Could not import {}: {e}",
                    path.display()
                )));
                cx.notify();
                return;
            }
        };
        let Some(store) = self.store.clone() else {
            self.notice(Status::Error(
                "Storage is unavailable on this system".into(),
            ));
            cx.notify();
            return;
        };
        match imported {
            Imported::Collection(col, warnings) => match write_collection(&store, &col) {
                Ok((_, sum)) => {
                    self.refresh_sidebar();
                    self.side_view = SideView::Collections;
                    self.notice(Status::Info(format!(
                        "Imported \"{}\": {} request(s), {} folder(s){}",
                        col.name,
                        sum.requests,
                        sum.folders,
                        notes(&warnings)
                    )));
                }
                Err(e) => self.notice(Status::Error(e.to_string())),
            },
            Imported::Environment(mut env, warnings) => {
                // Environment indices shift when one is added; save edits first, then reload.
                if self.main_view == MainView::Environments
                    && self.env_has_changes(cx)
                    && !self.save_env_scope(cx)
                {
                    cx.notify();
                    return;
                }
                env.name = unique_name(&env.name, |n| {
                    self.envs
                        .iter()
                        .any(|(e, _)| e.name.eq_ignore_ascii_case(n))
                });
                match store.save_environment(&env, None) {
                    Ok(_) => {
                        self.reload_envs();
                        if self.main_view == MainView::Environments {
                            self.load_scope(Scope::Globals, window, cx);
                        }
                        self.notice(Status::Info(format!(
                            "Imported environment \"{}\" with {} variable(s){}",
                            env.name,
                            env.variables.len(),
                            notes(&warnings)
                        )));
                    }
                    Err(e) => self.notice(Status::Error(e.to_string())),
                }
            }
            Imported::Globals(vars, warnings) => {
                if self.main_view == MainView::Environments
                    && self.env_has_changes(cx)
                    && !self.save_env_scope(cx)
                {
                    cx.notify();
                    return;
                }
                let (merged, replaced) = merge_vars(&self.globals, vars);
                let total = merged.len();
                match store.save_globals(&merged) {
                    Ok(()) => {
                        self.reload_envs();
                        if self.main_view == MainView::Environments {
                            self.load_scope(Scope::Globals, window, cx);
                        }
                        self.notice(Status::Info(format!(
                            "Imported global variables: {total} in total, {replaced} replaced{}",
                            notes(&warnings)
                        )));
                    }
                    Err(e) => self.notice(Status::Error(e.to_string())),
                }
            }
        }
        cx.notify();
    }

    fn on_export_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((name, dir)) = self.selected_collection() else {
            self.notice(Status::Error(
                "Click a collection in the sidebar (or open one of its requests) to choose what to export"
                    .into(),
            ));
            cx.notify();
            return;
        };
        let (col, mut warnings) = match read_collection(&name, &dir) {
            Ok(x) => x,
            Err(e) => {
                self.notice(Status::Error(e.to_string()));
                cx.notify();
                return;
            }
        };
        let (text, more) = export_collection(&col);
        warnings.extend(more);
        let file = format!("{}.postman_collection.json", slug(&name));
        self.save_export(
            text,
            file,
            format!("Exported collection \"{name}\""),
            warnings,
            window,
            cx,
        );
    }

    fn on_export_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Export what's on screen: save pending edits first.
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        let (name, vars, globals) = match self.env_scope {
            Scope::Globals => ("Globals".to_string(), self.globals.clone(), true),
            Scope::Env(ix) => match self.envs.get(ix) {
                Some((e, _)) => (e.name.clone(), e.variables.clone(), false),
                None => return,
            },
        };
        let text = export_environment(&name, &vars, self.export_secrets, globals);
        let secrets = vars.iter().filter(|v| v.secret).count();
        let file = if globals {
            "globals.postman_globals.json".to_string()
        } else {
            format!("{}.postman_environment.json", slug(&name))
        };
        let mut warnings = Vec::new();
        if secrets > 0 && !self.export_secrets {
            warnings.push(format!(
                "{secrets} secret value(s) left empty (turn on \"Include secrets\" to export them)"
            ));
        }
        self.save_export(
            text,
            file,
            format!("Exported \"{name}\""),
            warnings,
            window,
            cx,
        );
    }

    /// Ask where to save, then write the file and report.
    fn save_export(
        &mut self,
        text: String,
        file_name: String,
        done: String,
        warnings: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_new_path(&self.export_dir(), Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let written = std::fs::write(&path, text);
            this.update(cx, |this, cx| {
                match written {
                    Ok(()) => this.notice(Status::Info(format!(
                        "{done} to {}{}",
                        path.display(),
                        notes(&warnings)
                    ))),
                    Err(e) => this.notice(Status::Error(format!(
                        "Could not write {}: {e}",
                        path.display()
                    ))),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Leave the environments page (autosaving it). False if the save failed and we stayed.
    fn leave_env_page(&mut self, cx: &mut Context<Self>) -> bool {
        match self.main_view {
            MainView::Environments => self.close_envs(cx),
            // the runner keeps running in the background; its page can be reopened
            MainView::Runner | MainView::Cookies => self.main_view = MainView::Request,
            MainView::Request => {}
        }
        self.main_view == MainView::Request
    }

    // ---------------- collection runner (FR-RUN) ----------------

    /// What "Run…" runs: the folder or collection last clicked in the sidebar (for a tab with
    /// no file), else the collection holding the active tab's request.
    fn run_target(&self) -> Option<(String, PathBuf)> {
        let tab = self.tab();
        if tab.current_path.is_none() {
            if let Some(dir) = &tab.save_dir {
                if let Some(i) = self.items.iter().find(|i| {
                    matches!(i.kind, ItemKind::Collection | ItemKind::Folder) && &i.path == dir
                }) {
                    return Some((i.label.clone(), i.path.clone()));
                }
            }
        }
        self.selected_collection()
    }

    fn open_runner(&mut self, cx: &mut Context<Self>) {
        if !self.leave_env_page(cx) {
            return;
        }
        if self.runner.cancel.is_none() {
            // not running: pick up the current selection
            self.runner.target = self.run_target();
        }
        self.main_view = MainView::Runner;
        cx.notify();
    }

    fn start_run(&mut self, cx: &mut Context<Self>) {
        let Some((name, dir)) = self.runner.target.clone() else {
            return;
        };
        let mut files = Vec::new();
        match Store::load_tree(&dir) {
            Ok(nodes) => run_order(&nodes, &mut files),
            Err(e) => {
                self.runner.notes = vec![e.to_string()];
                cx.notify();
                return;
            }
        }
        if files.is_empty() {
            self.runner.notes = vec![format!("\"{name}\" has no requests to run")];
            cx.notify();
            return;
        }
        let iterations = parse_count(&self.runner.iterations.read(cx).value(), 1, 1, 100) as usize;
        let delay = Duration::from_millis(parse_count(
            &self.runner.delay.read(cx).value(),
            0,
            0,
            60_000,
        ));
        let stop_on_fail = self.runner.stop_on_fail;
        let token = CancellationToken::new();
        let r = &mut self.runner;
        r.rows.clear();
        r.notes.clear();
        r.copied = false;
        r.planned = files.len() * iterations;
        r.cancel = Some(token.clone());
        r.started = Some(Instant::now());
        r.elapsed = None;

        let vars = self.variables();
        let env_name = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| e.name.clone());
        let (tx, rx) = async_channel::unbounded();
        let cookies = self.cookies.clone();
        // Requests run one after another; variables set by scripts flow into the next one.
        self.rt.spawn(async move {
            let mut vars = vars;
            'run: for iteration in 0..iterations {
                for (name, file) in &files {
                    if token.is_cancelled() {
                        break 'run;
                    }
                    let msg = match file.as_ref().map(|p| (p, Store::load_request(p))) {
                        Err(e) => RunMsg::Row {
                            row: Box::new(RunRow {
                                iteration,
                                name: name.clone(),
                                path: None,
                                method: Method::Get,
                                result: RunResult::Failed(format!("unreadable file: {e}")),
                                tests: Vec::new(),
                                errors: Vec::new(),
                            }),
                            env: Default::default(),
                            globals: Default::default(),
                        },
                        Ok((path, Err(e))) => RunMsg::Row {
                            row: Box::new(RunRow {
                                iteration,
                                name: name.clone(),
                                path: Some(path.clone()),
                                method: Method::Get,
                                result: RunResult::Failed(e.to_string()),
                                tests: Vec::new(),
                                errors: Vec::new(),
                            }),
                            env: Default::default(),
                            globals: Default::default(),
                        },
                        Ok((path, Ok(req))) => {
                            let method = req.method;
                            let (out, after) = execute(
                                req,
                                vars.clone(),
                                env_name.clone(),
                                token.child_token(),
                                None,
                                cookies.clone(),
                            )
                            .await;
                            let row = RunRow::from_outcome(
                                iteration,
                                name.clone(),
                                path.clone(),
                                method,
                                &out,
                            );
                            let msg = RunMsg::Row {
                                row: Box::new(row),
                                env: (out.env_before.clone(), to_var_map(&after.environment)),
                                globals: (out.globals_before.clone(), to_var_map(&after.global)),
                            };
                            // runtime (pm.variables) values live for one request only
                            vars = Variables {
                                runtime: Default::default(),
                                ..after
                            };
                            msg
                        }
                    };
                    let failed = matches!(&msg, RunMsg::Row { row, .. } if row.failed());
                    if tx.send(msg).await.is_err() || (failed && stop_on_fail) {
                        break 'run;
                    }
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
            let _ = tx.send(RunMsg::Done).await;
        });
        cx.spawn(async move |this, cx| {
            while let Ok(msg) = rx.recv().await {
                let done = matches!(msg, RunMsg::Done);
                if this
                    .update(cx, |this, cx| this.on_run_msg(msg, cx))
                    .is_err()
                    || done
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn on_run_msg(&mut self, msg: RunMsg, cx: &mut Context<Self>) {
        match msg {
            RunMsg::Row { row, env, globals } => {
                if let Some(n) = self.keep_script_vars(&env.0, &env.1, &globals.0, &globals.1) {
                    if !self.runner.notes.contains(&n) {
                        self.runner.notes.push(n);
                    }
                }
                self.runner.rows.push(*row);
                self.save_cookies_if_changed();
            }
            RunMsg::Done => {
                self.runner.cancel = None;
                self.runner.elapsed = self.runner.started.map(|s| s.elapsed());
            }
        }
        cx.notify();
    }

    // ---------------- cookie jar (FR-COOKIE) ----------------

    fn save_cookies_if_changed(&mut self) {
        if !self.cookies.take_changed() {
            return;
        }
        let Some(store) = &self.store else {
            return;
        };
        if let Err(e) = self.cookies.save(&store.data_dir().join("cookies.json")) {
            self.cookie_note = Some(format!("Cookies not saved: {e}"));
        }
    }

    fn open_cookies(&mut self, cx: &mut Context<Self>) {
        if !self.leave_env_page(cx) {
            return;
        }
        self.confirm_clear_cookies = false;
        self.main_view = MainView::Cookies;
        cx.notify();
    }

    fn render_cookies(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let cookies = self.cookies.list();
        let now = now_ms() / 1000;
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .p_2()
            .child(div().text_color(muted).child("COOKIES"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .children((!cookies.is_empty()).then(|| {
                        Button::new("cookies-clear")
                            .label(if self.confirm_clear_cookies {
                                "Click again to delete all"
                            } else {
                                "Delete all"
                            })
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                if this.confirm_clear_cookies {
                                    this.confirm_clear_cookies = false;
                                    this.cookies.clear();
                                    this.save_cookies_if_changed();
                                } else {
                                    this.confirm_clear_cookies = true;
                                }
                                cx.notify();
                            }))
                    }))
                    .child(
                        Button::new("cookies-back")
                            .label("← Back to requests")
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.main_view = MainView::Request;
                                cx.notify();
                            })),
                    ),
            );
        let mut list = div()
            .id("cookie-list")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px_2();
        if cookies.is_empty() {
            list = list.child(div().p_2().text_color(muted).child(
                "No cookies yet. Cookies that servers set (Set-Cookie) are kept here and sent back automatically with matching requests, in tabs and in runs.",
            ));
        }
        let mut last_domain: Option<String> = None;
        for (ix, ck) in cookies.iter().enumerate() {
            if last_domain.as_deref() != Some(ck.domain.as_str()) {
                let domain = ck.domain.clone();
                list = list.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .pt_2()
                        .pb_1()
                        .border_b_1()
                        .border_color(border)
                        .child(div().child(ck.domain.clone()))
                        .child(
                            Button::new(("cookie-domain-clear", ix))
                                .label("Delete domain")
                                .on_click(cx.listener(move |this, _ev, _window, cx| {
                                    this.cookies.clear_domain(&domain);
                                    this.save_cookies_if_changed();
                                    cx.notify();
                                })),
                        ),
                );
                last_domain = Some(ck.domain.clone());
            }
            let flags = [
                ck.secure.then_some("Secure"),
                ck.http_only.then_some("HttpOnly"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let (d, p, n) = (ck.domain.clone(), ck.path.clone(), ck.name.clone());
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .child(
                        div()
                            .w(px(160.))
                            .flex_none()
                            .truncate()
                            .child(ck.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .font_family(mono())
                            .child(ck.value.clone()),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .text_color(muted)
                            .truncate()
                            .child(ck.path.clone()),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .text_color(muted)
                            .child(expires_label(now, ck.expires)),
                    )
                    .child(div().w(px(120.)).flex_none().text_color(muted).child(flags))
                    .child(
                        Button::new(("cookie-del", ix))
                            .label("✕")
                            .on_click(cx.listener(move |this, _ev, _window, cx| {
                                this.cookies.remove(&d, &p, &n);
                                this.save_cookies_if_changed();
                                cx.notify();
                            })),
                    ),
            );
        }
        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .children(
                self.cookie_note
                    .clone()
                    .map(|n| div().px_2().text_color(c(0xf38ba8)).child(n)),
            )
            .child(list)
    }

    fn render_runner(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let (green, red) = (c(0xa6e3a1), c(0xf38ba8));
        let r = &self.runner;
        let running = r.cancel.is_some();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .p_2()
            .child(div().text_color(muted).child("COLLECTION RUNNER"))
            .child(
                Button::new("run-back")
                    .label("← Back to requests")
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.main_view = MainView::Request;
                        cx.notify();
                    })),
            );
        let target = match &r.target {
            Some((name, _)) => format!("Runs every request in \"{name}\", in sidebar order."),
            None => "Click a collection or folder in the sidebar, then Run….".to_string(),
        };
        let options = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_2()
            .child(div().text_color(muted).child("Iterations"))
            .child(div().w(px(60.)).child(Input::new(&r.iterations)))
            .child(div().text_color(muted).child("Delay (ms)"))
            .child(div().w(px(80.)).child(Input::new(&r.delay)))
            .child(
                seg_btn_ix(
                    "run-stop-fail",
                    0,
                    if r.stop_on_fail {
                        "Stop on first failure: on"
                    } else {
                        "Stop on first failure: off"
                    },
                    r.stop_on_fail,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.runner.stop_on_fail = !this.runner.stop_on_fail;
                    cx.notify();
                })),
            )
            .child(
                Button::new("run-go")
                    .primary()
                    .label(if running { "Stop" } else { "Run" })
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        if let Some(t) = this.runner.cancel.clone() {
                            t.cancel();
                        } else {
                            this.start_run(cx);
                        }
                    })),
            )
            .children((!r.rows.is_empty()).then(|| {
                Button::new("run-copy")
                    .label(if r.copied {
                        "Copied ✓"
                    } else {
                        "Copy summary"
                    })
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            this.runner.summary_text(),
                        ));
                        this.runner.copied = true;
                        cx.notify();
                    }))
            }));

        let (n, failed, pass, fail) = r.counts();
        let time = match (r.elapsed, r.started) {
            (Some(e), _) => format!("{:.1} s", e.as_secs_f32()),
            (None, Some(s)) if running => format!("{:.1} s…", s.elapsed().as_secs_f32()),
            _ => String::new(),
        };
        let progress = if r.rows.is_empty() && !running {
            String::new()
        } else {
            format!(
                "{n} / {} requests · {failed} failed · tests {pass} passed, {fail} failed · {time}{}",
                r.planned,
                if running {
                    ""
                } else if n < r.planned {
                    " · stopped"
                } else {
                    " · done"
                }
            )
        };

        let multi = r.rows.iter().any(|x| x.iteration > 0);
        let mut list = div()
            .id("run-rows")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px_2();
        for (ix, row) in r.rows.iter().enumerate() {
            let (status, colour) = match &row.result {
                RunResult::Response {
                    code,
                    text,
                    ms,
                    size,
                } => (
                    format!("{code} {text} · {ms} ms · {}", human(*size)),
                    status_color(*code),
                ),
                RunResult::Failed(e) => (e.clone(), red),
            };
            let counted: Vec<_> = row.tests.iter().filter(|t| !t.skipped).collect();
            let passed = counted.iter().filter(|t| t.passed).count();
            let tests = if counted.is_empty() {
                String::new()
            } else {
                format!("tests {passed}/{}", counted.len())
            };
            let path = row.path.clone();
            let mut item = div()
                .id(("run-row", ix))
                .py_1()
                .border_b_1()
                .border_color(border)
                .cursor_pointer()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .w(px(16.))
                                .text_color(if row.failed() { red } else { green })
                                .child(if row.failed() { "✗" } else { "✓" }),
                        )
                        .children(multi.then(|| {
                            div()
                                .text_color(muted)
                                .child(format!("#{}", row.iteration + 1))
                        }))
                        .child(div().w(px(64.)).child(row.method.as_str()))
                        .child(div().flex_1().truncate().child(row.name.clone()))
                        .child(div().text_color(colour).child(status))
                        .child(
                            div()
                                .w(px(80.))
                                .text_color(if passed == counted.len() { muted } else { red })
                                .child(tests),
                        ),
                );
            for t in row.tests.iter().filter(|t| !t.passed) {
                item = item.child(div().pl(px(24.)).text_color(red).child(format!(
                    "✗ {}: {}",
                    t.name,
                    t.error.as_deref().unwrap_or("")
                )));
            }
            for e in &row.errors {
                item = item.child(div().pl(px(24.)).text_color(red).child(e.clone()));
            }
            if let Some(p) = path {
                item = item.on_click(cx.listener(move |this, _ev, window, cx| {
                    this.main_view = MainView::Request;
                    this.open_request(p.clone(), window, cx);
                    cx.notify();
                }));
            }
            list = list.child(item);
        }
        if r.rows.is_empty() && !running {
            list = list.child(div().p_2().text_color(muted).child(
                "Variables set by scripts carry over from one request to the next and are saved to the active environment. Runs use the saved files (unsaved tab edits aren't included) and don't fill History.",
            ));
        }

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(header)
            .child(div().px_2().child(target))
            .child(options)
            .children(
                r.notes
                    .iter()
                    .map(|n| div().px_2().text_color(c(0xf9e2af)).child(n.clone())),
            )
            .child(div().px_2().text_color(muted).child(progress))
            .child(list)
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
            if tab.find_open {
                tab.refresh_matches(cx);
                if tab.scroll_to_match {
                    tab.reveal_match(avail);
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
        tab_bar = tab_bar.child(
            div().flex_none().child(
                Button::new("tab-curl")
                    .label("+ cURL")
                    .on_click(cx.listener(|this, _ev, window, cx| this.on_paste_curl(window, cx))),
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
            )
            .child(
                Button::new("cookies-open")
                    .label("Cookies")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.open_cookies(cx))),
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
                seg_btn(
                    "t-body",
                    body_label(tab.body_mode, tab.raw_kind),
                    tab.req_tab == ReqTab::Body,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Body;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-auth",
                    match tab.auth_kind {
                        AuthKind::None => "Auth".to_string(),
                        k => format!("Auth ({})", k.label()),
                    },
                    tab.req_tab == ReqTab::Auth,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Auth;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-scripts",
                    if Self::has_scripts(tab, cx) {
                        "Scripts ●".to_string()
                    } else {
                        "Scripts".to_string()
                    },
                    tab.req_tab == ReqTab::Scripts,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Scripts;
                    cx.notify();
                })),
            )
            .child(
                seg_btn(
                    "t-settings",
                    "Settings".to_string(),
                    tab.req_tab == ReqTab::Settings,
                )
                .on_click(cx.listener(|this, _ev, _window, cx| {
                    this.tab_mut().req_tab = ReqTab::Settings;
                    cx.notify();
                })),
            );
        let req_panel =
            div()
                .id("req-panel")
                .h(px(210.))
                .overflow_y_scroll()
                .child(match tab.req_tab {
                    ReqTab::Params => self.render_kv(tab, Which::Params, cx).into_any_element(),
                    ReqTab::Headers => self.render_kv(tab, Which::Headers, cx).into_any_element(),
                    ReqTab::Body => self.render_body(tab, cx).into_any_element(),
                    ReqTab::Auth => self.render_auth(tab, cx).into_any_element(),
                    ReqTab::Scripts => self.render_scripts(tab, cx).into_any_element(),
                    ReqTab::Settings => self.render_settings(tab, cx).into_any_element(),
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
                    .child(
                        seg_btn(
                            "r-tests",
                            tab.report
                                .as_ref()
                                .map(|r| r.label())
                                .unwrap_or_else(|| "Tests".to_string()),
                            tab.res_tab == ResTab::Tests,
                        )
                        .on_click(cx.listener(|this, _ev, _window, cx| {
                            this.tab_mut().res_tab = ResTab::Tests;
                            cx.notify();
                        })),
                    )
                    .child(
                        seg_btn("r-code", "Code".to_string(), tab.res_tab == ResTab::Code)
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.tab_mut().res_tab = ResTab::Code;
                                this.code_copied = false;
                                cx.notify();
                            })),
                    )
                    .children(copy_btn)
                    .children(
                        (tab.res_tab == ResTab::Body && tab.view.is_some()).then(|| {
                            Button::new("save-body").label("Save…").on_click(
                                cx.listener(|this, _ev, window, cx| this.on_save_body(window, cx)),
                            )
                        }),
                    )
                    .children(
                        (tab.res_tab == ResTab::Body && tab.view.is_some()).then(|| {
                            Button::new("find-open").label("Find").on_click(
                                cx.listener(|this, _ev, window, cx| this.open_find(window, cx)),
                            )
                        }),
                    )
                    .children(wrap_btn),
            );

        let viewer = match &tab.view {
            Some(v) => {
                let rows = v.rows(tab.wrap);
                let data = v.data.clone();
                let n = rows.len();
                // the row holding the current Find match is highlighted
                let hl_row = if tab.find_open {
                    tab.matches.get(tab.match_ix).map(|&off| rows.row_of(off))
                } else {
                    None
                };
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
                                .when(hl_row == Some(ix), |d| d.bg(accent))
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
                .track_scroll(tab.list_scroll.clone())
                .h_full()
                .into_any_element()
            }
            None => div()
                .p_4()
                .text_color(muted)
                .child("Send a request to see the response")
                .into_any_element(),
        };

        let find_bar =
            (tab.res_tab == ResTab::Body && tab.find_open).then(|| {
                let count = if tab.find.read(cx).value().is_empty() {
                    String::new()
                } else if tab.matches.is_empty() {
                    "No matches".to_string()
                } else if tab.matches.len() >= FIND_MAX {
                    format!("{} / {}+", tab.match_ix + 1, FIND_MAX)
                } else {
                    format!("{} / {}", tab.match_ix + 1, tab.matches.len())
                };
                let id = tab.id;
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(div().w(px(260.)).child(Input::new(&tab.find)))
                    .child(div().w(px(90.)).text_color(muted).child(count))
                    .child(Button::new("find-prev").label("↑").on_click(
                        cx.listener(move |this, _ev, _window, cx| this.find_step(id, -1, cx)),
                    ))
                    .child(Button::new("find-next").label("↓").on_click(
                        cx.listener(move |this, _ev, _window, cx| this.find_step(id, 1, cx)),
                    ))
                    .child(Button::new("find-close").label("✕").on_click(cx.listener(
                        |this, _ev, window, cx| {
                            this.tab_mut().find_open = false;
                            this.refocus(window);
                            cx.notify();
                        },
                    )))
            });

        let resp_area = match tab.res_tab {
            ResTab::Body => div()
                .size_full()
                .flex()
                .flex_col()
                .children(find_bar)
                .child(div().flex_1().overflow_hidden().child(viewer))
                .into_any_element(),
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
            ResTab::Tests => self.render_tests(tab, cx).into_any_element(),
            ResTab::Code => self.render_code(cx).into_any_element(),
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
        let side_head = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .p_2()
            .child(
                seg_btn(
                    "side-col",
                    "Collections".to_string(),
                    self.side_view == SideView::Collections,
                )
                .on_click(
                    cx.listener(|this, _ev, _window, cx| this.show_side(SideView::Collections, cx)),
                ),
            )
            .child(
                seg_btn(
                    "side-hist",
                    "History".to_string(),
                    self.side_view == SideView::History,
                )
                .on_click(
                    cx.listener(|this, _ev, _window, cx| this.show_side(SideView::History, cx)),
                ),
            )
            .child(div().flex_1())
            .children((self.side_view == SideView::Collections).then(|| {
                Button::new("new-col")
                    .label("+")
                    .on_click(cx.listener(|this, _ev, _window, cx| this.on_new_collection(cx)))
            }));

        let mut sidebar = div()
            .w(px(260.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(border)
            .child(side_head);
        sidebar = match self.side_view {
            SideView::Collections => sidebar.child(side_list).child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .p_2()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        Button::new("col-import").label("Import…").on_click(
                            cx.listener(|this, _ev, window, cx| this.on_import(window, cx)),
                        ),
                    )
                    .child(Button::new("col-export").label("Export…").on_click(
                        cx.listener(|this, _ev, window, cx| this.on_export_collection(window, cx)),
                    ))
                    .child(
                        Button::new("col-run")
                            .label(if self.runner.cancel.is_some() {
                                "Run… ●"
                            } else {
                                "Run…"
                            })
                            .on_click(cx.listener(|this, _ev, _window, cx| this.open_runner(cx))),
                    ),
            ),
            SideView::History => {
                let now = now_ms();
                let mut list = div()
                    .id("hist-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col();
                if self.history.is_some() && self.hist_items.is_empty() {
                    let empty = if self.hist_search.read(cx).value().trim().is_empty() {
                        "No requests yet. Everything you send appears here."
                    } else {
                        "No matches."
                    };
                    list = list.child(div().p_2().text_color(muted).child(empty));
                }
                for (ix, s) in self.hist_items.iter().enumerate() {
                    let (code, code_col) = match s.status {
                        Some(n) => (n.to_string(), status_color(n)),
                        None => ("ERR".to_string(), c(0xf38ba8)),
                    };
                    let id = s.id;
                    list = list.child(
                        div()
                            .id(("hist", ix))
                            .w_full()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .flex()
                            .flex_col()
                            .border_b_1()
                            .border_color(border)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(s.method.clone())
                                    .child(div().text_color(code_col).child(code))
                                    .child(div().flex_1())
                                    .child(div().text_color(muted).child(ago(now, s.ts_ms))),
                            )
                            .child(div().w_full().text_color(muted).truncate().child(
                                if s.url.is_empty() {
                                    "(no URL)".to_string()
                                } else {
                                    s.url.clone()
                                },
                            ))
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.open_history(id, window, cx)
                            })),
                    );
                }
                let tools = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .pb_2()
                    .child(div().flex_1().child(Input::new(&self.hist_search)))
                    .child(
                        Button::new("hist-clear")
                            .label(if self.confirm_clear {
                                "Really clear?"
                            } else {
                                "Clear"
                            })
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                if this.confirm_clear {
                                    this.clear_history(cx);
                                } else {
                                    this.confirm_clear = true;
                                    cx.notify();
                                }
                            })),
                    );
                sidebar
                    .children(self.history.is_some().then_some(tools))
                    .children(
                        self.hist_error
                            .clone()
                            .map(|e| div().px_2().pb_2().text_color(c(0xf38ba8)).child(e)),
                    )
                    .child(list)
            }
        };

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
            .on_action(cx.listener(|this, _: &FindInResponse, window, cx| {
                if this.main_view == MainView::Request && this.tab().view.is_some() {
                    this.open_find(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ReopenTab, window, cx| {
                if this.main_view == MainView::Request {
                    this.reopen_tab(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SaveRequest, window, cx| this.on_save(window, cx)))
            .child(sidebar)
            .child(match self.main_view {
                MainView::Environments => self.render_env_page(cx).into_any_element(),
                MainView::Runner => self.render_runner(cx).into_any_element(),
                MainView::Cookies => self.render_cookies(cx).into_any_element(),
                MainView::Request => main_col.into_any_element(),
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
    fn cookie_expiry_labels() {
        let now = 1_000_000;
        assert_eq!(expires_label(now, None), "session");
        assert_eq!(expires_label(now, Some(now - 1)), "expired");
        assert_eq!(expires_label(now, Some(now + 30)), "in <1 min");
        assert_eq!(expires_label(now, Some(now + 600)), "in 10 min");
        assert_eq!(expires_label(now, Some(now + 7200)), "in 2 h");
        assert_eq!(expires_label(now, Some(now + 86_400 * 3)), "in 3 d");
    }

    #[test]
    fn runner_order_and_options() {
        let nodes = vec![
            Node::Folder {
                name: "Auth".into(),
                path: "/c/Auth".into(),
                children: vec![Node::Request {
                    name: "Login".into(),
                    path: "/c/Auth/login.toml".into(),
                }],
            },
            Node::Request {
                name: "List".into(),
                path: "/c/list.toml".into(),
            },
            Node::Broken {
                name: "bad".into(),
                path: "/c/bad.toml".into(),
                error: "cannot read".into(),
            },
        ];
        let mut order = Vec::new();
        run_order(&nodes, &mut order);
        let names: Vec<_> = order.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["Login", "List", "bad"],
            "depth-first, sidebar order"
        );
        assert!(order[2].1.is_err());

        assert_eq!(parse_count("3", 1, 1, 100), 3);
        assert_eq!(parse_count("", 1, 1, 100), 1);
        assert_eq!(parse_count("x", 1, 1, 100), 1);
        assert_eq!(parse_count("0", 1, 1, 100), 1, "clamped up");
        assert_eq!(parse_count("5000", 1, 1, 100), 100, "clamped down");
    }

    #[test]
    fn run_rows_fail_on_errors_and_failed_tests_only() {
        let ok = RunResult::Response {
            code: 500,
            text: "Internal Server Error".into(),
            ms: 3,
            size: 0,
        };
        let row =
            |result: RunResult, tests: Vec<at_script::TestResult>, errors: Vec<String>| RunRow {
                iteration: 0,
                name: "r".into(),
                path: None,
                method: Method::Get,
                result,
                tests,
                errors,
            };
        let pass = at_script::TestResult {
            name: "p".into(),
            passed: true,
            skipped: false,
            error: None,
        };
        let fail = at_script::TestResult {
            name: "f".into(),
            passed: false,
            skipped: false,
            error: Some("nope".into()),
        };
        assert!(
            !row(ok.clone(), vec![], vec![]).failed(),
            "a 500 alone isn't a failure"
        );
        assert!(!row(ok.clone(), vec![pass.clone()], vec![]).failed());
        assert!(row(ok.clone(), vec![pass.clone(), fail.clone()], vec![]).failed());
        assert!(row(ok.clone(), vec![], vec!["tests script: boom".into()]).failed());
        assert!(row(RunResult::Failed("refused".into()), vec![], vec![]).failed());

        let runner_rows = [
            row(ok.clone(), vec![pass.clone(), fail], vec![]),
            row(ok, vec![pass], vec![]),
        ];
        let passed = runner_rows.iter().filter(|r| !r.failed()).count();
        assert_eq!(passed, 1);
    }

    #[test]
    fn script_variable_changes_merge_as_a_diff() {
        let map = |pairs: &[(&str, &str)]| -> VarMap {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let mut secret = EnvVar::new("token", "old");
        secret.secret = true;
        let mut off = EnvVar::new("off", "x");
        off.enabled = false;
        let stored = vec![
            EnvVar::new("host", "h"),
            secret,
            off,
            EnvVar::new("gone", "1"),
        ];
        let before = map(&[("host", "h"), ("token", "old"), ("gone", "1")]);
        let after = map(&[("host", "h"), ("token", "new"), ("added", "2")]);
        let merged = merge_script_vars(&stored, &before, &after).unwrap();
        let keys: Vec<_> = merged.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(keys, vec!["host", "token", "off", "added"]);
        assert!(merged[1].secret, "secret flag kept");
        assert_eq!(merged[1].value, "new");
        assert!(!merged[2].enabled, "disabled variable untouched");
        assert_eq!(merge_script_vars(&stored, &before, &before), None);

        // a change made elsewhere meanwhile survives (diff, not overwrite)
        let mut elsewhere = stored.clone();
        elsewhere[0].value = "changed-by-other-tab".into();
        let merged = merge_script_vars(&elsewhere, &before, &after).unwrap();
        assert_eq!(merged[0].value, "changed-by-other-tab");
    }

    #[test]
    fn pre_request_changes_flow_into_the_request() {
        let mut req = Request {
            url: "https://a.test".into(),
            headers: vec![KeyValue::new("A", "1"), {
                let mut off = KeyValue::new("Off", "x");
                off.enabled = false;
                off
            }],
            body: Body::Xml { raw: "<a/>".into() },
            ..Default::default()
        };
        let before = script_request(&req);
        assert_eq!(before.headers, vec![("A".to_string(), "1".to_string())]);
        let mut after = before.clone();
        after.headers.push(("B".into(), "2".into()));
        after.body = "<b/>".into();
        take_script_request(&mut req, &before, &after);
        assert_eq!(req.headers.len(), 2);
        assert_eq!(req.body, Body::Xml { raw: "<b/>".into() }, "raw kind kept");
        assert_eq!(req.url, "https://a.test", "unchanged parts stay");
    }

    #[test]
    fn editor_helpers() {
        assert_eq!(parse_timeout("5000", 1), 5000);
        assert_eq!(parse_timeout(" 0 ", 1), 0, "0 means no timeout");
        assert_eq!(parse_timeout("", 1), RequestSettings::default().timeout_ms);
        assert_eq!(
            parse_timeout("12s", 777),
            777,
            "half-typed keeps the last value"
        );
        assert_eq!(parse_timeout("-5", 777), 777);

        assert_eq!(body_label(BodyMode::None, RawKind::Json), "Body");
        assert_eq!(body_label(BodyMode::Raw, RawKind::Xml), "Body (XML)");
        assert_eq!(
            body_label(BodyMode::FormData, RawKind::Json),
            "Body (form-data)"
        );

        assert_eq!(
            RawKind::Html.body("<p/>".into()),
            Body::Html { raw: "<p/>".into() }
        );
        assert_eq!(RawKind::ALL.len(), 4);
    }

    #[test]
    fn import_helpers() {
        assert_eq!(notes(&[]), "");
        assert_eq!(notes(&["a".into(), "b".into()]), ". 2 note(s): a; b");

        let taken = ["Prod", "Prod 2"];
        let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
        assert_eq!(unique_name("Dev", is_taken), "Dev");
        assert_eq!(unique_name("prod", is_taken), "prod 3");
        assert_eq!(unique_name("  ", is_taken), "Imported");

        let existing = vec![EnvVar::new("a", "1"), EnvVar::new("b", "2")];
        let (merged, replaced) = merge_vars(
            &existing,
            vec![EnvVar::new("b", "20"), EnvVar::new("c", "3")],
        );
        assert_eq!(replaced, 1);
        assert_eq!(
            merged,
            vec![
                EnvVar::new("a", "1"),
                EnvVar::new("b", "20"),
                EnvVar::new("c", "3")
            ]
        );
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
