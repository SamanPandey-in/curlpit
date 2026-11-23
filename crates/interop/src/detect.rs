//! One entry point for every import format (FR-IE-01, 03, 04, 09).
//!
//! The file picker, drag and drop and paste all hand over raw text with no hint of what it is.
//! This module sniffs the top-level shape and routes to the matching reader, so the app never
//! has to know which formats exist.
use serde_json::Value;

use crate::native_json::FORMAT_TAG;
use crate::{import_har, import_json, import_postman, Imported, InteropError};

/// Read any supported import file: a native JSON collection, a HAR log, or a Postman
/// collection, environment or globals export. Malformed input is an error, never a panic.
///
/// Order matters only for what is checked first: the native tag and the HAR `log.entries`
/// array are specific; anything else falls through to the Postman reader, which owns the
/// "not something we recognise" error.
pub fn import_any(text: &str) -> Result<Imported, InteropError> {
    // A UTF-8 BOM is common on files saved by Windows tools. Strip it once here so every
    // reader behaves the same, rather than relying on each one to remember.
    let text = text.trim_start_matches('\u{feff}');
    let v: Value = serde_json::from_str(text)?;
    if v.get("format").and_then(Value::as_str) == Some(FORMAT_TAG) {
        return import_json(text);
    }
    if v.get("log")
        .and_then(|l| l.get("entries"))
        .is_some_and(Value::is_array)
    {
        return import_har(text);
    }
    import_postman(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export_collection, export_json, Collection, Item};

    const HAR: &str = r#"{"log": {"entries": [
        {"request": {"method": "GET", "url": "https://x.test/users?page=2",
                     "headers": [{"name": "Accept", "value": "application/json"}],
                     "queryString": [{"name": "page", "value": "2"}]}},
        {"request": {"method": "POST", "url": "https://x.test/users", "headers": [],
                     "queryString": [],
                     "postData": {"mimeType": "application/json", "text": "{\"a\":1}"}}}
    ]}}"#;

    const POSTMAN: &str = r#"{
        "info": {"name": "Shop"},
        "item": [
            {"name": "Users", "item": [
                {"name": "List", "request": {"method": "GET", "url": "https://x.test/users"}},
                {"name": "Admin", "item": [
                    {"name": "Purge", "request": {"method": "DELETE", "url": "https://x.test/admin"}}
                ]}
            ]},
            {"name": "Ping", "request": {"method": "GET", "url": "https://x.test/ping"}}
        ]}"#;

    fn collection(imported: Imported) -> Collection {
        match imported {
            Imported::Collection(c, _) => c,
            other => panic!("expected a collection, got {other:?}"),
        }
    }

    /// The tree reduced to what survives any format: nesting, names, methods and URLs.
    fn shape(items: &[Item], depth: usize, out: &mut Vec<String>) {
        for it in items {
            match it {
                Item::Folder { name, items } => {
                    out.push(format!("{}folder {name}", " ".repeat(depth)));
                    shape(items, depth + 1, out);
                }
                Item::Request(r) => out.push(format!(
                    "{}{} {} {}",
                    " ".repeat(depth),
                    r.method.as_str(),
                    r.name,
                    r.url
                )),
            }
        }
    }

    fn shape_of(c: &Collection) -> Vec<String> {
        let mut out = Vec::new();
        shape(&c.items, 0, &mut out);
        out
    }

    #[test]
    fn each_format_is_routed_to_its_reader() {
        let har = collection(import_any(HAR).unwrap());
        assert_eq!(har.items.len(), 2);

        let pm = collection(import_any(POSTMAN).unwrap());
        assert_eq!(pm.name, "Shop");

        let native = export_json(&pm);
        assert_eq!(collection(import_any(&native).unwrap()), pm);
    }

    #[test]
    fn postman_environments_still_route_to_the_postman_reader() {
        let env = r#"{"name": "Prod", "values": [{"key": "host", "value": "x.test"}]}"#;
        assert!(matches!(
            import_any(env).unwrap(),
            Imported::Environment(..)
        ));
    }

    #[test]
    fn a_bom_is_accepted_for_every_format() {
        let native = export_json(&collection(import_any(POSTMAN).unwrap()));
        for text in [HAR, POSTMAN, native.as_str()] {
            let with_bom = format!("\u{feff}{text}");
            assert!(
                matches!(import_any(&with_bom), Ok(Imported::Collection(..))),
                "BOM broke: {}",
                &text[..20.min(text.len())]
            );
        }
    }

    #[test]
    fn malformed_and_unrecognised_input_is_an_error_not_a_panic() {
        assert!(matches!(import_any("{nope"), Err(InteropError::Json(_))));
        assert!(matches!(import_any(""), Err(InteropError::Json(_))));
        assert!(matches!(import_any("[]"), Err(InteropError::Unknown)));
        assert!(matches!(
            import_any(r#"{"a": 1}"#),
            Err(InteropError::Unknown)
        ));
        // looks a little like HAR but has no entries array: not HAR, so not recognised
        assert!(matches!(
            import_any(r#"{"log": {"entries": "nope"}}"#),
            Err(InteropError::Unknown)
        ));
        // a different file that merely has a `format` key must not be mistaken for ours
        assert!(matches!(
            import_any(r#"{"format": "something-else", "collection": {}}"#),
            Err(InteropError::Unknown)
        ));
        // Postman v1 keeps its specific, actionable error
        assert!(matches!(
            import_any(r#"{"requests": [], "order": []}"#),
            Err(InteropError::PostmanV1)
        ));
    }

    /// Phase 5 exit criterion: a Postman collection survives import, export and import again
    /// with every request and folder intact.
    #[test]
    fn postman_survives_export_and_reimport() {
        let first = collection(import_any(POSTMAN).unwrap());
        let (exported, _) = export_collection(&first);
        let second = collection(import_any(&exported).unwrap());
        assert_eq!(shape_of(&first), shape_of(&second));
        assert_eq!(
            shape_of(&first),
            [
                "folder Users",
                " GET List https://x.test/users",
                " folder Admin",
                "  DELETE Purge https://x.test/admin",
                "GET Ping https://x.test/ping",
            ]
        );
    }

    /// Native JSON is lossless: whatever the source format, nothing changes through it.
    #[test]
    fn any_format_survives_native_json_unchanged() {
        for text in [HAR, POSTMAN] {
            let original = collection(import_any(text).unwrap());
            let back = collection(import_any(&export_json(&original)).unwrap());
            assert_eq!(back, original);
        }
    }
}
