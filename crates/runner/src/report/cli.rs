//! The human-readable report: one line per request (failed tests and script errors indented
//! under it), then the totals. Same wording as `summary_text`; ANSI colour is optional so a
//! terminal gets it and a file or pipe doesn't.

use super::Report;
use crate::{counts, RunResult, RunRow};

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("{code}{s}{RESET}")
    } else {
        s.to_string()
    }
}

/// One request's lines (ending in a newline), as printed while the run goes.
pub fn row_lines(r: &RunRow, color: bool) -> String {
    let status = match &r.result {
        RunResult::Response { code, ms, .. } => {
            let c = if *code >= 400 { YELLOW } else { DIM };
            format!(
                "{} {}",
                paint(color, c, &code.to_string()),
                paint(color, DIM, &format!("({ms} ms)"))
            )
        }
        RunResult::Failed(e) => paint(color, RED, &format!("ERROR {e}")),
    };
    let mark = if r.failed() {
        paint(color, RED, "FAIL")
    } else {
        paint(color, GREEN, "ok  ")
    };
    let mut s = format!(
        "{mark} #{} {} {} -> {status}\n",
        r.iteration + 1,
        r.method.as_str(),
        r.name
    );
    for t in r.tests.iter().filter(|t| !t.passed) {
        s.push_str(&paint(
            color,
            RED,
            &format!("      ✗ {}: {}", t.name, t.error.as_deref().unwrap_or("")),
        ));
        s.push('\n');
    }
    for e in &r.errors {
        s.push_str(&paint(color, RED, &format!("      ! {e}")));
        s.push('\n');
    }
    s
}

/// The totals line(s) printed after the rows.
pub fn totals(r: &Report, color: bool) -> String {
    let (n, failed, pass, fail) = counts(r.rows);
    let head = format!(
        "Run \"{}\": {n} request(s), {failed} failed; tests {pass} passed, {fail} failed ({} ms)",
        r.name,
        r.elapsed.as_millis()
    );
    let c = if failed > 0 { RED } else { GREEN };
    format!("{}\n", paint(color, &format!("{BOLD}{c}"), &head))
}

/// Rows then totals.
pub fn render(r: &Report, color: bool) -> String {
    let mut s: String = r.rows.iter().map(|row| row_lines(row, color)).collect();
    s.push_str(&totals(r, color));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::tests::sample_rows;
    use std::time::Duration;

    #[test]
    fn plain_text_has_no_escape_codes_and_colour_has_them() {
        let rows = sample_rows();
        let r = Report {
            name: "c",
            rows: &rows,
            started_ms: 0,
            elapsed: Duration::from_millis(1600),
        };
        let plain = render(&r, false);
        assert!(!plain.contains('\x1b'));
        assert!(plain.contains("ok   #1 POST Login <admin> -> 200 (12 ms)"));
        assert!(plain.contains("FAIL #2 GET Broken -> ERROR connection refused"));
        assert!(plain.contains("      ✗ has <items>: expected"));
        assert!(plain
            .ends_with("Run \"c\": 3 request(s), 2 failed; tests 1 passed, 1 failed (1600 ms)\n"));
        let coloured = render(&r, true);
        assert!(coloured.contains(RED) && coloured.contains(GREEN));
    }
}
