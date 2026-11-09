//! State and bookkeeping for the collection runner (FR-RUN): the order requests run in,
//! one result row per request, the progress messages the network task sends back, and the
//! counts and plain-text report derived from them. The run loop itself lives in
//! `workspace::runner`; this module is what it fills in.

use crate::workspace::SendOutcome;
use at_core::Method;
use at_script::VarMap;
use at_store::Node;
use gpui::Entity;
use gpui_component::input::InputState;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Requests of a collection/folder in run order (the sidebar's order: folders first, then by
/// name), each with its file or why it can't run.
pub(crate) fn run_order(nodes: &[Node], out: &mut Vec<(String, Result<PathBuf, String>)>) {
    for n in nodes {
        match n {
            Node::Folder { children, .. } => run_order(children, out),
            Node::Request { name, path } => out.push((name.clone(), Ok(path.clone()))),
            Node::Broken { name, error, .. } => out.push((name.clone(), Err(error.clone()))),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RunResult {
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
pub(crate) struct RunRow {
    pub(crate) iteration: usize,
    pub(crate) name: String,
    pub(crate) path: Option<PathBuf>,
    pub(crate) method: Method,
    pub(crate) result: RunResult,
    pub(crate) tests: Vec<at_script::TestResult>,
    /// Pre-request / test script errors.
    pub(crate) errors: Vec<String>,
}

impl RunRow {
    /// A row counts as failed when there was no response, a script broke, or a test failed.
    /// (An HTTP error status alone isn't a failure: write a test for that.)
    pub(crate) fn failed(&self) -> bool {
        matches!(self.result, RunResult::Failed(_))
            || !self.errors.is_empty()
            || self.tests.iter().any(|t| !t.passed)
    }

    pub(crate) fn from_outcome(
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
pub(crate) enum RunMsg {
    Row {
        row: Box<RunRow>,
        /// (before, after) of the environment and globals, to save script changes.
        env: (VarMap, VarMap),
        globals: (VarMap, VarMap),
    },
    Done,
}

/// The runner page's state.
pub(crate) struct Runner {
    /// Collection or folder being run: (display name, directory).
    pub(crate) target: Option<(String, PathBuf)>,
    pub(crate) iterations: Entity<InputState>,
    pub(crate) delay: Entity<InputState>,
    pub(crate) stop_on_fail: bool,
    pub(crate) rows: Vec<RunRow>,
    pub(crate) planned: usize,
    pub(crate) cancel: Option<CancellationToken>,
    pub(crate) started: Option<Instant>,
    pub(crate) elapsed: Option<Duration>,
    pub(crate) notes: Vec<String>,
    pub(crate) copied: bool,
}

impl Runner {
    /// (requests run, requests failed, tests passed, tests failed)
    pub(crate) fn counts(&self) -> (usize, usize, usize, usize) {
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
    pub(crate) fn summary_text(&self) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::parse_count;
    use at_core::Method;

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
}
