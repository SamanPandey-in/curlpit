//! Run reports built from the result rows: human text (`cli`), machine-readable JSON
//! (`json`), JUnit XML for CI (`junit`) and a single self-contained HTML page (`html`).
//! They only read `RunRow`s (no response bodies), so a report costs the same whatever the
//! responses weighed.

pub mod cli;
pub mod html;
pub mod json;
pub mod junit;

use crate::{RunResult, RunRow};
use std::time::Duration;

/// One finished (or stopped) run, as the reporters see it.
#[derive(Clone, Copy, Debug)]
pub struct Report<'a> {
    /// Collection, folder or request name.
    pub name: &'a str,
    pub rows: &'a [RunRow],
    /// Start time, Unix milliseconds.
    pub started_ms: u64,
    pub elapsed: Duration,
}

/// The report formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Cli,
    Json,
    Junit,
    Html,
}

impl Format {
    pub fn render(self, r: &Report) -> String {
        match self {
            Format::Cli => cli::render(r, false),
            Format::Json => json::render(r),
            Format::Junit => junit::render(r),
            Format::Html => html::render(r),
        }
    }

    /// File name used when only an output directory is given.
    pub fn file_name(self) -> &'static str {
        match self {
            Format::Cli => "report.txt",
            Format::Json => "report.json",
            Format::Junit => "junit.xml",
            Format::Html => "report.html",
        }
    }
}

/// Number of iterations the rows cover.
pub(crate) fn iterations(rows: &[RunRow]) -> usize {
    rows.iter().map(|r| r.iteration + 1).max().unwrap_or(0)
}

/// Response time in ms, 0 without a response.
pub(crate) fn row_ms(r: &RunRow) -> u128 {
    match r.result {
        RunResult::Response { ms, .. } => ms,
        RunResult::Failed(_) => 0,
    }
}

/// `2026-11-23T10:04:05.123Z` from Unix milliseconds (UTC; no date crate needed).
pub fn iso8601(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        tod / 3600,
        tod / 60 % 60,
        tod % 60,
        ms % 1000
    )
}

/// Escape text for XML/HTML content and attribute values, dropping characters XML 1.0 can't
/// carry at all (control characters other than tab and line breaks, U+FFFE/U+FFFF).
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use at_core::Method;

    pub(crate) fn sample_rows() -> Vec<RunRow> {
        let t = |name: &str, passed: bool, err: Option<&str>| at_script::TestResult {
            name: name.into(),
            passed,
            skipped: false,
            error: err.map(String::from),
        };
        vec![
            RunRow {
                iteration: 0,
                name: "Login <admin>".into(),
                path: Some("/c/auth/login.toml".into()),
                method: Method::Post,
                result: RunResult::Response {
                    code: 200,
                    text: "OK".into(),
                    ms: 12,
                    size: 34,
                },
                tests: vec![t("status is 200", true, None)],
                errors: vec![],
            },
            RunRow {
                iteration: 0,
                name: "List \"users\" & co".into(),
                path: None,
                method: Method::Get,
                result: RunResult::Response {
                    code: 500,
                    text: "Internal Server Error".into(),
                    ms: 1500,
                    size: 0,
                },
                tests: vec![t("has <items>", false, Some("expected 'a' & \"b\"\u{1}"))],
                errors: vec![],
            },
            RunRow {
                iteration: 1,
                name: "Broken".into(),
                path: None,
                method: Method::Get,
                result: RunResult::Failed("connection refused ]]>".into()),
                tests: vec![],
                errors: vec!["tests script: ReferenceError: x".into()],
            },
        ]
    }

    #[test]
    fn iso8601_matches_known_instants() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso8601(1_795_435_445_123), "2026-11-23T12:04:05.123Z");
    }

    #[test]
    fn xml_escape_covers_markup_and_drops_invalid_chars() {
        assert_eq!(
            xml_escape("a<b>&\"c\"'d'\u{0}\u{1b}\t\n"),
            "a&lt;b&gt;&amp;&quot;c&quot;&apos;d&apos;\t\n"
        );
    }
}
