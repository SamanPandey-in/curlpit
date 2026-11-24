//! JUnit XML, the format CI systems (GitHub, GitLab, Jenkins, Azure) read. One `<testsuite>`
//! per run and one `<testcase>` per request per iteration. A row with no response or a script
//! error is an `<error>`; failed `pm.test`s are a `<failure>` listing each one. Every test's
//! outcome is in `<system-out>`.

use super::{iso8601, iterations, row_ms, xml_escape as esc, Report};
use crate::{RunResult, RunRow};

fn secs(ms: u128) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

fn case_name(r: &RunRow, many: bool) -> String {
    let base = format!("{} {}", r.method.as_str(), r.name);
    if many {
        format!("{base} (iteration {})", r.iteration + 1)
    } else {
        base
    }
}

/// What a row is in JUnit terms.
enum Outcome {
    Pass,
    Failure,
    Error,
}

fn outcome(r: &RunRow) -> Outcome {
    if matches!(r.result, RunResult::Failed(_)) || !r.errors.is_empty() {
        Outcome::Error
    } else if r.tests.iter().any(|t| !t.passed) {
        Outcome::Failure
    } else {
        Outcome::Pass
    }
}

pub fn render(r: &Report) -> String {
    let many = iterations(r.rows) > 1;
    let (mut failures, mut errors) = (0, 0);
    let mut cases = String::new();
    for row in r.rows {
        let o = outcome(row);
        match o {
            Outcome::Failure => failures += 1,
            Outcome::Error => errors += 1,
            Outcome::Pass => {}
        }
        cases.push_str(&format!(
            "    <testcase name=\"{}\" classname=\"{}\" time=\"{}\">\n",
            esc(&case_name(row, many)),
            esc(r.name),
            secs(row_ms(row))
        ));
        if let Outcome::Error = o {
            let mut msgs: Vec<String> = Vec::new();
            if let RunResult::Failed(e) = &row.result {
                msgs.push(e.clone());
            }
            msgs.extend(row.errors.iter().cloned());
            cases.push_str(&format!(
                "      <error message=\"{}\" type=\"Error\">{}</error>\n",
                esc(&msgs[0]),
                esc(&msgs.join("\n"))
            ));
        }
        let failed: Vec<_> = row.tests.iter().filter(|t| !t.passed).collect();
        if !failed.is_empty() {
            let detail: Vec<String> = failed
                .iter()
                .map(|t| format!("{}: {}", t.name, t.error.as_deref().unwrap_or("failed")))
                .collect();
            cases.push_str(&format!(
                "      <failure message=\"{}\" type=\"AssertionError\">{}</failure>\n",
                esc(&format!("{} test(s) failed", failed.len())),
                esc(&detail.join("\n"))
            ));
        }
        if !row.tests.is_empty() {
            let out: Vec<String> = row
                .tests
                .iter()
                .map(|t| {
                    let mark = if t.skipped {
                        "skipped"
                    } else if t.passed {
                        "pass"
                    } else {
                        "FAIL"
                    };
                    format!("[{mark}] {}", t.name)
                })
                .collect();
            cases.push_str(&format!(
                "      <system-out>{}</system-out>\n",
                esc(&out.join("\n"))
            ));
        }
        cases.push_str("    </testcase>\n");
    }
    let n = r.rows.len();
    let time = secs(r.elapsed.as_millis());
    let name = esc(r.name);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <testsuites name=\"{name}\" tests=\"{n}\" failures=\"{failures}\" errors=\"{errors}\" time=\"{time}\">\n  \
         <testsuite name=\"{name}\" tests=\"{n}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"0\" time=\"{time}\" timestamp=\"{}\">\n\
         {cases}  </testsuite>\n\
         </testsuites>\n",
        iso8601(r.started_ms)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::tests::sample_rows;
    use std::time::Duration;

    #[test]
    fn junit_is_well_formed_and_escaped() {
        let rows = sample_rows();
        let xml = render(&Report {
            name: "My <API> & \"tests\"",
            rows: &rows,
            started_ms: 0,
            elapsed: Duration::from_millis(1600),
        });
        let doc = roxmltree::Document::parse(&xml).expect("well-formed XML");
        let root = doc.root_element();
        assert_eq!(root.tag_name().name(), "testsuites");
        assert_eq!(root.attribute("name"), Some("My <API> & \"tests\""));
        assert_eq!(root.attribute("tests"), Some("3"));
        assert_eq!(root.attribute("failures"), Some("1"));
        assert_eq!(root.attribute("errors"), Some("1"));
        let suite = root.first_element_child().unwrap();
        assert_eq!(suite.tag_name().name(), "testsuite");
        assert_eq!(
            suite.attribute("timestamp"),
            Some("1970-01-01T00:00:00.000Z")
        );
        assert_eq!(suite.attribute("time"), Some("1.600"));
        let cases: Vec<_> = suite
            .children()
            .filter(|n| n.has_tag_name("testcase"))
            .collect();
        assert_eq!(cases.len(), 3);
        assert_eq!(
            cases[0].attribute("name"),
            Some("POST Login <admin> (iteration 1)")
        );
        assert_eq!(cases[0].attribute("time"), Some("0.012"));
        assert!(cases[0].children().all(|c| !c.has_tag_name("failure")));

        let failure = cases[1]
            .children()
            .find(|c| c.has_tag_name("failure"))
            .unwrap();
        assert_eq!(failure.attribute("message"), Some("1 test(s) failed"));
        assert_eq!(
            failure.text(),
            Some("has <items>: expected 'a' & \"b\""),
            "control character dropped, markup escaped"
        );
        let error = cases[2]
            .children()
            .find(|c| c.has_tag_name("error"))
            .unwrap();
        assert_eq!(error.attribute("message"), Some("connection refused ]]>"));
        assert!(error.text().unwrap().contains("ReferenceError"));
    }

    #[test]
    fn one_iteration_names_have_no_suffix_and_empty_runs_are_valid() {
        let rows = vec![sample_rows().remove(0)];
        let xml = render(&Report {
            name: "c",
            rows: &rows,
            started_ms: 0,
            elapsed: Duration::ZERO,
        });
        assert!(xml.contains("<testcase name=\"POST Login &lt;admin&gt;\""));
        let empty = render(&Report {
            name: "c",
            rows: &[],
            started_ms: 0,
            elapsed: Duration::ZERO,
        });
        let doc = roxmltree::Document::parse(&empty).unwrap();
        assert_eq!(doc.root_element().attribute("tests"), Some("0"));
    }
}
