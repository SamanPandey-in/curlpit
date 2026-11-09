//! Import/export (FR-IMP): Postman Collection v2.0/v2.1, Postman environments and globals.
//!
//! Parsing is deliberately tolerant (`serde_json::Value`, every field optional) because real
//! Postman exports vary a lot. Anything we can't represent is skipped and reported as a
//! warning, never silently dropped.
use std::path::Path;

use at_core::{
    ApiKeyLocation, Auth, Body, EnvVar, Environment, FormField, FormValue, KeyValue, Method,
    Request, RequestSettings,
};
use at_store::{Node, Store, StoreError};
use serde_json::{json, Map, Value};

pub const POSTMAN_SCHEMA_V21: &str =
    "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

#[derive(Debug, thiserror::Error)]
pub enum InteropError {
    #[error("not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("this is a Postman v1 collection; open it in Postman and export it as v2.1")]
    PostmanV1,
    #[error("not a Postman collection, environment or globals file")]
    Unknown,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A collection tree, independent of where it lives.
// Trees are small and short-lived (import/export only); boxing every request would add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Folder { name: String, items: Vec<Item> },
    Request(Request),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Collection {
    pub name: String,
    pub items: Vec<Item>,
}

/// Something read from an import file, plus notes about what could not be brought over.
#[derive(Clone, Debug, PartialEq)]
pub enum Imported {
    Collection(Collection, Vec<String>),
    Environment(Environment, Vec<String>),
    Globals(Vec<EnvVar>, Vec<String>),
}

// ---------------------------------------------------------------- import

/// Detect and read a Postman collection, environment or globals export.
pub fn import_postman(text: &str) -> Result<Imported, InteropError> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    if v.get("info").is_some() && v.get("item").is_some() {
        let mut ctx = Ctx::default();
        let root_auth = v.get("auth").map(|a| ctx.auth(a));
        let name = str_of(&v["info"], "name").unwrap_or_else(|| "Imported collection".into());
        let root = Inherited {
            auth: root_auth.unwrap_or(Auth::None),
            pre: event_script(&v, "prerequest"),
            test: event_script(&v, "test"),
        };
        ctx.note_inherited(&root);
        let items = ctx.items(v.get("item"), &root);
        if let Some(vars) = v.get("variable").and_then(Value::as_array) {
            if !vars.is_empty() {
                ctx.warn(format!(
                    "{} collection variable(s) were not imported; add them to an environment",
                    vars.len()
                ));
            }
        }
        return Ok(Imported::Collection(
            Collection { name, items },
            ctx.finish(),
        ));
    }
    if v.get("requests").is_some() && v.get("order").is_some() {
        return Err(InteropError::PostmanV1);
    }
    if let Some(values) = v.get("values").and_then(Value::as_array) {
        let vars: Vec<EnvVar> = values.iter().filter_map(env_var).collect();
        let mut warnings = Vec::new();
        let skipped = values.len() - vars.len();
        if skipped > 0 {
            warnings.push(format!("{skipped} variable(s) without a name were skipped"));
        }
        let scope = str_of(&v, "_postman_variable_scope").unwrap_or_default();
        if scope == "globals" {
            return Ok(Imported::Globals(vars, warnings));
        }
        let name = str_of(&v, "name").unwrap_or_else(|| "Imported environment".into());
        return Ok(Imported::Environment(
            Environment {
                name,
                variables: vars,
            },
            warnings,
        ));
    }
    Err(InteropError::Unknown)
}

