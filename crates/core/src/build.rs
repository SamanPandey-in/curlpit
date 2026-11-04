//! Request build pipeline (FR-ENG-02): resolve variables, build URL with enabled
//! params, headers, auth, and serialize the body into a transport-neutral form.
use crate::encode::{base64, percent_encode};
use crate::models::*;
use crate::vars::Variables;

#[derive(Debug, Clone, PartialEq)]
pub enum BuiltPart {
    Text { name: String, value: String },
    File { name: String, path: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuiltBody {
    None,
    /// Content-Type (if any) is already in `Built::headers`.
    Bytes(Vec<u8>),
    /// The HTTP engine sets the multipart Content-Type with its boundary.
    Multipart(Vec<BuiltPart>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: BuiltBody,
    /// Variable names that could not be resolved anywhere in the request.
    pub unresolved: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BuildError {
    #[error("URL is empty")]
    EmptyUrl,
    #[error("URL must start with http:// or https:// (got `{0}`)")]
    InvalidUrl(String),
}

fn has_header(h: &[(String, String)], name: &str) -> bool {
    h.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

pub fn build(req: &Request, vars: &Variables) -> Result<Built, BuildError> {
    build_inner(req, vars, true)
}

/// Like [`build`], but the URL may still start with an unresolved `{{variable}}`. Used for code
/// snippets that keep placeholders; never for sending.
pub fn build_template(req: &Request, vars: &Variables) -> Result<Built, BuildError> {
    build_inner(req, vars, false)
}

fn build_inner(req: &Request, vars: &Variables, require_scheme: bool) -> Result<Built, BuildError> {
    let mut unresolved: Vec<String> = Vec::new();
    let mut r = |s: &str| -> String {
        let res = vars.resolve(s);
        for u in res.unresolved {
            if !unresolved.contains(&u) {
                unresolved.push(u);
            }
        }
        res.text
    };

    // ---- URL ----
    let raw_url = r(req.url.trim());
    if raw_url.is_empty() {
        return Err(BuildError::EmptyUrl);
    }
    let lower = raw_url.to_ascii_lowercase();
    if require_scheme && !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(BuildError::InvalidUrl(raw_url));
    }
    let (mut url, fragment) = match raw_url.split_once('#') {
        Some((b, f)) => (b.to_string(), Some(f.to_string())),
        None => (raw_url, None),
    };

    let mut pairs: Vec<(String, String)> = Vec::new();
    for p in req.params.iter().filter(|p| p.enabled && !p.key.is_empty()) {
        pairs.push((r(&p.key), r(&p.value)));
    }

    // ---- headers ----
    let mut headers: Vec<(String, String)> = Vec::new();
    for h in req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
    {
        headers.push((r(&h.key), r(&h.value)));
    }

    // ---- auth (applied at send time, FR-AUTH-02) ----
    let user_set_auth = has_header(&headers, "authorization");
    match &req.auth {
        Auth::None => {}
        Auth::Basic { username, password } => {
            if !user_set_auth {
                let cred = format!("{}:{}", r(username), r(password));
                headers.push((
                    "Authorization".into(),
                    format!("Basic {}", base64(cred.as_bytes())),
                ));
            }
        }
        Auth::Bearer { token } => {
            if !user_set_auth {
                headers.push(("Authorization".into(), format!("Bearer {}", r(token))));
            }
        }
        Auth::Jwt { token, prefix } => {
            if !user_set_auth {
                let prefix = r(prefix);
                let token = r(token);
                let value = if prefix.is_empty() {
                    token
                } else {
                    format!("{prefix} {token}")
                };
                headers.push(("Authorization".into(), value));
            }
        }
        Auth::ApiKey {
            key,
            value,
            location,
        } => {
            let (k, v) = (r(key), r(value));
            if !k.is_empty() {
                match location {
                    ApiKeyLocation::Header => headers.push((k, v)),
                    ApiKeyLocation::Query => pairs.push((k, v)),
                }
            }
        }
        Auth::Custom { headers: hs } => {
            for h in hs.iter().filter(|h| h.enabled && !h.key.is_empty()) {
                headers.push((r(&h.key), r(&h.value)));
            }
        }
    }

    // ---- append query string ----
    if !pairs.is_empty() {
        let sep = if !url.contains('?') {
            Some('?')
        } else if url.ends_with('?') || url.ends_with('&') {
            None
        } else {
            Some('&')
        };
        if let Some(c) = sep {
            url.push(c);
        }
        let qs: Vec<String> = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
            .collect();
        url.push_str(&qs.join("&"));
    }
    if let Some(f) = fragment {
        url.push('#');
        url.push_str(&f);
    }

    // ---- body ----
    let mut default_ct: Option<&str> = None;
    let body = match &req.body {
        Body::None => BuiltBody::None,
        Body::Json { raw } => {
            default_ct = Some("application/json");
            BuiltBody::Bytes(r(raw).into_bytes())
        }
        Body::Xml { raw } => {
            default_ct = Some("application/xml");
            BuiltBody::Bytes(r(raw).into_bytes())
        }
        Body::Html { raw } => {
            default_ct = Some("text/html");
            BuiltBody::Bytes(r(raw).into_bytes())
        }
        Body::Text { raw } => {
            default_ct = Some("text/plain");
            BuiltBody::Bytes(r(raw).into_bytes())
        }
        Body::UrlEncoded { fields } => {
            default_ct = Some("application/x-www-form-urlencoded");
            let s: Vec<String> = fields
                .iter()
                .filter(|f| f.enabled && !f.key.is_empty())
                .map(|f| {
                    format!(
                        "{}={}",
                        percent_encode(&r(&f.key)),
                        percent_encode(&r(&f.value))
                    )
                })
                .collect();
            BuiltBody::Bytes(s.join("&").into_bytes())
        }
        Body::FormData { fields } => {
            let parts = fields
                .iter()
                .filter(|f| f.enabled && !f.key.is_empty())
                .map(|f| match &f.value {
                    FormValue::Text { value } => BuiltPart::Text {
                        name: r(&f.key),
                        value: r(value),
                    },
                    FormValue::File { path } => BuiltPart::File {
                        name: r(&f.key),
                        path: r(path),
                    },
                })
                .collect();
            BuiltBody::Multipart(parts)
        }
    };
    if let Some(ct) = default_ct {
        if !has_header(&headers, "content-type") {
            headers.push(("Content-Type".into(), ct.into()));
        }
    }

    // Placeholders in params were percent-encoded with everything else; in a template they
    // should read as `{{name}}` again.
    let url = if require_scheme {
        url
    } else {
        url.replace("%7B%7B", "{{").replace("%7D%7D", "}}")
    };

    Ok(Built {
        method: req.method,
        url,
        headers,
        body,
        unresolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(k: &str, v: &str) -> KeyValue {
        KeyValue::new(k, v)
    }

    #[test]
    fn template_build_keeps_placeholder_urls() {
        let req = Request {
            url: "{{base}}/users".into(),
            params: vec![kv("q", "{{term}}")],
            ..Default::default()
        };
        let vars = Variables::default();
        assert!(matches!(build(&req, &vars), Err(BuildError::InvalidUrl(_))));
        let b = build_template(&req, &vars).unwrap();
        assert_eq!(b.url, "{{base}}/users?q={{term}}");
        assert_eq!(b.unresolved, vec!["base".to_string(), "term".to_string()]);
        // an empty URL is still an error
        let empty = Request::default();
        assert_eq!(build_template(&empty, &vars), Err(BuildError::EmptyUrl));
    }

    #[test]
    fn url_params_and_fragment() {
        let req = Request {
            url: "https://x.test/a?x=1#frag".into(),
            params: vec![
                kv("q", "a b"),
                KeyValue {
                    enabled: false,
                    ..kv("off", "1")
                },
            ],
            ..Default::default()
        };
        let b = build(&req, &Variables::default()).unwrap();
        assert_eq!(b.url, "https://x.test/a?x=1&q=a%20b#frag");
    }

    #[test]
    fn variables_in_every_field() {
        let mut v = Variables::default();
        v.environment.insert("host".into(), "api.test".into());
        v.environment.insert("tok".into(), "T0K".into());
        let req = Request {
            url: "https://{{host}}/v1".into(),
            headers: vec![kv("X-Id", "{{nope}}")],
            auth: Auth::Bearer {
                token: "{{tok}}".into(),
            },
            ..Default::default()
        };
        let b = build(&req, &v).unwrap();
        assert_eq!(b.url, "https://api.test/v1");
        assert!(b
            .headers
            .contains(&("Authorization".into(), "Bearer T0K".into())));
        assert!(b.headers.contains(&("X-Id".into(), "{{nope}}".into())));
        assert_eq!(b.unresolved, vec!["nope".to_string()]);
    }

    #[test]
    fn basic_auth_header() {
        let req = Request {
            url: "http://x.test".into(),
            auth: Auth::Basic {
                username: "user".into(),
                password: "pass".into(),
            },
            ..Default::default()
        };
        let b = build(&req, &Variables::default()).unwrap();
        assert!(b
            .headers
            .contains(&("Authorization".into(), "Basic dXNlcjpwYXNz".into())));
    }

    #[test]
    fn user_authorization_header_wins() {
        let req = Request {
            url: "http://x.test".into(),
            headers: vec![kv("authorization", "Custom abc")],
            auth: Auth::Bearer {
                token: "zzz".into(),
            },
            ..Default::default()
        };
        let b = build(&req, &Variables::default()).unwrap();
        let n = b
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn api_key_in_query() {
        let req = Request {
            url: "http://x.test/p".into(),
            auth: Auth::ApiKey {
                key: "api_key".into(),
                value: "s3 cret".into(),
                location: ApiKeyLocation::Query,
            },
            ..Default::default()
        };
        assert_eq!(
            build(&req, &Variables::default()).unwrap().url,
            "http://x.test/p?api_key=s3%20cret"
        );
    }

    #[test]
    fn json_body_sets_content_type_unless_user_did() {
        let mut req = Request {
            method: Method::Post,
            url: "http://x.test".into(),
            body: Body::Json {
                raw: "{\"a\":1}".into(),
            },
            ..Default::default()
        };
        let b = build(&req, &Variables::default()).unwrap();
        assert_eq!(b.body, BuiltBody::Bytes(b"{\"a\":1}".to_vec()));
        assert!(b
            .headers
            .contains(&("Content-Type".into(), "application/json".into())));

        req.headers
            .push(kv("content-type", "application/vnd.api+json"));
        let b = build(&req, &Variables::default()).unwrap();
        assert_eq!(
            b.headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .count(),
            1
        );
    }

    #[test]
    fn urlencoded_body() {
        let req = Request {
            method: Method::Post,
            url: "http://x.test".into(),
            body: Body::UrlEncoded {
                fields: vec![kv("a", "1 2"), kv("b", "&")],
            },
            ..Default::default()
        };
        let b = build(&req, &Variables::default()).unwrap();
        assert_eq!(b.body, BuiltBody::Bytes(b"a=1%202&b=%26".to_vec()));
    }

    #[test]
    fn multipart_parts() {
        let req = Request {
            method: Method::Post,
            url: "http://x.test".into(),
            body: Body::FormData {
                fields: vec![
                    FormField {
                        key: "t".into(),
                        value: FormValue::Text { value: "v".into() },
                        enabled: true,
                    },
                    FormField {
                        key: "f".into(),
                        value: FormValue::File {
                            path: "/tmp/a.txt".into(),
                        },
                        enabled: true,
                    },
                    FormField {
                        key: "off".into(),
                        value: FormValue::Text { value: "x".into() },
                        enabled: false,
                    },
                ],
            },
            ..Default::default()
        };
        let b = build(&req, &Variables::default()).unwrap();
        assert_eq!(
            b.body,
            BuiltBody::Multipart(vec![
                BuiltPart::Text {
                    name: "t".into(),
                    value: "v".into()
                },
                BuiltPart::File {
                    name: "f".into(),
                    path: "/tmp/a.txt".into()
                },
            ])
        );
    }

    #[test]
    fn bad_urls() {
        let v = Variables::default();
        assert_eq!(
            build(
                &Request {
                    url: "  ".into(),
                    ..Default::default()
                },
                &v
            ),
            Err(BuildError::EmptyUrl)
        );
        assert!(matches!(
            build(
                &Request {
                    url: "ftp://x".into(),
                    ..Default::default()
                },
                &v
            ),
            Err(BuildError::InvalidUrl(_))
        ));
        // unresolved variable in the host leaves a non-URL -> clear error, not a panic
        assert!(matches!(
            build(
                &Request {
                    url: "{{base}}/x".into(),
                    ..Default::default()
                },
                &v
            ),
            Err(BuildError::InvalidUrl(_))
        ));
    }
}
