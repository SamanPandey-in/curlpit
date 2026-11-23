//! Import HAR (HTTP Archive) files (FR-IE-03).
//!
//! HAR is the browser dev-tools export format: a top-level object with `.log.entries[]`, each
//! entry wrapping a captured `.request`. HAR has no folders, so we produce a flat collection with
//! one request per entry. Parsing is tolerant (`serde_json::Value`, every field optional) and we
//! never crash on malformed input (FR-IE-09): anything we can't represent is skipped or
//! approximated and reported as a warning.

use at_core::{Body, FormField, FormValue, KeyValue, Method, Request};
use serde_json::Value;

use crate::{Collection, Imported, InteropError, Item};

/// Read a HAR document into a flat collection (one request per `log.entries[]` entry).
pub fn import_har(text: &str) -> Result<Imported, InteropError> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    let Some(entries) = v.pointer("/log/entries").and_then(Value::as_array) else {
        return Err(InteropError::Unknown);
    };

    let mut items = Vec::new();
    let mut warnings = Vec::new();
    for entry in entries {
        let req = entry.get("request").unwrap_or(&Value::Null);
        let method_text = str_of(req, "method").unwrap_or_default();
        let Some(method) = method_of(&method_text) else {
            warnings.push(format!(
                "skipped an entry with unsupported method \"{method_text}\""
            ));
            continue;
        };
        let url = str_of(req, "url").unwrap_or_default();
        items.push(Item::Request(Request {
            name: short_name(method, &url),
            method,
            url: url.clone(),
            params: kv_list(req.get("queryString")),
            headers: headers_of(req.get("headers")),
            body: body_of(req.get("postData"), &mut warnings),
            ..Default::default()
        }));
    }

    Ok(Imported::Collection(
        Collection {
            name: "Imported from HAR".into(),
            items,
        },
        warnings,
    ))
}

/// A short, readable name: `"<METHOD> <path>"`, falling back to the full URL.
fn short_name(method: Method, url: &str) -> String {
    format!("{} {}", method.as_str(), url_path(url))
}

/// The path portion of a URL (no scheme, host or query). Falls back to the whole URL when there
/// is no clear path, so we stay simple and dependency-free.
fn url_path(url: &str) -> String {
    let after_scheme = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
    };
    let Some(slash) = after_scheme.find('/') else {
        return url.to_string();
    };
    let path = after_scheme[slash..].split('?').next().unwrap_or("");
    if path.is_empty() {
        url.to_string()
    } else {
        path.to_string()
    }
}

/// Headers as `{name, value}` pairs, skipping HTTP/2 pseudo-headers (names starting with `:`).
fn headers_of(v: Option<&Value>) -> Vec<KeyValue> {
    let Some(list) = v.and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|h| {
            let name = str_of(h, "name")?;
            if name.starts_with(':') {
                return None;
            }
            Some(KeyValue::new(&name, &scalar(h.get("value"))))
        })
        .collect()
}

/// Generic `[{name, value}]` list (used for queryString).
fn kv_list(v: Option<&Value>) -> Vec<KeyValue> {
    let Some(list) = v.and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|e| {
            let name = str_of(e, "name")?;
            Some(KeyValue::new(&name, &scalar(e.get("value"))))
        })
        .collect()
}

fn body_of(pd: Option<&Value>, warnings: &mut Vec<String>) -> Body {
    let Some(pd) = pd else {
        return Body::None;
    };
    let mime_full = str_of(pd, "mimeType").unwrap_or_default();
    let mime = mime_full
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let text = str_of(pd, "text").unwrap_or_default();
    match mime.as_str() {
        "application/json" => Body::Json { raw: text },
        "application/xml" | "text/xml" => Body::Xml { raw: text },
        "text/html" => Body::Html { raw: text },
        "application/x-www-form-urlencoded" => Body::UrlEncoded {
            fields: urlencoded_fields(pd, &text),
        },
        "multipart/form-data" => Body::FormData {
            fields: form_fields(pd),
        },
        other if other.starts_with("text/") => Body::Text { raw: text },
        _ => {
            if text.is_empty() {
                Body::None
            } else {
                warnings.push(format!(
                    "a body with mime type \"{mime_full}\" was imported as plain text"
                ));
                Body::Text { raw: text }
            }
        }
    }
}

/// Fields of a urlencoded body: from `postData.params`, or by parsing the raw `text` when params
/// are absent.
fn urlencoded_fields(pd: &Value, text: &str) -> Vec<KeyValue> {
    if let Some(params) = pd.get("params").and_then(Value::as_array) {
        if !params.is_empty() {
            return params
                .iter()
                .filter_map(|p| {
                    let name = str_of(p, "name")?;
                    Some(KeyValue::new(&name, &scalar(p.get("value"))))
                })
                .collect();
        }
    }
    parse_query(text)
}

/// Parse `a=1&b=2` into key/value pairs (no percent-decoding; values are kept as written).
fn parse_query(text: &str) -> Vec<KeyValue> {
    text.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => KeyValue::new(k, v),
            None => KeyValue::new(pair, ""),
        })
        .collect()
}

