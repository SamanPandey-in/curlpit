//! Native JSON interchange (FR-IE-04 import, FR-IE-06 export).
//!
//! Round-trippable JSON for the app's own `Collection` tree. Unlike the Postman
//! importer, this format is our own, so it is lossless: `import_json(export_json(c))`
//! reproduces `c` exactly. A self-describing, versioned wrapper makes the format
//! detectable and lets future readers migrate older payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Collection, Imported, InteropError};

/// Marker stored in every exported file so the format can be recognised on import.
pub(crate) const FORMAT_TAG: &str = "api-tester-collection";
/// Current wrapper schema version.
const FORMAT_VERSION: u32 = 1;

/// Self-describing envelope around a `Collection` (used for export).
#[derive(Serialize, Deserialize)]
struct Wrapper {
    format: String,
    version: u32,
    collection: Collection,
}

/// Serialize a collection to pretty native JSON (FR-IE-06).
pub fn export_json(col: &Collection) -> String {
    let wrapper = Wrapper {
        format: FORMAT_TAG.to_string(),
        version: FORMAT_VERSION,
        collection: col.clone(),
    };
    // Our own types always serialize cleanly, so the result is never an error.
    serde_json::to_string_pretty(&wrapper).expect("Collection serializes to JSON")
}

/// Parse native JSON produced by [`export_json`] (FR-IE-04).
///
/// Errors: invalid JSON -> [`InteropError::Json`]; valid JSON whose `format` field
/// is missing or wrong -> [`InteropError::Unknown`].
pub fn import_json(text: &str) -> Result<Imported, InteropError> {
    // Parse loosely first so a missing/wrong `format` is a clean `Unknown`
    // (a self-describing-format mismatch), not a serde "missing field" error.
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT_TAG) {
        return Err(InteropError::Unknown);
    }
    let collection: Collection =
        serde_json::from_value(value.get("collection").cloned().unwrap_or(Value::Null))?;
    // No warnings on our own lossless format; the vec is here for forward-compat.
    Ok(Imported::Collection(collection, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Item;
    use at_core::{Auth, Body, KeyValue, Method, Request};

    fn sample_collection() -> Collection {
        let json_req = Request {
            id: "r1".into(),
            name: "Create user".into(),
            method: Method::Post,
            url: "https://api.example.com/users".into(),
            body: Body::Json {
                raw: "{\"name\":\"ada\"}".into(),
            },
            auth: Auth::Bearer {
                token: "secret-token".into(),
            },
            ..Default::default()
        };
        let form_req = Request {
            id: "r2".into(),
            name: "Login".into(),
            method: Method::Post,
            url: "https://api.example.com/login".into(),
            body: Body::UrlEncoded {
                fields: vec![
                    KeyValue::new("user", "ada"),
                    KeyValue::new("pass", "hunter2"),
                ],
            },
            ..Default::default()
        };
        let get_req = Request {
            id: "r3".into(),
            name: "List".into(),
            method: Method::Get,
            url: "https://api.example.com/users".into(),
            ..Default::default()
        };
        Collection {
            name: "My API".into(),
            items: vec![
                Item::Request(get_req),
                Item::Folder {
                    name: "Users".into(),
                    items: vec![
                        Item::Request(json_req),
                        Item::Folder {
                            name: "Auth".into(),
                            items: vec![Item::Request(form_req)],
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn round_trips_nested_collection() {
        let col = sample_collection();
        let text = export_json(&col);
        match import_json(&text).expect("import succeeds") {
            Imported::Collection(recovered, warnings) => {
                assert_eq!(recovered, col);
                assert!(warnings.is_empty());
            }
            other => panic!("expected a collection, got {other:?}"),
        }
    }

    #[test]
    fn export_contains_self_describing_wrapper() {
        let text = export_json(&sample_collection());
        assert!(text.contains("\"format\": \"api-tester-collection\""));
        assert!(text.contains("\"version\": 1"));
    }

    #[test]
    fn rejects_json_without_format_wrapper() {
        // Valid JSON, but not our envelope.
        let err = import_json("{\"name\":\"x\",\"items\":[]}").unwrap_err();
        assert!(matches!(err, InteropError::Unknown));
    }

    #[test]
    fn rejects_wrong_format_tag() {
        let text =
            r#"{"format":"something-else","version":1,"collection":{"name":"x","items":[]}}"#;
        assert!(matches!(import_json(text), Err(InteropError::Unknown)));
    }

    #[test]
    fn rejects_non_json() {
        assert!(matches!(
            import_json("this is not json"),
            Err(InteropError::Json(_))
        ));
    }
}
