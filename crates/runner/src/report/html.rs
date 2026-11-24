//! A single self-contained HTML page (inline CSS, no scripts or external files), so it can be
//! attached to a CI job or mailed as is. Follows the system light/dark setting.

use super::{iso8601, iterations, xml_escape as esc, Report};
use crate::{counts, RunResult};

const STYLE: &str = "
:root{--bg:#fff;--fg:#1f2328;--muted:#656d76;--line:#d0d7de;--ok:#1a7f37;--bad:#cf222e;--warn:#9a6700;--card:#f6f8fa}
@media (prefers-color-scheme:dark){:root{--bg:#0d1117;--fg:#e6edf3;--muted:#8d96a0;--line:#30363d;--ok:#3fb950;--bad:#f85149;--warn:#d29922;--card:#161b22}}
*{box-sizing:border-box}body{margin:0;padding:24px 16px;background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,-apple-system,Segoe UI,sans-serif}
main{max-width:1100px;margin:0 auto}h1{font-size:20px;margin:0 0 4px}.meta{color:var(--muted);margin-bottom:16px}
.cards{display:flex;flex-wrap:wrap;gap:8px;margin-bottom:16px}.card{background:var(--card);border:1px solid var(--line);border-radius:6px;padding:8px 12px;min-width:120px}
.card b{display:block;font-size:20px}.ok{color:var(--ok)}.bad{color:var(--bad)}.warn{color:var(--warn)}
.wrap{overflow-x:auto}table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line);vertical-align:top}
th{color:var(--muted);font-weight:600}td.num{text-align:right;white-space:nowrap}ul{margin:4px 0 0;padding-left:18px}code{font-family:ui-monospace,Consolas,monospace;font-size:12px}
";

pub fn render(r: &Report) -> String {
    let (n, failed, pass, fail) = counts(r.rows);
    let name = esc(r.name);
    let verdict = if failed == 0 {
        "<b class=\"ok\">passed</b>"
    } else {
        "<b class=\"bad\">failed</b>"
    };
    let mut rows = String::new();
    for row in r.rows {
        let (status, ms) = match &row.result {
            RunResult::Response { code, text, ms, .. } => {
                let class = if *code >= 400 { "warn" } else { "" };
                (
                    format!("<span class=\"{class}\">{code} {}</span>", esc(text)),
                    format!("{ms} ms"),
                )
            }
            RunResult::Failed(e) => (
                format!("<span class=\"bad\">error: {}</span>", esc(e)),
                String::new(),
            ),
        };
        let mut detail = String::new();
        if !row.tests.is_empty() || !row.errors.is_empty() {
            detail.push_str("<ul>");
            for t in &row.tests {
                let (class, mark) = if t.skipped {
                    ("", "skipped")
                } else if t.passed {
                    ("ok", "✓")
                } else {
                    ("bad", "✗")
                };
                detail.push_str(&format!(
                    "<li class=\"{class}\">{mark} {}{}</li>",
                    esc(&t.name),
                    t.error
                        .as_deref()
                        .filter(|_| !t.passed)
                        .map(|e| format!(": <code>{}</code>", esc(e)))
                        .unwrap_or_default()
                ));
            }
            for e in &row.errors {
                detail.push_str(&format!("<li class=\"bad\">! <code>{}</code></li>", esc(e)));
            }
            detail.push_str("</ul>");
        }
        let result = if row.failed() {
            "<span class=\"bad\">FAIL</span>"
        } else {
            "<span class=\"ok\">ok</span>"
        };
        rows.push_str(&format!(
            "<tr><td>{result}</td><td class=\"num\">{}</td><td><code>{}</code></td><td>{}{detail}</td><td>{status}</td><td class=\"num\">{ms}</td></tr>\n",
            row.iteration + 1,
            row.method.as_str(),
            esc(&row.name),
        ));
    }
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{name} run report</title><style>{STYLE}</style></head><body><main>\n\
<h1>{name}</h1><div class=\"meta\">Started {} · {} ms · {} iteration(s) · {verdict}</div>\n\
<div class=\"cards\"><div class=\"card\"><b>{n}</b>requests</div>\
<div class=\"card\"><b class=\"{}\">{failed}</b>failed</div>\
<div class=\"card\"><b class=\"ok\">{pass}</b>tests passed</div>\
<div class=\"card\"><b class=\"{}\">{fail}</b>tests failed</div></div>\n\
<div class=\"wrap\"><table><thead><tr><th>Result</th><th>#</th><th>Method</th><th>Request</th><th>Status</th><th>Time</th></tr></thead><tbody>\n\
{rows}</tbody></table></div>\n</main></body></html>\n",
        iso8601(r.started_ms),
        r.elapsed.as_millis(),
        iterations(r.rows),
        if failed > 0 { "bad" } else { "ok" },
        if fail > 0 { "bad" } else { "ok" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::tests::sample_rows;
    use std::time::Duration;

    #[test]
    fn html_is_self_contained_and_escaped() {
        let rows = sample_rows();
        let html = render(&Report {
            name: "<script>x</script>",
            rows: &rows,
            started_ms: 0,
            elapsed: Duration::from_millis(5),
        });
        assert!(html.starts_with("<!doctype html>"));
        assert!(!html.contains("<script"), "no scripts, name escaped");
        assert!(!html.contains("src=") && !html.contains("<link"));
        assert!(html.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(html.contains("Login &lt;admin&gt;"));
        assert!(html.contains("✗ has &lt;items&gt;"));
        assert_eq!(html.matches("<tr><td>").count(), 3);
    }
}