/// Multipart form fields from `postData.params`, all as text values (HAR carries no file paths).
fn form_fields(pd: &Value) -> Vec<FormField> {
    let Some(params) = pd.get("params").and_then(Value::as_array) else {
        return Vec::new();
    };
    params
        .iter()
        .filter_map(|p| {
            let name = str_of(p, "name")?;
            Some(FormField {
                key: name,
                value: FormValue::Text {
                    value: scalar(p.get("value")),
                },
                enabled: true,
            })
        })
        .collect()
}

fn method_of(s: &str) -> Option<Method> {
    Some(match s.to_ascii_uppercase().as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        "OPTIONS" => Method::Options,
        "HEAD" => Method::Head,
        "TRACE" => Method::Trace,
        "CONNECT" => Method::Connect,
        _ => return None,
    })
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// A JSON value as text: strings as-is, numbers/bools printed, null/absent empty.
fn scalar(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAR: &str = r#"{
      "log": {
        "version": "1.2",
        "creator": {"name": "test", "version": "1.0"},
        "entries": [
          {"request": {
            "method": "GET",
            "url": "https://api.test/pets?limit=10&tag=dog",
            "headers": [
              {"name": "Accept", "value": "application/json"},
              {"name": ":authority", "value": "api.test"}
            ],
            "queryString": [
              {"name": "limit", "value": "10"},
              {"name": "tag", "value": "dog"}
            ]
          }},
          {"request": {
            "method": "post",
            "url": "https://api.test/pets",
            "headers": [],
            "queryString": [],
            "postData": {
              "mimeType": "application/json; charset=utf-8",
              "text": "{\"name\":\"rex\"}"
            }
          }},
          {"request": {
            "method": "POST",
            "url": "https://api.test/login",
            "headers": [],
            "queryString": [],
            "postData": {
              "mimeType": "application/x-www-form-urlencoded",
              "params": [
                {"name": "user", "value": "u"},
                {"name": "pass", "value": "p"}
              ]
            }
          }}
        ]
      }
    }"#;

    fn import(text: &str) -> (Collection, Vec<String>) {
        match import_har(text).unwrap() {
            Imported::Collection(c, w) => (c, w),
            other => panic!("expected a collection, got {other:?}"),
        }
    }

    fn request<'a>(c: &'a Collection, name: &str) -> &'a Request {
        c.items
            .iter()
            .find_map(|it| match it {
                Item::Request(r) if r.name == name => Some(r),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} not found"))
    }

    #[test]
    fn imports_one_request_per_entry() {
        let (c, w) = import(HAR);
        assert_eq!(c.name, "Imported from HAR");
        assert_eq!(c.items.len(), 3);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn get_maps_method_url_params_and_headers() {
        let (c, _) = import(HAR);
        let r = request(&c, "GET /pets");
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.url, "https://api.test/pets?limit=10&tag=dog");
        assert_eq!(
            r.params,
            vec![KeyValue::new("limit", "10"), KeyValue::new("tag", "dog")]
        );
        // the :authority pseudo-header is dropped, Accept is kept
        assert_eq!(r.headers, vec![KeyValue::new("Accept", "application/json")]);
    }

    #[test]
    fn json_post_becomes_json_body() {
        let (c, _) = import(HAR);
        let r = request(&c, "POST /pets");
        assert_eq!(r.method, Method::Post, "lowercase method accepted");
        assert_eq!(
            r.body,
            Body::Json {
                raw: "{\"name\":\"rex\"}".into()
            }
        );
    }

    #[test]
    fn urlencoded_post_becomes_urlencoded_body() {
        let (c, _) = import(HAR);
        let r = request(&c, "POST /login");
        assert_eq!(
            r.body,
            Body::UrlEncoded {
                fields: vec![KeyValue::new("user", "u"), KeyValue::new("pass", "p")]
            }
        );
    }

    #[test]
    fn bogus_method_entry_is_skipped_with_a_warning() {
        let doc = r#"{"log": {"entries": [
          {"request": {"method": "FROBNICATE", "url": "https://x.test/a"}},
          {"request": {"method": "GET", "url": "https://x.test/b"}}
        ]}}"#;
        let (c, w) = import(doc);
        assert_eq!(c.items.len(), 1, "only the valid entry imports");
        assert_eq!(request(&c, "GET /b").url, "https://x.test/b");
        assert!(w.iter().any(|w| w.contains("FROBNICATE")), "{w:?}");
    }

    #[test]
    fn unknown_mime_body_becomes_text_with_a_warning() {
        let doc = r#"{"log": {"entries": [
          {"request": {"method": "POST", "url": "https://x.test/odd",
            "postData": {"mimeType": "application/vnd.custom", "text": "blob"}}}
        ]}}"#;
        let (c, w) = import(doc);
        assert_eq!(
            request(&c, "POST /odd").body,
            Body::Text { raw: "blob".into() }
        );
        assert!(
            w.iter().any(|w| w.contains("application/vnd.custom")),
            "{w:?}"
        );
    }

    #[test]
    fn rejects_malformed_and_non_har_input() {
        assert!(matches!(import_har("{nope"), Err(InteropError::Json(_))));
        assert!(matches!(
            import_har("{\"a\": 1}"),
            Err(InteropError::Unknown)
        ));
        assert!(matches!(
            import_har(r#"{"log": {}}"#),
            Err(InteropError::Unknown)
        ));
    }
}
