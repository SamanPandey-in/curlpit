//! The send pipeline without a UI: one request through pre-request script -> build -> send
//! -> test script -> history (`execute`), plus the collection run loop (`run`). The app's
//! tabs and runner page, and the CLI, all go through here so "Send" and "Run" can't drift
//! apart (D34).

pub mod data;
pub mod report;
mod run;
mod script;

pub use at_script::{DataRow, VarMap};
pub use run::{counts, run, run_order, summary_text, RunMsg, RunPlan, RunResult, RunRow};
pub use script::{
    merge_script_vars, script_input, script_request, script_response, take_script_request,
    take_script_vars, to_var_map, SCRIPT_BODY_MAX,
};

use at_core::{build, Body, Method, Request, Variables};
use at_history::{now_ms, History, Record};
use at_http::{send, CookieJar, EngineConfig, HttpError, ResponseBody};
use at_script::{Phase, ScriptInput, ScriptOutput};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// What one send produced, for the tab (or the runner) to fold back in.
pub struct SendOutcome {
    /// `Err` when the request couldn't be built; otherwise the engine's result.
    pub sent: Result<Result<at_http::HttpResponse, HttpError>, String>,
    pub unresolved: Vec<String>,
    pub pre: Option<ScriptOutput>,
    pub test: Option<ScriptOutput>,
    pub env_before: VarMap,
    pub globals_before: VarMap,
    pub hist_err: Option<String>,
}

/// Where a send sits in a collection run, shown to its scripts as `pm.iterationData` and
/// `pm.info.iteration` / `iterationCount`. The default is a plain single send.
#[derive(Clone, Debug, Default)]
pub struct Iteration {
    pub data: DataRow,
    /// 0-based.
    pub index: usize,
    pub count: usize,
}

/// Whether requests with this method carry a body.
pub fn has_body(m: Method) -> bool {
    matches!(
        m,
        Method::Post | Method::Put | Method::Patch | Method::Delete
    )
}

/// The request as it goes out: `https://` is added when the URL has no scheme *after*
/// variable substitution (so `{{base}}/users` works when `base` contains one), and methods
/// that don't take a body send none.
pub fn sendable(mut req: Request, vars: &Variables) -> Request {
    if !req.url.is_empty() && !vars.resolve(&req.url).text.contains("://") {
        req.url = format!("https://{}", req.url);
    }
    if !has_body(req.method) {
        req.body = Body::None;
    }
    req
}

/// What history keeps of one send. `request` is the request as typed (variables unresolved).
pub fn history_record(
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

/// One request through the whole pipeline, off the UI thread: pre-request script -> build ->
/// send -> test script (scripts on the blocking pool so a slow one never stalls networking).
/// Returns what happened plus the variables after its scripts, so a run can chain them.
/// `history: None` skips recording (the collection runner doesn't fill History).
pub async fn execute(
    req: Request,
    vars: Variables,
    env_name: Option<String>,
    token: CancellationToken,
    history: Option<Arc<Mutex<History>>>,
    cookies: Arc<CookieJar>,
    proxy: String,
) -> (SendOutcome, Variables) {
    execute_in(
        req,
        vars,
        env_name,
        token,
        history,
        cookies,
        proxy,
        &Iteration::default(),
    )
    .await
}

/// [`execute`] as one step of a collection run: `iter` is what the scripts see of the run.
/// (The data row's values must already be in `vars`; see [`run`].)
#[allow(clippy::too_many_arguments)]
pub async fn execute_in(
    mut req: Request,
    mut vars: Variables,
    env_name: Option<String>,
    token: CancellationToken,
    history: Option<Arc<Mutex<History>>>,
    cookies: Arc<CookieJar>,
    proxy: String,
    iter: &Iteration,
) -> (SendOutcome, Variables) {
    let with_iter = |mut input: ScriptInput| {
        input.iteration_data = iter.data.clone();
        input.iteration = iter.index;
        input.iteration_count = iter.count;
        input
    };
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
        let input = with_iter(script_input(&req, &vars, env_name.clone(), None));
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
        proxy,
        ..Default::default()
    };
    let r = send(&built, &req.settings, &engine, &token).await;

    let test = match &r {
        Ok(resp) if !req.test_script.trim().is_empty() => {
            let input = with_iter(script_input(
                &req,
                &vars,
                env_name,
                Some(script_response(resp)),
            ));
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
