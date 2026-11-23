//! Collection- and folder-level settings and saved response examples (G-03, G-08).
//!
//! These are data only for now: `at-store` reads and writes `collection.toml` / `folder.toml`,
//! but nothing applies them to requests yet (that is the request → folder → collection
//! resolution walk), and examples have no storage yet.
use serde::{Deserialize, Serialize};

use crate::env::EnvVar;
use crate::models::{Auth, KeyValue, Request};

fn is_inherit(a: &Auth) -> bool {
    matches!(a, Auth::Inherit)
}

fn inherit() -> Auth {
    Auth::Inherit
}

fn is_none(a: &Auth) -> bool {
    matches!(a, Auth::None)
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// Settings shared by every request in a collection, kept in `collection.toml` at the
/// collection root.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectionMeta {
    /// Display name.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pre_request_script: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub test_script: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<EnvVar>,
    /// Default headers; a request's own header with the same name wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    /// What requests and folders that use [`Auth::Inherit`] get. `Inherit` here means none.
    #[serde(default, skip_serializing_if = "is_none")]
    pub auth: Auth,
    /// Whether its scripts may run (G-16). Imports that bring scripts are written as
    /// untrusted until the user says otherwise; collections made in the app, and every
    /// collection from before this field existed, are trusted. Only written when false.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub trusted: bool,
}

impl Default for CollectionMeta {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            pre_request_script: String::new(),
            test_script: String::new(),
            variables: Vec::new(),
            headers: Vec::new(),
            auth: Auth::None,
            trusted: true,
        }
    }
}

/// Settings for the requests in one folder, kept in `folder.toml` inside the folder. A
/// folder's auth defaults to inheriting from its parent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FolderMeta {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pre_request_script: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub test_script: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<EnvVar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default = "inherit", skip_serializing_if = "is_inherit")]
    pub auth: Auth,
}

impl Default for FolderMeta {
    fn default() -> Self {
        Self {
            description: String::new(),
            pre_request_script: String::new(),
            test_script: String::new(),
            variables: Vec::new(),
            headers: Vec::new(),
            auth: Auth::Inherit,
        }
    }
}

/// A saved response example ("save response as example"). Data only; no storage yet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Example {
    #[serde(default)]
    pub name: String,
    /// HTTP status code.
    #[serde(default)]
    pub status: u16,
    /// Empty means "use the `Content-Type` header, if any".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_type: String,
    /// The response body as text.
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    /// The request that produced the response, when it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Request>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Body;

    #[test]
    fn empty_meta_files_parse_to_defaults() {
        let c: CollectionMeta = serde_json::from_str("{}").unwrap();
        assert_eq!(c, CollectionMeta::default());
        assert_eq!(c.auth, Auth::None);
        assert!(c.trusted, "collections from before G-16 count as trusted");
        let untrusted = CollectionMeta {
            trusted: false,
            ..Default::default()
        };
        let s = serde_json::to_string(&untrusted).unwrap();
        assert!(s.contains("\"trusted\":false"));
        assert_eq!(
            serde_json::from_str::<CollectionMeta>(&s).unwrap(),
            untrusted
        );
        assert!(!serde_json::to_string(&c).unwrap().contains("trusted"));
        let f: FolderMeta = serde_json::from_str("{}").unwrap();
        assert_eq!(f, FolderMeta::default());
        assert_eq!(f.auth, Auth::Inherit);
        // the defaults serialize to almost nothing
        assert_eq!(serde_json::to_string(&f).unwrap(), "{}");
    }

    #[test]
    fn example_roundtrip_with_request_snapshot() {
        let e = Example {
            name: "200 OK".into(),
            status: 200,
            content_type: "application/json".into(),
            body: "{\"ok\":true}".into(),
            headers: vec![KeyValue::new("X-A", "1")],
            request: Some(Request {
                url: "https://x.test".into(),
                body: Body::GraphQl {
                    query: "{ a }".into(),
                    variables: String::new(),
                },
                ..Default::default()
            }),
        };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Example>(&s).unwrap(), e);
    }
}
