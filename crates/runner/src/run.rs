//! The collection runner without a UI (FR-RUN): the order requests run in, one result row
//! per request, the sequential run loop, and the counts and plain-text report derived from
//! the rows. The app's runner page and the CLI both drive `run`.

use crate::{execute_in, to_var_map, DataRow, Iteration, SendOutcome};
use at_core::{Method, Variables};
use at_http::CookieJar;
use at_script::VarMap;
use at_store::{Node, Store};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Requests of a collection/folder in run order (the sidebar's order: folders first, then by
/// name), each with its file or why it can't run.
pub fn run_order(nodes: &[Node], out: &mut Vec<(String, Result<PathBuf, String>)>) {
    for n in nodes {
        match n {
            Node::Folder { children, .. } => run_order(children, out),
            Node::Request { name, path } => out.push((name.clone(), Ok(path.clone()))),
            Node::Broken { name, error, .. } => out.push((name.clone(), Err(error.clone()))),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RunResult {
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
pub struct RunRow {
    pub iteration: usize,
    pub name: String,
    pub path: Option<PathBuf>,
    pub method: Method,
    pub result: RunResult,
    pub tests: Vec<at_script::TestResult>,
    /// Pre-request / test script errors.
    pub errors: Vec<String>,
}

impl RunRow {
    /// A row counts as failed when there was no response, a script broke, or a test failed.
    /// (An HTTP error status alone isn't a failure: write a test for that.)
    pub fn failed(&self) -> bool {
        matches!(self.result, RunResult::Failed(_))
            || !self.errors.is_empty()
            || self.tests.iter().any(|t| !t.passed)
    }

    pub fn from_outcome(
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

    /// A row for a request that never ran (unreadable or unparsable file).
    fn not_run(iteration: usize, name: &str, path: Option<PathBuf>, error: String) -> RunRow {
        RunRow {
            iteration,
            name: name.to_string(),
            path,
            method: Method::Get,
            result: RunResult::Failed(error),
            tests: Vec::new(),
            errors: Vec::new(),
        }
    }
}

/// Run progress, one message per request and a final `Done`.
pub enum RunMsg {
    Row {
        row: Box<RunRow>,
        /// (before, after) of the environment and globals, to save script changes.
        env: (VarMap, VarMap),
        globals: (VarMap, VarMap),
    },
    Done,
}

/// Everything one run needs. `files` comes from `run_order`.
pub struct RunPlan {
    pub files: Vec<(String, Result<PathBuf, String>)>,
    pub iterations: usize,
    pub delay: Duration,
    pub stop_on_fail: bool,
    pub vars: Variables,
    pub env_name: Option<String>,
    pub cookies: Arc<CookieJar>,
    pub proxy: String,
    /// Data-file rows (`data::load`). Iteration `i` uses row `i`; past the last row the last
    /// row repeats. Empty = no data file.
    pub data: Vec<DataRow>,
    /// Replaces every request's own timeout (0 = none) when set.
    pub timeout_ms: Option<u64>,
    /// Turns TLS certificate verification off for every request.
    pub insecure: bool,
}

impl Default for RunPlan {
    fn default() -> Self {
        RunPlan {
            files: Vec::new(),
            iterations: 1,
            delay: Duration::ZERO,
            stop_on_fail: false,
            vars: Variables::default(),
            env_name: None,
            cookies: Arc::new(CookieJar::default()),
            proxy: String::new(),
            data: Vec::new(),
            timeout_ms: None,
            insecure: false,
        }
    }
}

/// Run the requests one after another, `iterations` times; variables set by one request's
/// scripts flow into the next. Each result is sent on `tx` as it happens, then `Done`. Stops
/// early when `token` is cancelled, the receiver is gone, or (with `stop_on_fail`) a row fails.
/// Runs are not recorded in History.
///
/// An iteration's data row is visible to scripts as `pm.iterationData` and resolves as
/// `{{column}}`: its values are laid into the runtime scope at the start of each request, so
/// they beat environment, collection and global values, while a script's own
/// `pm.variables.set` still beats them (Postman's global < collection < environment < data <
/// local). Like runtime values, they never reach the environment or globals.
pub async fn run(plan: RunPlan, token: CancellationToken, tx: async_channel::Sender<RunMsg>) {
    let RunPlan {
        files,
        iterations,
        delay,
        stop_on_fail,
        mut vars,
        env_name,
        cookies,
        proxy,
        data,
        timeout_ms,
        insecure,
    } = plan;
    'run: for iteration in 0..iterations {
        let iter = Iteration {
            data: data
                .get(iteration)
                .or(data.last())
                .cloned()
                .unwrap_or_default(),
            index: iteration,
            count: iterations,
        };
        for (name, file) in &files {
            if token.is_cancelled() {
                break 'run;
            }
            let msg = match file.as_ref().map(|p| (p, Store::load_request(p))) {
                Err(e) => RunMsg::Row {
                    row: Box::new(RunRow::not_run(
                        iteration,
                        name,
                        None,
                        format!("unreadable file: {e}"),
                    )),
                    env: Default::default(),
                    globals: Default::default(),
                },
                Ok((path, Err(e))) => RunMsg::Row {
                    row: Box::new(RunRow::not_run(
                        iteration,
                        name,
                        Some(path.clone()),
                        e.to_string(),
                    )),
                    env: Default::default(),
                    globals: Default::default(),
                },
                Ok((path, Ok(mut req))) => {
                    let method = req.method;
                    if let Some(ms) = timeout_ms {
                        req.settings.timeout_ms = ms;
                    }
                    if insecure {
                        req.settings.verify_tls = false;
                    }
                    let mut v = vars.clone();
                    for (k, val) in &iter.data {
                        v.runtime.insert(k.clone(), crate::data::value_text(val));
                    }
                    let (out, after) = execute_in(
                        req,
                        v,
                        env_name.clone(),
                        token.child_token(),
                        None,
                        cookies.clone(),
                        proxy.clone(),
                        &iter,
                    )
                    .await;
                    let row =
                        RunRow::from_outcome(iteration, name.clone(), path.clone(), method, &out);
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
}

/// (requests run, requests failed, tests passed, tests failed); skipped tests aren't counted.
pub fn counts(rows: &[RunRow]) -> (usize, usize, usize, usize) {
    let failed = rows.iter().filter(|r| r.failed()).count();
    let (mut pass, mut fail) = (0, 0);
    for t in rows.iter().flat_map(|r| r.tests.iter()) {
        if t.skipped {
            continue;
        }
        if t.passed {
            pass += 1;
        } else {
            fail += 1;
        }
    }
    (rows.len(), failed, pass, fail)
}

/// Plain-text report of a run named `name`.
pub fn summary_text(name: &str, rows: &[RunRow]) -> String {
    let (n, failed, pass, fail) = counts(rows);
    let mut s = format!(
        "Run \"{name}\": {n} request(s), {failed} failed; tests {pass} passed, {fail} failed\n"
    );
    for r in rows {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_order_is_depth_first_sidebar_order() {
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
        assert_eq!(names, vec!["Login", "List", "bad"]);
        assert!(order[2].1.is_err());
    }

    fn row(result: RunResult, tests: Vec<at_script::TestResult>, errors: Vec<String>) -> RunRow {
        RunRow {
            iteration: 0,
            name: "r".into(),
            path: None,
            method: Method::Get,
            result,
            tests,
            errors,
        }
    }

    fn test(name: &str, passed: bool) -> at_script::TestResult {
        at_script::TestResult {
            name: name.into(),
            passed,
            skipped: false,
            error: (!passed).then(|| "nope".to_string()),
        }
    }

    #[test]
    fn run_rows_fail_on_errors_and_failed_tests_only() {
        let ok = RunResult::Response {
            code: 500,
            text: "Internal Server Error".into(),
            ms: 3,
            size: 0,
        };
        let (pass, fail) = (test("p", true), test("f", false));
        assert!(
            !row(ok.clone(), vec![], vec![]).failed(),
            "a 500 alone isn't a failure"
        );
        assert!(!row(ok.clone(), vec![pass.clone()], vec![]).failed());
        assert!(row(ok.clone(), vec![pass.clone(), fail.clone()], vec![]).failed());
        assert!(row(ok.clone(), vec![], vec!["tests script: boom".into()]).failed());
        assert!(row(RunResult::Failed("refused".into()), vec![], vec![]).failed());
    }

    #[test]
    fn counts_and_summary_report_failures() {
        let ok = RunResult::Response {
            code: 200,
            text: "OK".into(),
            ms: 7,
            size: 2,
        };
        let mut skipped = test("s", true);
        skipped.skipped = true;
        let rows = [
            row(ok.clone(), vec![test("p", true), test("f", false)], vec![]),
            row(ok, vec![test("p2", true), skipped], vec![]),
            row(RunResult::Failed("refused".into()), vec![], vec![]),
        ];
        assert_eq!(counts(&rows), (3, 2, 2, 1));
        let s = summary_text("c", &rows);
        assert!(s.starts_with("Run \"c\": 3 request(s), 2 failed; tests 2 passed, 1 failed\n"));
        assert!(s.contains("FAIL #1 GET r -> 200 (7 ms)"));
        assert!(s.contains("✗ f: nope"));
        assert!(s.contains("ok   #1 GET r -> 200"));
        assert!(s.contains("ERROR refused"));
    }

    #[tokio::test]
    async fn a_run_reports_unreadable_files_and_finishes() {
        let plan = RunPlan {
            files: vec![
                ("gone".into(), Err("cannot read".into())),
                (
                    "missing".into(),
                    Ok(PathBuf::from("/definitely/not/here.toml")),
                ),
            ],
            iterations: 2,
            delay: Duration::ZERO,
            stop_on_fail: false,
            ..Default::default()
        };
        let (tx, rx) = async_channel::unbounded();
        run(plan, CancellationToken::new(), tx).await;
        let mut rows = Vec::new();
        while let Ok(m) = rx.recv().await {
            match m {
                RunMsg::Row { row, .. } => rows.push(*row),
                RunMsg::Done => break,
            }
        }
        assert_eq!(rows.len(), 4, "2 files x 2 iterations");
        assert!(rows.iter().all(|r| r.failed()));
        assert_eq!(rows[2].iteration, 1);
    }

    #[tokio::test]
    async fn stop_on_fail_stops_after_the_first_failure() {
        let plan = RunPlan {
            files: vec![("a".into(), Err("x".into())), ("b".into(), Err("y".into()))],
            iterations: 3,
            delay: Duration::ZERO,
            stop_on_fail: true,
            ..Default::default()
        };
        let (tx, rx) = async_channel::unbounded();
        run(plan, CancellationToken::new(), tx).await;
        let mut n = 0;
        while let Ok(RunMsg::Row { .. }) = rx.recv().await {
            n += 1;
        }
        assert_eq!(n, 1);
    }

    /// A one-shot-per-connection HTTP server answering every request with its own path.
    async fn echo_server() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf);
                    let path = head.split(' ').nth(1).unwrap_or("/").to_string();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{path}",
                        path.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn data_rows_resolve_as_variables_and_reach_scripts() {
        let base = echo_server().await;
        let dir = std::env::temp_dir().join(format!("at-runner-data-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let req = at_core::Request {
            name: "Echo".into(),
            url: "{{base}}/u/{{user}}".into(),
            pre_request_script: "if (pm.iterationData.get('user') === 'cy') pm.variables.set('user', 'local');".into(),
            test_script: r#"
                pm.test("echo", () => pm.expect(pm.response.text()).to.equal("/u/" + pm.variables.get("user")));
                pm.test("data", () => pm.expect(pm.iterationData.get("user")).to.be.a("string"));
                pm.test("info", () => pm.expect(pm.info.iterationCount).to.equal(4));
            "#
            .into(),
            ..Default::default()
        };
        let path = Store::save_request(&dir, &req, None).unwrap();
        let mut vars = Variables::default();
        vars.environment.insert("base".into(), base);
        vars.environment.insert("user".into(), "from-env".into());
        let data = crate::data::parse_csv("user\nada\nbob\ncy\n").unwrap();
        let plan = RunPlan {
            files: vec![("Echo".into(), Ok(path))],
            iterations: 4,
            vars,
            data,
            ..Default::default()
        };
        let (tx, rx) = async_channel::unbounded();
        run(plan, CancellationToken::new(), tx).await;
        let mut seen = Vec::new();
        while let Ok(RunMsg::Row { row, env, .. }) = rx.recv().await {
            assert!(!row.failed(), "{row:?}");
            assert!(!env.1.contains_key("user") || env.1["user"] == "from-env");
            seen.push(row);
        }
        assert_eq!(seen.len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