fn env_var(v: &Value) -> Option<EnvVar> {
    let key = str_of(v, "key")?;
    if key.trim().is_empty() {
        return None;
    }
    Some(EnvVar {
        key,
        value: scalar(v.get("value")),
        secret: str_of(v, "type").as_deref() == Some("secret"),
        enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// What a request gets from the collection and folders above it. We have no collection- or
/// folder-level auth or scripts yet, so they are copied into each request (Postman runs outer
/// scripts first, so they are prepended).
#[derive(Clone, Default)]
struct Inherited {
    auth: Auth,
    pre: String,
    test: String,
}

/// The script of one event type (`prerequest` or `test`) on a collection, folder or request.
fn event_script(v: &Value, listen: &str) -> String {
    let Some(events) = v.get("event").and_then(Value::as_array) else {
        return String::new();
    };
    events
        .iter()
        .filter(|e| e.get("listen").and_then(Value::as_str) == Some(listen))
        .filter_map(|e| match e.pointer("/script/exec") {
            Some(Value::Array(lines)) => Some(
                lines
                    .iter()
                    .map(|l| scalar(Some(l)))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        })
        // Postman writes empty stubs (`"exec": [""]`); they're not scripts
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn join_scripts(outer: &str, inner: &str) -> String {
    match (outer.trim().is_empty(), inner.trim().is_empty()) {
        (true, _) => inner.to_string(),
        (_, true) => outer.to_string(),
        _ => format!("{outer}\n\n{inner}"),
    }
}

#[derive(Default)]
struct Ctx {
    warnings: Vec<String>,
    /// Collections/folders whose scripts were copied down into their requests.
    copied_scripts: usize,
}

impl Ctx {
    fn warn(&mut self, w: String) {
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    fn note_inherited(&mut self, level: &Inherited) {
        if !level.pre.trim().is_empty() || !level.test.trim().is_empty() {
            self.copied_scripts += 1;
        }
    }

    fn finish(mut self) -> Vec<String> {
        if self.copied_scripts > 0 {
            self.warnings.insert(
                0,
                format!(
                    "scripts on {} collection/folder level(s) were copied into each request below them",
                    self.copied_scripts
                ),
            );
        }
        self.warnings
    }

    fn items(&mut self, items: Option<&Value>, inherited: &Inherited) -> Vec<Item> {
        let Some(items) = items.and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for it in items {
            let name = str_of(it, "name").unwrap_or_default();
            if it.get("item").is_some() {
                let own = Inherited {
                    auth: match it.get("auth") {
                        Some(a) => self.auth(a),
                        None => inherited.auth.clone(),
                    },
                    pre: event_script(it, "prerequest"),
                    test: event_script(it, "test"),
                };
                self.note_inherited(&own);
                let level = Inherited {
                    auth: own.auth,
                    pre: join_scripts(&inherited.pre, &own.pre),
                    test: join_scripts(&inherited.test, &own.test),
                };
                out.push(Item::Folder {
                    name: if name.trim().is_empty() {
                        "Untitled folder".into()
                    } else {
                        name
                    },
                    items: self.items(it.get("item"), &level),
                });
            } else if let Some(r) = it.get("request") {
                let mut req = self.request(r, &inherited.auth);
                req.pre_request_script =
                    join_scripts(&inherited.pre, &event_script(it, "prerequest"));
                req.test_script = join_scripts(&inherited.test, &event_script(it, "test"));
                req.name = if name.trim().is_empty() {
                    "Untitled request".into()
                } else {
                    name
                };
                if let Some(ppb) = it.get("protocolProfileBehavior") {
                    if let Some(b) = ppb.get("followRedirects").and_then(Value::as_bool) {
                        req.settings.follow_redirects = b;
                    }
                    if let Some(b) = ppb.get("strictSSL").and_then(Value::as_bool) {
                        req.settings.verify_tls = b;
                    }
                }
                out.push(Item::Request(req));
            }
        }
        out
    }

    fn request(&mut self, r: &Value, inherited: &Auth) -> Request {
        // v2.1 allows a bare URL string as the whole request
        if let Some(url) = r.as_str() {
            return Request {
                url: url.to_string(),
                auth: inherited.clone(),
                ..Default::default()
            };
        }
        let method = method_of(&str_of(r, "method").unwrap_or_default());
        if method.is_none() {
            if let Some(m) = str_of(r, "method") {
                self.warn(format!("unsupported method {m} was changed to GET"));
            }
        }
        let (url, params) = self.url(r.get("url"));
        let headers = kv_list(r.get("header"));
        let body = self.body(r.get("body"));
        let auth = match r.get("auth") {
            Some(a) => self.auth(a),
            None => inherited.clone(),
        };
        Request {
            method: method.unwrap_or_default(),
            url,
            params,
            headers,
            body,
            auth,
            settings: RequestSettings::default(),
            ..Default::default()
        }
    }

    /// URL text without the query string, plus the query as params (so it isn't sent twice).
    fn url(&mut self, u: Option<&Value>) -> (String, Vec<KeyValue>) {
        match u {
            Some(Value::String(s)) => (s.clone(), Vec::new()),
            Some(o @ Value::Object(_)) => {
                if o.get("variable")
                    .and_then(Value::as_array)
                    .is_some_and(|v| !v.is_empty())
                {
                    self.warn(
                        "path variables (:name) were kept as text; replace them with {{variables}}"
                            .into(),
                    );
                }
                let raw = str_of(o, "raw").unwrap_or_else(|| url_from_parts(o));
                let query = kv_list(o.get("query"));
                if query.is_empty() {
                    (raw, Vec::new())
                } else {
                    let base = raw.split('?').next().unwrap_or("").to_string();
                    (base, query)
                }
            }
            _ => (String::new(), Vec::new()),
        }
    }

    fn body(&mut self, b: Option<&Value>) -> Body {
        let Some(b) = b else {
            return Body::None;
        };
        if b.get("disabled").and_then(Value::as_bool) == Some(true) {
            return Body::None;
        }
        match str_of(b, "mode").as_deref() {
            Some("raw") => {
                let raw = str_of(b, "raw").unwrap_or_default();
                if raw.is_empty() {
                    return Body::None;
                }
                match b
                    .pointer("/options/raw/language")
                    .and_then(Value::as_str)
                    .unwrap_or("text")
                {
                    "json" => Body::Json { raw },
                    "xml" => Body::Xml { raw },
                    "html" => Body::Html { raw },
                    _ => Body::Text { raw },
                }
            }
            Some("urlencoded") => Body::UrlEncoded {
                fields: kv_list(b.get("urlencoded")),
            },
            Some("formdata") => {
                let fields = b
                    .get("formdata")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|f| {
                                let key = str_of(f, "key")?;
                                let enabled =
                                    f.get("disabled").and_then(Value::as_bool) != Some(true);
                                let value = if str_of(f, "type").as_deref() == Some("file") {
                                    FormValue::File {
                                        path: match f.get("src") {
                                            Some(Value::Array(a)) => a
                                                .first()
                                                .map(|s| scalar(Some(s)))
                                                .unwrap_or_default(),
                                            other => scalar(other),
                                        },
                                    }
                                } else {
                                    FormValue::Text {
                                        value: scalar(f.get("value")),
                                    }
                                };
                                Some(FormField {
                                    key,
                                    value,
                                    enabled,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Body::FormData { fields }
            }
            Some("graphql") => {
                self.warn(
                    "GraphQL bodies were converted to plain JSON ({\"query\", \"variables\"})"
                        .into(),
                );
                let query = b.pointer("/graphql/query").cloned().unwrap_or(Value::Null);
                let vars = match b.pointer("/graphql/variables") {
                    Some(Value::String(s)) if !s.trim().is_empty() => {
                        serde_json::from_str(s).unwrap_or(Value::String(s.clone()))
                    }
                    Some(Value::String(_)) | None => Value::Null,
                    Some(v) => v.clone(),
                };
                let raw = serde_json::to_string_pretty(&json!({"query": query, "variables": vars}))
                    .unwrap_or_default();
                Body::Json { raw }
            }
            Some("file") => {
                self.warn("binary file bodies were not imported".into());
                Body::None
            }
            _ => Body::None,
        }
    }

    fn auth(&mut self, a: &Value) -> Auth {
        let ty = str_of(a, "type").unwrap_or_default();
        let p = |key: &str| auth_param(a, &ty, key);
        match ty.as_str() {
            "noauth" | "" => Auth::None,
            "basic" => Auth::Basic {
                username: p("username"),
                password: p("password"),
            },
            "bearer" => Auth::Bearer { token: p("token") },
            "apikey" => Auth::ApiKey {
                key: p("key"),
                value: p("value"),
                location: if p("in") == "query" {
                    ApiKeyLocation::Query
                } else {
                    ApiKeyLocation::Header
                },
            },
            other => {
                self.warn(format!(
                    "\"{other}\" authorization is not supported yet; those requests were imported without auth"
                ));
                Auth::None
            }
        }
    }
}

/// Auth parameter in either v2.1 form (`[{key, value}]`) or v2.0 form (`{key: value}`).
fn auth_param(a: &Value, ty: &str, key: &str) -> String {
    match a.get(ty) {
        Some(Value::Array(list)) => list
            .iter()
            .find(|e| e.get("key").and_then(Value::as_str) == Some(key))
            .map(|e| scalar(e.get("value")))
            .unwrap_or_default(),
        Some(o @ Value::Object(_)) => scalar(o.get(key)),
        _ => String::new(),
    }
}

fn url_from_parts(o: &Value) -> String {
    let join = |v: Option<&Value>, sep: &str| match v {
        Some(Value::Array(a)) => a
            .iter()
            .map(|s| scalar(Some(s)))
            .collect::<Vec<_>>()
            .join(sep),
        other => scalar(other),
    };
    let host = join(o.get("host"), ".");
    let path = join(o.get("path"), "/");
    let mut s = String::new();
    if let Some(p) = str_of(o, "protocol") {
        s.push_str(&p);
        s.push_str("://");
    }
    s.push_str(&host);
    if let Some(port) = str_of(o, "port") {
        s.push(':');
        s.push_str(&port);
    }
    if !path.is_empty() {
        s.push('/');
        s.push_str(&path);
    }
    s
}

fn kv_list(v: Option<&Value>) -> Vec<KeyValue> {
    let Some(list) = v.and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|e| {
            let key = str_of(e, "key")?;
            Some(KeyValue {
                key,
                value: scalar(e.get("value")),
                description: description(e.get("description")),
                enabled: e.get("disabled").and_then(Value::as_bool) != Some(true),
            })
        })
        .collect()
}

fn description(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(o @ Value::Object(_)) => scalar(o.get("content")),
        _ => String::new(),
    }
}

/// A JSON value as text: strings as-is, numbers/bools printed, null/absent empty.
fn scalar(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn method_of(s: &str) -> Option<Method> {
    Some(match s.to_ascii_uppercase().as_str() {
        "" | "GET" => Method::Get,
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

// ---------------------------------------------------------------- export

/// A Postman v2.1 collection document, plus notes about anything that couldn't be expressed.
pub fn export_collection(col: &Collection) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let doc = json!({
        "info": {
            "name": col.name,
            "schema": POSTMAN_SCHEMA_V21,
        },
        "item": export_items(&col.items, &mut warnings),
    });
    (
        serde_json::to_string_pretty(&doc).unwrap_or_default(),
        warnings,
    )
}

fn export_items(items: &[Item], warnings: &mut Vec<String>) -> Vec<Value> {
    items
        .iter()
        .map(|it| match it {
            Item::Folder { name, items } => json!({
                "name": name,
                "item": export_items(items, warnings),
            }),
            Item::Request(r) => export_request(r, warnings),
        })
        .collect()
}

fn export_request(r: &Request, warnings: &mut Vec<String>) -> Value {
    let mut headers: Vec<Value> = r.headers.iter().map(kv_json).collect();
    let mut request = Map::new();
    request.insert("method".into(), json!(r.method.as_str()));

    match &r.auth {
        Auth::None => {}
        Auth::Basic { username, password } => {
            request.insert(
                "auth".into(),
                json!({"type": "basic", "basic": [
                    {"key": "username", "value": username, "type": "string"},
                    {"key": "password", "value": password, "type": "string"},
                ]}),
            );
        }
        Auth::Bearer { token } => {
            request.insert(
                "auth".into(),
                json!({"type": "bearer", "bearer": [{"key": "token", "value": token, "type": "string"}]}),
            );
        }
        Auth::Jwt { token, prefix } => {
            // Postman's JWT type generates tokens; ours sends a ready token, i.e. a header.
            headers
                .push(json!({"key": "Authorization", "value": format!("{prefix} {token}").trim()}));
        }
        Auth::ApiKey {
            key,
            value,
            location,
        } => {
            let place = match location {
                ApiKeyLocation::Header => "header",
                ApiKeyLocation::Query => "query",
            };
            request.insert(
                "auth".into(),
                json!({"type": "apikey", "apikey": [
                    {"key": "key", "value": key, "type": "string"},
                    {"key": "value", "value": value, "type": "string"},
                    {"key": "in", "value": place, "type": "string"},
                ]}),
            );
        }
        Auth::Custom { headers: extra } => headers.extend(extra.iter().map(kv_json)),
    }
    request.insert("header".into(), Value::Array(headers));

    let enabled: Vec<String> = r
        .params
        .iter()
        .filter(|p| p.enabled)
        .map(|p| {
            if p.value.is_empty() {
                p.key.clone()
            } else {
                format!("{}={}", p.key, p.value)
            }
        })
        .collect();
    let raw = if enabled.is_empty() {
        r.url.clone()
    } else {
        let sep = if r.url.contains('?') { '&' } else { '?' };
        format!("{}{sep}{}", r.url, enabled.join("&"))
    };
    let mut url = Map::new();
    url.insert("raw".into(), json!(raw));
    if !r.params.is_empty() {
        url.insert(
            "query".into(),
            Value::Array(r.params.iter().map(kv_json).collect()),
        );
    }
    request.insert("url".into(), Value::Object(url));

    let raw_body = |raw: &str, lang: &str| json!({"mode": "raw", "raw": raw, "options": {"raw": {"language": lang}}});
    let body = match &r.body {
        Body::None => None,
        Body::Json { raw } => Some(raw_body(raw, "json")),
        Body::Xml { raw } => Some(raw_body(raw, "xml")),
        Body::Html { raw } => Some(raw_body(raw, "html")),
        Body::Text { raw } => Some(raw_body(raw, "text")),
        Body::UrlEncoded { fields } => Some(json!({
            "mode": "urlencoded",
            "urlencoded": fields.iter().map(kv_json).collect::<Vec<_>>(),
        })),
        Body::FormData { fields } => Some(json!({
            "mode": "formdata",
            "formdata": fields.iter().map(|f| {
                let mut o = match &f.value {
                    FormValue::Text { value } => json!({"key": f.key, "value": value, "type": "text"}),
                    FormValue::File { path } => json!({"key": f.key, "src": path, "type": "file"}),
                };
                if !f.enabled {
                    o["disabled"] = json!(true);
                }
                o
            }).collect::<Vec<_>>(),
        })),
    };
    if let Some(b) = body {
        request.insert("body".into(), b);
    }

    let mut item = Map::new();
    item.insert("name".into(), json!(r.name));
    item.insert("request".into(), Value::Object(request));
    let events: Vec<Value> = [
        ("prerequest", &r.pre_request_script),
        ("test", &r.test_script),
    ]
    .into_iter()
    .filter(|(_, src)| !src.trim().is_empty())
    .map(|(listen, src)| {
        json!({
            "listen": listen,
            "script": {"type": "text/javascript", "exec": src.lines().collect::<Vec<_>>()},
        })
    })
    .collect();
    if !events.is_empty() {
        item.insert("event".into(), Value::Array(events));
    }
    let defaults = RequestSettings::default();
    if r.settings.follow_redirects != defaults.follow_redirects
        || r.settings.verify_tls != defaults.verify_tls
    {
        item.insert(
            "protocolProfileBehavior".into(),
            json!({"followRedirects": r.settings.follow_redirects, "strictSSL": r.settings.verify_tls}),
        );
    }
    if r.settings.timeout_ms != defaults.timeout_ms {
        let w =
            "per-request timeouts are not part of the Postman format and were left out".to_string();
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }
    Value::Object(item)
}

fn kv_json(kv: &KeyValue) -> Value {
    let mut o = json!({"key": kv.key, "value": kv.value});
    if !kv.description.is_empty() {
        o["description"] = json!(kv.description);
    }
    if !kv.enabled {
        o["disabled"] = json!(true);
    }
    o
}

/// A Postman environment (or globals, when `globals` is set). Secret values are left empty
/// unless `include_secrets` (FR-ENV-04).
pub fn export_environment(
    name: &str,
    vars: &[EnvVar],
    include_secrets: bool,
    globals: bool,
) -> String {
    let values: Vec<Value> = vars
        .iter()
        .map(|v| {
            json!({
                "key": v.key,
                "value": if v.secret && !include_secrets { "" } else { v.value.as_str() },
                "type": if v.secret { "secret" } else { "default" },
                "enabled": v.enabled,
            })
        })
        .collect();
    let doc = json!({
        "name": name,
        "values": values,
        "_postman_variable_scope": if globals { "globals" } else { "environment" },
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

// ---------------------------------------------------------------- store glue

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriteSummary {
    pub requests: usize,
    pub folders: usize,
}

/// Write an imported collection as a new collection on disk (never merges into an existing one;
/// a name clash gets a new directory). Returns the collection directory.
pub fn write_collection(
    store: &Store,
    col: &Collection,
) -> Result<(std::path::PathBuf, WriteSummary), InteropError> {
    let tree = store.create_collection(&col.name)?;
    let mut sum = WriteSummary::default();
    write_items(&tree.dir, &col.items, &mut sum)?;
    Ok((tree.dir, sum))
}

fn write_items(dir: &Path, items: &[Item], sum: &mut WriteSummary) -> Result<(), InteropError> {
    for it in items {
        match it {
            Item::Folder { name, items } => {
                let sub = Store::create_folder(dir, name)?;
                sum.folders += 1;
                write_items(&sub, items, sum)?;
            }
            Item::Request(r) => {
                Store::save_request(dir, r, None)?;
                sum.requests += 1;
            }
        }
    }
    Ok(())
}

/// Read a collection directory back into a tree. Unreadable files are skipped and reported.
pub fn read_collection(name: &str, dir: &Path) -> Result<(Collection, Vec<String>), InteropError> {
    let nodes = Store::load_tree(dir)?;
    let mut warnings = Vec::new();
    let items = nodes_to_items(&nodes, &mut warnings);
    Ok((
        Collection {
            name: name.to_string(),
            items,
        },
        warnings,
    ))
}

fn nodes_to_items(nodes: &[Node], warnings: &mut Vec<String>) -> Vec<Item> {
    let mut out = Vec::new();
    for n in nodes {
        match n {
            Node::Folder { name, children, .. } => out.push(Item::Folder {
                name: name.clone(),
                items: nodes_to_items(children, warnings),
            }),
            Node::Request { path, .. } => match Store::load_request(path) {
                Ok(r) => out.push(Item::Request(r)),
                Err(e) => warnings.push(format!("skipped: {e}")),
            },
            Node::Broken { name, .. } => {
                warnings.push(format!("skipped unreadable request \"{name}\""))
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSTMAN: &str = r#"{
      "info": {"name": "Pet Store", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
      "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}", "type": "string"}]},
      "variable": [{"key": "base", "value": "https://petstore.test"}],
      "event": [{"listen": "prerequest", "script": {"exec": ["console.log(1)"]}}],
      "item": [
        {"name": "Pets", "item": [
          {"name": "List pets",
           "event": [{"listen": "test", "script": {"exec": [""]}}],
           "request": {"method": "GET",
             "header": [{"key": "Accept", "value": "application/json"},
                        {"key": "X-Off", "value": "1", "disabled": true}],
             "url": {"raw": "{{base}}/pets?limit=10&tag=dog",
                     "host": ["{{base}}"], "path": ["pets"],
                     "query": [{"key": "limit", "value": "10"},
                               {"key": "tag", "value": "dog", "disabled": true}]}}},
          {"name": "Create pet",
           "request": {"method": "POST",
             "auth": {"type": "noauth"},
             "header": [],
             "body": {"mode": "raw", "raw": "{\"name\":\"rex\"}", "options": {"raw": {"language": "json"}}},
             "url": "{{base}}/pets"}}
        ]},
        {"name": "Login", "request": {"method": "post",
           "auth": {"type": "basic", "basic": {"username": "u", "password": "p"}},
           "body": {"mode": "urlencoded", "urlencoded": [{"key": "a", "value": "1"}]},
           "url": "{{base}}/login"}},
        {"name": "Upload", "request": {"method": "PUT",
           "body": {"mode": "formdata", "formdata": [
              {"key": "f", "type": "file", "src": "/tmp/x.png"},
              {"key": "note", "type": "text", "value": "hi", "disabled": true}]},
           "url": {"raw": "https://up.test/u/:id", "variable": [{"key": "id", "value": "7"}]}}},
        {"name": "OAuth thing", "request": {"method": "GET",
           "auth": {"type": "oauth2", "oauth2": []},
           "url": "https://o.test"}},
        {"name": "Bare", "request": "https://bare.test/x"}
      ]
    }"#;

    fn collection() -> (Collection, Vec<String>) {
        match import_postman(POSTMAN).unwrap() {
            Imported::Collection(c, w) => (c, w),
            other => panic!("expected a collection, got {other:?}"),
        }
    }

    fn find<'a>(items: &'a [Item], name: &str) -> &'a Request {
        for it in items {
            match it {
                Item::Request(r) if r.name == name => return r,
                Item::Folder { items, .. } => {
                    if let Some(r) = try_find(items, name) {
                        return r;
                    }
                }
                _ => {}
            }
        }
        panic!("{name} not found")
    }

    fn try_find<'a>(items: &'a [Item], name: &str) -> Option<&'a Request> {
        items.iter().find_map(|it| match it {
            Item::Request(r) if r.name == name => Some(r),
            Item::Folder { items, .. } => try_find(items, name),
            _ => None,
        })
    }

    #[test]
    fn imports_structure_and_names() {
        let (c, _) = collection();
        assert_eq!(c.name, "Pet Store");
        assert_eq!(c.items.len(), 5);
        match &c.items[0] {
            Item::Folder { name, items } => {
                assert_eq!(name, "Pets");
                assert_eq!(items.len(), 2);
            }
            other => panic!("expected folder, got {other:?}"),
        }
    }

    #[test]
    fn query_moves_to_params_without_duplicating() {
        let (c, _) = collection();
        let r = find(&c.items, "List pets");
        assert_eq!(r.url, "{{base}}/pets");
        assert_eq!(r.params.len(), 2);
        assert_eq!(r.params[0], KeyValue::new("limit", "10"));
        assert!(!r.params[1].enabled);
        assert!(r.headers[0].enabled && !r.headers[1].enabled);
    }

    #[test]
    fn auth_inherits_and_can_be_overridden() {
        let (c, _) = collection();
        assert_eq!(
            find(&c.items, "List pets").auth,
            Auth::Bearer {
                token: "{{token}}".into()
            }
        );
        assert_eq!(find(&c.items, "Create pet").auth, Auth::None, "noauth wins");
        assert_eq!(
            find(&c.items, "Login").auth,
            Auth::Basic {
                username: "u".into(),
                password: "p".into()
            },
            "v2.0 object form"
        );
        assert_eq!(find(&c.items, "OAuth thing").auth, Auth::None);
        assert_eq!(
            find(&c.items, "Bare").auth,
            Auth::Bearer {
                token: "{{token}}".into()
            }
        );
    }

    #[test]
    fn bodies_and_methods() {
        let (c, _) = collection();
        let create = find(&c.items, "Create pet");
        assert_eq!(create.method, Method::Post);
        assert_eq!(
            create.body,
            Body::Json {
                raw: "{\"name\":\"rex\"}".into()
            }
        );
        let login = find(&c.items, "Login");
        assert_eq!(login.method, Method::Post, "lowercase method accepted");
        assert_eq!(
            login.body,
            Body::UrlEncoded {
                fields: vec![KeyValue::new("a", "1")]
            }
        );
        match &find(&c.items, "Upload").body {
            Body::FormData { fields } => {
                assert_eq!(
                    fields[0].value,
                    FormValue::File {
                        path: "/tmp/x.png".into()
                    }
                );
                assert!(!fields[1].enabled);
            }
            other => panic!("expected form data, got {other:?}"),
        }
        assert_eq!(find(&c.items, "Bare").url, "https://bare.test/x");
    }

    #[test]
    fn warnings_explain_what_was_skipped() {
        let (_, w) = collection();
        let all = w.join("\n");
        assert!(
            all.contains("scripts on 1 collection/folder level"),
            "{all}"
        );
        assert!(all.contains("collection variable"), "{all}");
        assert!(all.contains("oauth2"), "{all}");
        assert!(all.contains("path variables"), "{all}");
    }

    #[test]
    fn scripts_are_imported_with_outer_levels_first() {
        let doc = r#"{"info": {"name": "s"},
          "event": [{"listen": "prerequest", "script": {"exec": ["// collection"]}}],
          "item": [{"name": "F",
            "event": [{"listen": "test", "script": {"exec": ["// folder test"]}}],
            "item": [{"name": "R",
              "event": [{"listen": "prerequest", "script": {"exec": ["// request", "pm.variables.set('a', 1);"]}},
                        {"listen": "test", "script": {"exec": ""}}],
              "request": {"method": "GET", "url": "https://s.test"}}]}]}"#;
        let Imported::Collection(c, w) = import_postman(doc).unwrap() else {
            panic!()
        };
        let r = find(&c.items, "R");
        assert_eq!(
            r.pre_request_script,
            "// collection\n\n// request\npm.variables.set('a', 1);"
        );
        assert_eq!(r.test_script, "// folder test", "empty stub ignored");
        assert!(w[0].contains("2 collection/folder level"), "{w:?}");

        // and they export as request-level events, importing back identically
        let (text, _) = export_collection(&c);
        let Imported::Collection(back, _) = import_postman(&text).unwrap() else {
            panic!()
        };
        assert_eq!(find(&back.items, "R"), r);
    }

    #[test]
    fn roundtrip_through_export() {
        let (c, _) = collection();
        let (text, warnings) = export_collection(&c);
        assert!(warnings.is_empty(), "{warnings:?}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["info"]["schema"], POSTMAN_SCHEMA_V21);
        // the exported raw URL carries the enabled params only
        assert_eq!(
            v["item"][0]["item"][0]["request"]["url"]["raw"],
            "{{base}}/pets?limit=10"
        );
        let back = match import_postman(&text).unwrap() {
            Imported::Collection(c, _) => c,
            other => panic!("{other:?}"),
        };
        // Auth is resolved on import (inheritance flattened), so the trees compare equal.
        assert_eq!(back, c);
    }

    #[test]
    fn jwt_and_custom_auth_export_as_headers() {
        let col = Collection {
            name: "x".into(),
            items: vec![Item::Request(Request {
                name: "j".into(),
                url: "https://j.test".into(),
                auth: Auth::Jwt {
                    token: "abc".into(),
                    prefix: "Bearer".into(),
                },
                ..Default::default()
            })],
        };
        let (text, _) = export_collection(&col);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["item"][0]["request"]["header"][0]["value"], "Bearer abc");
    }

    #[test]
    fn environment_and_globals() {
        let env = r#"{"name": "Staging", "_postman_variable_scope": "environment",
          "values": [{"key": "host", "value": "s.test", "enabled": true},
                     {"key": "token", "value": "t0p", "type": "secret", "enabled": false},
                     {"key": "", "value": "x"},
                     {"key": "port", "value": 8080}]}"#;
        match import_postman(env).unwrap() {
            Imported::Environment(e, w) => {
                assert_eq!(e.name, "Staging");
                assert_eq!(e.variables.len(), 3);
                assert!(e.variables[1].secret && !e.variables[1].enabled);
                assert_eq!(e.variables[2].value, "8080", "numbers become text");
                assert_eq!(w.len(), 1, "the nameless variable is reported");
            }
            other => panic!("{other:?}"),
        }
        let globals = r#"{"name": "Globals", "_postman_variable_scope": "globals",
          "values": [{"key": "a", "value": "1"}]}"#;
        assert!(
            matches!(import_postman(globals).unwrap(), Imported::Globals(v, _) if v.len() == 1)
        );
    }

    #[test]
    fn secrets_are_blank_unless_included() {
        let vars = vec![
            EnvVar {
                key: "token".into(),
                value: "t0p".into(),
                secret: true,
                enabled: true,
            },
            EnvVar::new("host", "h.test"),
        ];
        let out = export_environment("Prod", &vars, false, false);
        assert!(!out.contains("t0p"));
        assert!(out.contains("h.test"));
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["values"][0]["type"], "secret");
        assert_eq!(v["_postman_variable_scope"], "environment");
        assert!(export_environment("Prod", &vars, true, false).contains("t0p"));
        // and it imports back
        match import_postman(&out).unwrap() {
            Imported::Environment(e, _) => assert_eq!(e.variables.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_other_files_clearly() {
        assert!(matches!(
            import_postman("{nope"),
            Err(InteropError::Json(_))
        ));
        assert!(matches!(
            import_postman("{\"a\": 1}"),
            Err(InteropError::Unknown)
        ));
        assert!(matches!(import_postman("[]"), Err(InteropError::Unknown)));
        assert!(matches!(
            import_postman(r#"{"requests": [], "order": []}"#),
            Err(InteropError::PostmanV1)
        ));
        // a UTF-8 BOM (common on Windows) is fine
        let bom = format!("\u{feff}{}", r#"{"info": {"name": "b"}, "item": []}"#);
        assert!(matches!(
            import_postman(&bom).unwrap(),
            Imported::Collection(..)
        ));
    }

    #[test]
    fn graphql_becomes_json() {
        let g = r#"{"info": {"name": "g"}, "item": [{"name": "q", "request": {"method": "POST",
          "body": {"mode": "graphql", "graphql": {"query": "{ me { id } }", "variables": "{\"a\":1}"}},
          "url": "https://g.test"}}]}"#;
        let Imported::Collection(c, w) = import_postman(g).unwrap() else {
            panic!()
        };
        match &find(&c.items, "q").body {
            Body::Json { raw } => {
                let v: Value = serde_json::from_str(raw).unwrap();
                assert_eq!(v["query"], "{ me { id } }");
                assert_eq!(v["variables"]["a"], 1);
            }
            other => panic!("{other:?}"),
        }
        assert!(w.iter().any(|w| w.contains("GraphQL")));
    }

    #[test]
    fn write_and_read_back_from_disk() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let (c, _) = collection();
        let (dir, sum) = write_collection(&store, &c).unwrap();
        assert_eq!(
            sum,
            WriteSummary {
                requests: 6,
                folders: 1
            }
        );
        assert!(dir.join("Pets").is_dir());
        // importing the same file again makes a second collection, never merges
        let (dir2, _) = write_collection(&store, &c).unwrap();
        assert_ne!(dir, dir2);

        let (back, warnings) = read_collection("Pet Store", &dir).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        // load_tree sorts folders first, then by name, so compare as sets of request names
        let mut names = Vec::new();
        fn walk(items: &[Item], out: &mut Vec<String>) {
            for it in items {
                match it {
                    Item::Folder { items, .. } => walk(items, out),
                    Item::Request(r) => out.push(r.name.clone()),
                }
            }
        }
        walk(&back.items, &mut names);
        names.sort();
        assert_eq!(
            names,
            vec![
                "Bare",
                "Create pet",
                "List pets",
                "Login",
                "OAuth thing",
                "Upload"
            ]
        );
        assert_eq!(find(&back.items, "List pets"), find(&c.items, "List pets"));
    }
}
