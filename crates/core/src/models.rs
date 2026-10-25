//! Data models (SRS section 6). Serde-friendly; these are what gets saved to disk.
use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
    Trace,
    Connect,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Options => "OPTIONS",
            Method::Head => "HEAD",
            Method::Trace => "TRACE",
            Method::Connect => "CONNECT",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl KeyValue {
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            description: String::new(),
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FormValue {
    Text { value: String },
    File { path: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub key: String,
    pub value: FormValue,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Body {
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "json")]
    Json { raw: String },
    #[serde(rename = "xml")]
    Xml { raw: String },
    #[serde(rename = "html")]
    Html { raw: String },
    #[serde(rename = "text")]
    Text { raw: String },
    #[serde(rename = "urlencoded")]
    UrlEncoded { fields: Vec<KeyValue> },
    #[serde(rename = "formdata")]
    FormData { fields: Vec<FormField> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyLocation {
    Header,
    Query,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Auth {
    #[default]
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    Jwt {
        token: String,
        #[serde(default = "default_prefix")]
        prefix: String,
    },
    ApiKey {
        key: String,
        value: String,
        location: ApiKeyLocation,
    },
    Custom {
        headers: Vec<KeyValue>,
    },
}

fn default_prefix() -> String {
    "Bearer".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestSettings {
    /// 0 means no timeout.
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "yes")]
    pub follow_redirects: bool,
    #[serde(default = "yes")]
    pub verify_tls: bool,
}

fn default_timeout() -> u64 {
    30_000
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout_ms: default_timeout(),
            follow_redirects: true,
            verify_tls: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: Method,
    pub url: String,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub settings: RequestSettings,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_json_request_parses_with_defaults() {
        let r: Request = serde_json::from_str(r#"{"url":"https://x.test"}"#).unwrap();
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.body, Body::None);
        assert_eq!(r.settings.timeout_ms, 30_000);
        assert!(r.settings.verify_tls);
    }

    #[test]
    fn full_roundtrip() {
        let r = Request {
            method: Method::Post,
            url: "https://x.test/{{p}}".into(),
            body: Body::Json {
                raw: "{\"a\":1}".into(),
            },
            auth: Auth::ApiKey {
                key: "k".into(),
                value: "v".into(),
                location: ApiKeyLocation::Query,
            },
            params: vec![KeyValue::new("q", "1")],
            ..Default::default()
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: Request = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}
