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
    /// A GraphQL operation. Built as a JSON body `{"query": …, "variables": …}` with
    /// `Content-Type: application/json`, sent with the request's own method. `variables` is JSON
    /// text as typed (empty means none); `{{vars}}` are resolved in both fields.
    #[serde(rename = "graphql")]
    GraphQl {
        #[serde(default)]
        query: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        variables: String,
    },
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
    /// Use the auth of the enclosing folder or collection. Resolution is not built yet, so
    /// [`crate::build`] treats it as "nothing to inherit", i.e. like `None`.
    Inherit,
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
    /// OAuth 2.0. Stored only; [`crate::build`] refuses it until token fetching exists.
    /// Boxed because it is much larger than the other variants; build one with [`Auth::oauth2`].
    OAuth2(Box<OAuth2>),
    /// HTTP Digest (RFC 7616). Stored only; sending needs a 401 challenge round-trip.
    Digest {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
}

impl Auth {
    pub fn oauth2(o: OAuth2) -> Auth {
        Auth::OAuth2(Box::new(o))
    }

    /// The display name of an auth type that is stored but can't be sent yet, if this is one.
    pub fn unsupported(&self) -> Option<&'static str> {
        match self {
            Auth::OAuth2(_) => Some("OAuth 2.0"),
            Auth::Digest { .. } => Some("Digest auth"),
            _ => None,
        }
    }
}

fn default_prefix() -> String {
    "Bearer".into()
}

/// How an OAuth 2.0 access token is obtained.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2Grant {
    /// Authorization code, with PKCE when [`OAuth2::pkce`] is set (the default).
    #[default]
    AuthorizationCode,
    ClientCredentials,
    /// Resource owner password credentials (legacy).
    Password,
    /// Device authorization grant (RFC 8628).
    DeviceCode,
}

/// How the client authenticates to the token endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OAuth2ClientAuth {
    /// `Authorization: Basic base64(client_id:client_secret)`.
    #[default]
    Header,
    /// `client_id` and `client_secret` in the form body.
    Body,
}

/// OAuth 2.0 settings. Names follow Postman's v2.1 `oauth2` parameters where they map
/// (`authUrl` → `auth_url`, `accessTokenUrl` → `token_url`, `addTokenTo` → `add_token_to`, ...).
/// Tokens themselves are not stored here; they belong in a token store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth2 {
    #[serde(default)]
    pub grant_type: OAuth2Grant,
    /// Authorization endpoint (authorization code), or the device authorization endpoint
    /// (device code).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token_url: String,
    /// Where refreshes go; empty means `token_url`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub refresh_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
    /// Space-separated scopes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    /// Empty means a loopback redirect on a random local port.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub redirect_uri: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub state: String,
    /// Use PKCE (S256) with the authorization code grant.
    #[serde(default = "yes")]
    pub pkce: bool,
    /// Resource owner credentials for the password grant.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    #[serde(default)]
    pub client_auth: OAuth2ClientAuth,
    /// Where the access token goes on the request: a header or a query parameter.
    #[serde(default = "token_in_header")]
    pub add_token_to: ApiKeyLocation,
    /// Prefix of the `Authorization` value when the token goes in a header.
    #[serde(default = "default_prefix")]
    pub header_prefix: String,
}

fn token_in_header() -> ApiKeyLocation {
    ApiKeyLocation::Header
}

impl Default for OAuth2 {
    fn default() -> Self {
        Self {
            grant_type: OAuth2Grant::default(),
            auth_url: String::new(),
            token_url: String::new(),
            refresh_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            scope: String::new(),
            redirect_uri: String::new(),
            state: String::new(),
            pkce: true,
            username: String::new(),
            password: String::new(),
            client_auth: OAuth2ClientAuth::default(),
            add_token_to: ApiKeyLocation::Header,
            header_prefix: default_prefix(),
        }
    }
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
    /// Stable identity that survives renames and moves (unlike the slug file name). `at-store`
    /// assigns one when a request without it is saved; older files have none until then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: Method,
    pub url: String,
    /// JavaScript run before sending (FR-SCR). Plain values stay above the tables below so the
    /// TOML file reads top to bottom.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pre_request_script: String,
    /// JavaScript run after the response arrives (`pm.test(...)`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub test_script: String,
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

    #[test]
    fn empty_uid_is_not_written() {
        let r = Request::default();
        assert!(!serde_json::to_string(&r).unwrap().contains("uid"));
        let with = Request {
            uid: "u1".into(),
            ..Default::default()
        };
        assert!(serde_json::to_string(&with)
            .unwrap()
            .contains(r#""uid":"u1""#));
    }

    #[test]
    fn new_variants_use_the_same_tagging() {
        let cases = [
            (Auth::Inherit, r#"{"type":"inherit"}"#),
            (
                Auth::Digest {
                    username: "u".into(),
                    password: "p".into(),
                },
                r#"{"type":"digest","username":"u","password":"p"}"#,
            ),
            (
                Auth::oauth2(OAuth2 {
                    grant_type: OAuth2Grant::ClientCredentials,
                    token_url: "https://t.test/token".into(),
                    client_id: "c".into(),
                    ..Default::default()
                }),
                r#"{"type":"oauth2","grant_type":"client_credentials","token_url":"https://t.test/token","client_id":"c","pkce":true,"client_auth":"header","add_token_to":"header","header_prefix":"Bearer"}"#,
            ),
        ];
        for (auth, json) in cases {
            assert_eq!(serde_json::to_string(&auth).unwrap(), json);
            assert_eq!(serde_json::from_str::<Auth>(json).unwrap(), auth);
        }
        // a hand-written minimal oauth2 entry fills in defaults
        let a: Auth = serde_json::from_str(r#"{"type":"oauth2"}"#).unwrap();
        assert_eq!(a, Auth::oauth2(OAuth2::default()));

        let g = Body::GraphQl {
            query: "{ a }".into(),
            variables: String::new(),
        };
        let s = serde_json::to_string(&g).unwrap();
        assert_eq!(s, r#"{"type":"graphql","query":"{ a }"}"#);
        assert_eq!(serde_json::from_str::<Body>(&s).unwrap(), g);
    }

    #[test]
    fn old_json_request_still_loads() {
        // shape written before uid/oauth2/digest/graphql existed
        let old = r#"{"id":"","name":"Old","method":"POST","url":"https://x.test",
            "params":[],"headers":[{"key":"A","value":"1","description":"","enabled":true}],
            "body":{"type":"json","raw":"{}"},"auth":{"type":"basic","username":"u","password":"p"},
            "settings":{"timeout_ms":5000,"follow_redirects":true,"verify_tls":false}}"#;
        let r: Request = serde_json::from_str(old).unwrap();
        assert_eq!(r.uid, "");
        assert_eq!(r.name, "Old");
        assert!(matches!(r.auth, Auth::Basic { .. }));
        assert_eq!(r.settings.timeout_ms, 5000);
    }
}
