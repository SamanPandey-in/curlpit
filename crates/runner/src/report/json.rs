//! Machine-readable report. Shape (stable; add fields, don't rename them):
//!
//! ```json
//! { "name": "...", "started": "2026-11-23T12:04:05.123Z", "duration_ms": 1600,
//!   "summary": { "requests": 3, "failed": 2, "tests_passed": 1, "tests_failed": 1,
//!                "iterations": 2, "passed": false },
//!   "rows": [ { "iteration": 1, "name": "...", "path": "/c/x.toml" | null, "method": "GET",
//!               "status": 200 | null, "status_text": "OK" | null, "time_ms": 12 | null,
//!               "size": 34 | null, "error": null | "connection refused", "failed": false,
//!               "tests": [ { "name": "...", "passed": true, "skipped": false, "error": null } ],
//!               "script_errors": [ "tests script: ..." ] } ] }
//! ```
//! `iteration` is 1-based, as in the text report.

use super::{iso8601, iterations, Report};
use crate::{counts, RunResult, RunRow};
use serde_json::{json, Value};

fn row(r: &RunRow) -> Value {
    let (status, status_text, time_ms, size, error) = match &r.result {
        RunResult::Response {
            code,
            text,
            ms,
            size,
        } => (
            json!(code),
            json!(text),
            json!(*ms as u64),
            json!(size),
            Value::Null,
        ),
        RunResult::Failed(e) => (Value::Null, Value::Null, Value::Null, Value::Null, json!(e)),
    };
    json!({
        "iteration": r.iteration + 1,
        "name": r.name,
        "path": r.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "method": r.method.as_str(),
        "status": status,
        "status_text": status_text,
        "time_ms": time_ms,
        "size": size,
        "error": error,
        "failed": r.failed(),
        "tests": r.tests.iter().map(|t| json!({
            "name": t.name,
            "passed": t.passed,
            "skipped": t.skipped,
            "error": t.error,
        })).collect::<Vec<_>>(),
        "script_errors": r.errors,
    })
}

pub fn render(r: &Report) -> String {
    let (n, failed, pass, fail) = counts(r.rows);
    let v = json!({
        "name": r.name,
        "started": iso8601(r.started_ms),
        "duration_ms": r.elapsed.as_millis() as u64,
        "summary": {
            "requests": n,
            "failed": failed,
            "tests_passed": pass,
            "tests_failed": fail,
            "iterations": iterations(r.rows),
            "passed": failed == 0,
        },
        "rows": r.rows.iter().map(row).collect::<Vec<_>>(),
    });
    let mut s = serde_json::to_string_pretty(&v).unwrap_or_default();
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::tests::sample_rows;
    use std::time::Duration;

    #[test]
    fn json_report_shape() {
        let rows = sample_rows();
        let s = render(&Report {
            name: "c",
            rows: &rows,
            started_ms: 0,
            elapsed: Duration::from_millis(1600),
        });
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["name"], "c");
        assert_eq!(v["started"], "1970-01-01T00:00:00.000Z");
        assert_eq!(v["duration_ms"], 1600);
        assert_eq!(
            v["summary"],
            json!({"requests": 3, "failed": 2, "tests_passed": 1, "tests_failed": 1,
                   "iterations": 2, "passed": false})
        );
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["iteration"], 1);
        assert_eq!(rows[0]["method"], "POST");
        assert_eq!(rows[0]["status"], 200);
        assert_eq!(rows[0]["path"], "/c/auth/login.toml");
        assert_eq!(rows[0]["failed"], false);
        assert_eq!(rows[0]["tests"][0]["name"], "status is 200");
        assert_eq!(rows[1]["tests"][0]["passed"], false);
        assert_eq!(rows[2]["iteration"], 2);
        assert_eq!(rows[2]["status"], Value::Null);
        assert_eq!(rows[2]["error"], "connection refused ]]>");
        assert_eq!(
            rows[2]["script_errors"][0],
            "tests script: ReferenceError: x"
        );
    }
}
