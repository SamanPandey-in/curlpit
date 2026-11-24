//! Data files for data-driven runs (Postman's "iteration data"): a CSV file with a header row,
//! or a JSON array of objects. Each row becomes one iteration's `pm.iterationData` and its
//! values resolve as `{{column}}` during that iteration (see [`crate::run`]).

use crate::DataRow;
use serde_json::Value;
use std::path::Path;

/// Read a data file. `.json` is parsed as JSON and `.csv` as CSV; any other extension is
/// sniffed (a leading `[` means JSON).
pub fn load(path: &Path) -> Result<Vec<DataRow>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{}: the data file is not UTF-8 text", path.display()))?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let json = match ext.as_str() {
        "json" => true,
        "csv" => false,
        _ => strip_bom(&text).trim_start().starts_with('['),
    };
    let rows = if json {
        parse_json(&text)
    } else {
        parse_csv(&text)
    };
    rows.map_err(|e| format!("{}: {e}", path.display()))
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// A JSON array of objects, one per iteration. Values keep their JSON types.
pub fn parse_json(text: &str) -> Result<Vec<DataRow>, String> {
    let v: Value =
        serde_json::from_str(strip_bom(text)).map_err(|e| format!("invalid JSON: {e}"))?;
    let Value::Array(items) = v else {
        return Err("a JSON data file must be an array of objects".into());
    };
    items
        .into_iter()
        .enumerate()
        .map(|(i, item)| match item {
            Value::Object(m) => Ok(m),
            other => Err(format!(
                "item {} is {}, not an object",
                i + 1,
                json_kind(&other)
            )),
        })
        .collect()
}

fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// CSV (RFC 4180): the first record is the header. Fields may be quoted (`"a, b"`, `""` for a
/// quote, line breaks allowed inside quotes); a UTF-8 BOM, CRLF line ends and blank lines are
/// accepted. Every cell is a string (an empty cell is `""`); a row shorter than the header
/// leaves the remaining columns empty, a longer one is an error.
pub fn parse_csv(text: &str) -> Result<Vec<DataRow>, String> {
    let mut records = csv_records(strip_bom(text))?.into_iter();
    let Some((_, header)) = records.next() else {
        return Ok(Vec::new());
    };
    let header: Vec<String> = header.into_iter().map(|h| h.trim().to_string()).collect();
    if let Some(i) = header.iter().position(|h| h.is_empty()) {
        return Err(format!("column {} has no name in the header row", i + 1));
    }
    let mut rows = Vec::new();
    for (line, rec) in records {
        if rec.len() > header.len() {
            return Err(format!(
                "line {line}: {} fields, but the header has {}",
                rec.len(),
                header.len()
            ));
        }
        let mut row = DataRow::new();
        let mut cells = rec.into_iter();
        for h in &header {
            row.insert(h.clone(), Value::String(cells.next().unwrap_or_default()));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Records with the line each starts on; blank lines are skipped.
fn csv_records(text: &str) -> Result<Vec<(usize, Vec<String>)>, String> {
    let mut out = Vec::new();
    let mut rec: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false; // inside a quoted field
    let mut was_quoted = false; // this field had quotes (so it isn't a blank line)
    let mut line = 1;
    let mut start_line = 1;
    let mut chars = text.chars().peekable();
    let end_record = |rec: &mut Vec<String>,
                      field: &mut String,
                      was_quoted: &mut bool,
                      out: &mut Vec<(usize, Vec<String>)>,
                      start: usize| {
        let blank = rec.is_empty() && field.is_empty() && !*was_quoted;
        rec.push(std::mem::take(field));
        let r = std::mem::take(rec);
        if !blank {
            out.push((start, r));
        }
        *was_quoted = false;
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            '"' => return Err(format!("line {line}: a quote inside an unquoted field")),
            ',' => {
                rec.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                end_record(&mut rec, &mut field, &mut was_quoted, &mut out, start_line);
                line += 1;
                start_line = line;
            }
            _ if was_quoted => {
                return Err(format!(
                    "line {line}: text after the closing quote of a field"
                ))
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(format!("line {start_line}: a quoted field is never closed"));
    }
    end_record(&mut rec, &mut field, &mut was_quoted, &mut out, start_line);
    Ok(out)
}

/// A data value as a variable's text: strings as they are, `null` as empty, anything else as
/// JSON (`3`, `true`, `{"a":1}`).
pub fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(rows: &[DataRow]) -> Vec<Vec<(String, String)>> {
        rows.iter()
            .map(|r| {
                let mut c: Vec<_> = r.iter().map(|(k, v)| (k.clone(), value_text(v))).collect();
                c.sort();
                c
            })
            .collect()
    }

    fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
        p.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn csv_basic_quotes_commas_and_crlf() {
        let rows =
            parse_csv("user,note\r\nada,\"hello, world\"\r\nbob,\"say \"\"hi\"\"\"\r\n").unwrap();
        assert_eq!(
            cells(&rows),
            vec![
                pairs(&[("note", "hello, world"), ("user", "ada")]),
                pairs(&[("note", "say \"hi\""), ("user", "bob")]),
            ]
        );
    }

    #[test]
    fn csv_bom_empty_cells_short_rows_and_blank_lines() {
        let rows = parse_csv("\u{feff}a,b,c\n1,,3\n\n4\n\"\",x,\n").unwrap();
        assert_eq!(
            cells(&rows),
            vec![
                pairs(&[("a", "1"), ("b", ""), ("c", "3")]),
                pairs(&[("a", "4"), ("b", ""), ("c", "")]),
                pairs(&[("a", ""), ("b", "x"), ("c", "")]),
            ]
        );
        assert!(
            rows[0].contains_key("a"),
            "BOM is not part of the first column"
        );
    }

    #[test]
    fn csv_newlines_inside_quotes_and_no_trailing_newline() {
        let rows = parse_csv("k,v\n\"multi\nline\",2").unwrap();
        assert_eq!(
            cells(&rows),
            vec![pairs(&[("k", "multi\nline"), ("v", "2")])]
        );
    }

    #[test]
    fn csv_errors_name_the_line() {
        let long = parse_csv("a,b\n1,2\n1,2,3\n").unwrap_err();
        assert!(
            long.contains("line 3") && long.contains("3 fields"),
            "{long}"
        );
        assert!(parse_csv("a\n\"open\n")
            .unwrap_err()
            .contains("never closed"));
        assert!(parse_csv("a\nx\"y\n").unwrap_err().contains("quote inside"));
        assert!(parse_csv("a,,c\n1,2,3\n").unwrap_err().contains("column 2"));
        assert_eq!(parse_csv("").unwrap().len(), 0);
        assert_eq!(parse_csv("only,header\n").unwrap().len(), 0);
    }

    #[test]
    fn json_rows_keep_their_types() {
        let rows =
            parse_json("\u{feff}[{\"n\": 1, \"s\": \"x\", \"z\": null, \"o\": {\"a\": [1]}}, {}]")
                .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["n"], serde_json::json!(1));
        assert_eq!(value_text(&rows[0]["n"]), "1");
        assert_eq!(value_text(&rows[0]["s"]), "x");
        assert_eq!(value_text(&rows[0]["z"]), "");
        assert_eq!(value_text(&rows[0]["o"]), "{\"a\":[1]}");
    }

    #[test]
    fn json_that_is_not_an_array_of_objects_is_an_error() {
        assert!(parse_json("{\"a\": 1}")
            .unwrap_err()
            .contains("array of objects"));
        let e = parse_json("[{\"a\": 1}, 2]").unwrap_err();
        assert!(e.contains("item 2") && e.contains("a number"), "{e}");
        assert!(parse_json("[1").unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn load_picks_the_format_by_extension_or_content() {
        let dir = std::env::temp_dir().join(format!("at-runner-data-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("d.csv");
        std::fs::write(&csv, "a\n[x]\n").unwrap();
        assert_eq!(value_text(&load(&csv).unwrap()[0]["a"]), "[x]");
        let sniff = dir.join("d.txt");
        std::fs::write(&sniff, "  [{\"a\": 2}]").unwrap();
        assert_eq!(value_text(&load(&sniff).unwrap()[0]["a"]), "2");
        let e = load(&dir.join("missing.csv")).unwrap_err();
        assert!(e.contains("missing.csv"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
