//! Read a cURL command (as copied from a browser's dev tools, docs, or our own generator) into a
//! request. Bash quoting is understood: '…', "…", $'…', backslash escapes and line continuations.
//! Flags we can't represent are listed as notes instead of being silently dropped.
use at_core::{Auth, Body, FormField, FormValue, KeyValue, Method, Request};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CurlError {
    #[error("this doesn't look like a cURL command (it should start with `curl`)")]
    NotCurl,
    #[error("unbalanced quotes in the command")]
    Unbalanced,
    #[error("no URL found in the cURL command")]
    NoUrl,
    #[error("`{0}` needs a value")]
    MissingValue(String),
}

/// Split a command line like bash would (without expansions).
fn tokenize(s: &str) -> Result<Vec<String>, CurlError> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                // line continuation
                Some('\n') => {}
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(n) => {
                    cur.push(n);
                    in_token = true;
                }
                None => {}
            },
            '\'' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => cur.push(ch),
                        None => return Err(CurlError::Unbalanced),
                    }
                }
            }
            '"' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => cur.push(e),
                            Some('\n') => {}
                            Some(other) => {
                                cur.push('\\');
                                cur.push(other);
                            }
                            None => return Err(CurlError::Unbalanced),
                        },
                        Some(ch) => cur.push(ch),
                        None => return Err(CurlError::Unbalanced),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => cur.push('\n'),
                            Some('t') => cur.push('\t'),
                            Some('r') => cur.push('\r'),
                            Some('0') => cur.push('\0'),
                            Some(e @ ('\'' | '"' | '\\')) => cur.push(e),
                            Some(other) => {
                                cur.push('\\');
                                cur.push(other);
                            }
                            None => return Err(CurlError::Unbalanced),
                        },
                        Some(ch) => cur.push(ch),
                        None => return Err(CurlError::Unbalanced),
                    }
                }
            }
            c if c.is_whitespace() => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            c => {
                cur.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        out.push(cur);
    }
    Ok(out)
}

/// Long and short flags that take a value, and what they mean to us.
fn takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "-X" | "--request"
            | "-H"
            | "--header"
            | "-d"
            | "--data"
            | "--data-raw"
            | "--data-binary"
            | "--data-ascii"
            | "--data-urlencode"
            | "--json"
            | "-u"
            | "--user"
            | "-F"
            | "--form"
            | "--form-string"
            | "-A"
            | "--user-agent"
            | "-b"
            | "--cookie"
            | "-e"
            | "--referer"
            | "--url"
            | "-m"
            | "--max-time"
            | "--oauth2-bearer"
            // value flags we ignore (listed so their value isn't taken for the URL)
            | "-o"
            | "--output"
            | "-x"
            | "--proxy"
            | "--connect-timeout"
            | "-w"
            | "--write-out"
            | "-T"
            | "--upload-file"
            | "--cacert"
            | "--cert"
            | "-E"
            | "--key"
            | "-c"
            | "--cookie-jar"
            | "--retry"
            | "-r"
            | "--range"
            | "--resolve"
            | "--limit-rate"
    )
}

/// Flags that change nothing for us and need no note.
fn quiet_flag(flag: &str) -> bool {
    matches!(
        flag,
        "-s" | "--silent"
            | "-S"
            | "--show-error"
            | "-v"
            | "--verbose"
            | "-i"
            | "--include"
            | "--compressed"
            | "-g"
            | "--globoff"
            | "--http1.1"
            | "--http2"
            | "-L"
            | "--location"
            | "-f"
            | "--fail"
            | "-#"
            | "--progress-bar"
            | "-o"
            | "--output"
    )
}

fn percent_encode_value(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse a cURL command into a request plus notes about anything that was left out.
pub fn parse_curl(cmd: &str) -> Result<(Request, Vec<String>), CurlError> {
    let tokens = tokenize(cmd.trim())?;
    let mut it = tokens.into_iter();
    match it.next() {
        Some(first) if first.eq_ignore_ascii_case("curl") => {}
        _ => return Err(CurlError::NotCurl),
    }

    let mut notes: Vec<String> = Vec::new();
    let mut note = |n: String| {
        if !notes.contains(&n) {
            notes.push(n);
        }
    };
    let mut method: Option<Method> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<KeyValue> = Vec::new();
    let mut data: Vec<String> = Vec::new();
    let mut json_data = false;
    let mut form: Vec<FormField> = Vec::new();
    let mut auth = Auth::None;
    let mut get = false;
    let mut head = false;
    let mut req = Request::default();

    // Expand `-XPOST`, `-sSL`, `--flag=value` into (flag, inline value) pairs.
    let mut pending: std::collections::VecDeque<String> = it.collect();
    while let Some(tok) = pending.pop_front() {
        let (flag, inline): (String, Option<String>) = if let Some(rest) = tok.strip_prefix("--") {
            if rest.is_empty() {
                // `--` ends options; everything after is a URL
                if let Some(u) = pending.pop_front() {
                    url.get_or_insert(u);
                }
                continue;
            }
            match rest.split_once('=') {
                Some((f, v)) if takes_value(&format!("--{f}")) => {
                    (format!("--{f}"), Some(v.to_string()))
                }
                _ => (tok.clone(), None),
            }
        } else if tok.starts_with('-') && tok.len() > 1 {
            let mut chars = tok[1..].chars();
            let first = chars.next().map(|c| format!("-{c}")).unwrap_or_default();
            let rest: String = chars.collect();
            if rest.is_empty() {
                (first, None)
            } else if takes_value(&first) {
                (first, Some(rest))
            } else {
                // combined booleans like -sSL: handle the first, queue the rest
                pending.push_front(format!("-{rest}"));
                (first, None)
            }
        } else {
            match url {
                None => url = Some(tok),
                Some(_) => note(format!("ignored extra argument `{tok}`")),
            }
            continue;
        };

        let value = if takes_value(&flag) {
            match inline.or_else(|| pending.pop_front()) {
                Some(v) => Some(v),
                None => return Err(CurlError::MissingValue(flag)),
            }
        } else {
            None
        };
        let v = value.unwrap_or_default();

        match flag.as_str() {
            "-X" | "--request" => match method_from(&v) {
                Some(m) => method = Some(m),
                None => note(format!("unsupported method {v}; used GET")),
            },
            "-H" | "--header" => match v.split_once(':') {
                Some((k, val)) => headers.push(KeyValue::new(k.trim(), val.trim_start())),
                None => note(format!("ignored malformed header `{v}`")),
            },
            "-d" | "--data" | "--data-ascii" | "--data-binary" | "--data-raw" => {
                if v.starts_with('@') && flag != "--data-raw" {
                    note(format!("data from file `{}` was not loaded", &v[1..]));
                } else if flag == "-d" || flag == "--data" || flag == "--data-ascii" {
                    // curl strips newlines from these
                    data.push(v.replace(['\r', '\n'], ""));
                } else {
                    data.push(v);
                }
            }
            "--data-urlencode" => {
                let encoded = match v.split_once('=') {
                    Some((k, val)) if !k.is_empty() => {
                        format!("{k}={}", percent_encode_value(val))
                    }
                    Some((_, val)) => percent_encode_value(val),
                    None => percent_encode_value(&v),
                };
                data.push(encoded);
            }
            "--json" => {
                data.push(v);
                json_data = true;
            }
            "-F" | "--form" | "--form-string" => match v.split_once('=') {
                Some((k, val)) => {
                    let value = match val.strip_prefix('@') {
                        Some(path) if flag != "--form-string" => {
                            let (path, extra) = path.split_once(';').unwrap_or((path, ""));
                            if !extra.is_empty() {
                                note(format!("form options `;{extra}` were dropped"));
                            }
                            FormValue::File {
                                path: path.to_string(),
                            }
                        }
                        _ => {
                            if val.starts_with('<') && flag != "--form-string" {
                                note(format!("form field `{k}` reads a file (`<`); kept as text"));
                            }
                            FormValue::Text {
                                value: val.to_string(),
                            }
                        }
                    };
                    form.push(FormField {
                        key: k.to_string(),
                        value,
                        enabled: true,
                    });
                }
                None => note(format!("ignored malformed form field `{v}`")),
            },
            "-u" | "--user" => {
                let (u, p) = v.split_once(':').unwrap_or((v.as_str(), ""));
                if !v.contains(':') {
                    note("-u without a password: the password was left empty".into());
                }
                auth = Auth::Basic {
                    username: u.to_string(),
                    password: p.to_string(),
                };
            }
            "--oauth2-bearer" => auth = Auth::Bearer { token: v },
            "-A" | "--user-agent" => headers.push(KeyValue::new("User-Agent", &v)),
            "-b" | "--cookie" => {
                if v.contains('=') {
                    headers.push(KeyValue::new("Cookie", &v));
                } else {
                    note(format!("cookie file `{v}` was not loaded"));
                }
            }
            "-e" | "--referer" => headers.push(KeyValue::new("Referer", &v)),
            "--url" => {
                if url.is_none() {
                    url = Some(v);
                }
            }
            "-m" | "--max-time" => match v.parse::<f64>() {
                Ok(secs) if secs >= 0.0 => req.settings.timeout_ms = (secs * 1000.0) as u64,
                _ => note(format!("ignored invalid --max-time `{v}`")),
            },
            "-k" | "--insecure" => req.settings.verify_tls = false,
            "-G" | "--get" => get = true,
            "-I" | "--head" => head = true,
            f if quiet_flag(f) => {}
            f if takes_value(f) => note(format!("ignored `{f} {v}`")),
            f => note(format!("ignored `{f}`")),
        }
    }
    let notes_out = notes;

    let mut url = url.ok_or(CurlError::NoUrl)?;
    let has_ct = headers
        .iter()
        .any(|h| h.key.eq_ignore_ascii_case("content-type"));

    if get && !data.is_empty() {
        // -G: data becomes the query string
        for pair in data.join("&").split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            req.params
                .push(KeyValue::new(&percent_decode(k), &percent_decode(v)));
        }
        data.clear();
    }

    if json_data {
        if !has_ct {
            headers.push(KeyValue::new("Content-Type", "application/json"));
        }
        if !headers.iter().any(|h| h.key.eq_ignore_ascii_case("accept")) {
            headers.push(KeyValue::new("Accept", "application/json"));
        }
    }

    let ct = headers
        .iter()
        .find(|h| h.key.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_ascii_lowercase());
    if !data.is_empty() {
        let raw = data.join("&");
        if ct.is_none() {
            // curl's default for -d
            headers.push(KeyValue::new(
                "Content-Type",
                "application/x-www-form-urlencoded",
            ));
        }
        req.body = match ct.as_deref() {
            Some(c) if c.contains("json") => Body::Json { raw },
            Some(c) if c.contains("xml") => Body::Xml { raw },
            Some(c) if c.contains("html") => Body::Html { raw },
            _ => Body::Text { raw },
        };
    } else if !form.is_empty() {
        req.body = Body::FormData { fields: form };
    }

    req.method = if head {
        Method::Head
    } else if let Some(m) = method {
        m
    } else if !matches!(req.body, Body::None) {
        Method::Post
    } else {
        Method::Get
    };

    if !url.contains("://") {
        url = format!("http://{url}"); // curl's own default scheme
    }
    req.name = request_name(req.method, &url);
    req.url = url;
    req.headers = headers;
    req.auth = auth;
    Ok((req, notes_out))
}

fn method_from(s: &str) -> Option<Method> {
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

/// "GET api.test/users" — a readable default name.
fn request_name(m: Method, url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let short: String = rest.trim_end_matches('/').chars().take(60).collect();
    format!("{} {short}", m.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_handles_bash_quoting() {
        let t = tokenize("curl 'a b' \"c \\\"d\\\"\" $'e\\nf' g\\ h \\\n  i").unwrap();
        assert_eq!(t, vec!["curl", "a b", "c \"d\"", "e\nf", "g h", "i"]);
        assert_eq!(tokenize("curl 'x").unwrap_err(), CurlError::Unbalanced);
        assert_eq!(tokenize("curl \"x").unwrap_err(), CurlError::Unbalanced);
        // windows line endings in continuations
        assert_eq!(tokenize("curl a \\\r\n b").unwrap(), vec!["curl", "a", "b"]);
    }

    #[test]
    fn chrome_style_copy_as_curl() {
        let cmd = r#"curl 'https://api.test/v1/items?page=2' \
  -H 'accept: application/json' \
  -H 'authorization: Bearer abc' \
  -H 'content-type: application/json' \
  --data-raw $'{"name":"it\'s"}' \
  --compressed"#;
        let (r, notes) = parse_curl(cmd).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(r.method, Method::Post, "data implies POST");
        assert_eq!(r.url, "https://api.test/v1/items?page=2");
        assert_eq!(r.headers.len(), 3);
        assert_eq!(r.headers[1], KeyValue::new("authorization", "Bearer abc"));
        assert_eq!(
            r.body,
            Body::Json {
                raw: "{\"name\":\"it's\"}".into()
            }
        );
        assert_eq!(r.name, "POST api.test/v1/items");
    }

    #[test]
    fn flags_short_long_and_combined() {
        let (r, notes) =
            parse_curl("curl -sSL -XPUT --url=https://x.test/a -H'X-A: 1' -u bob:pw -k -m 2.5")
                .unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(r.method, Method::Put);
        assert_eq!(r.url, "https://x.test/a");
        assert_eq!(r.headers, vec![KeyValue::new("X-A", "1")]);
        assert_eq!(
            r.auth,
            Auth::Basic {
                username: "bob".into(),
                password: "pw".into()
            }
        );
        assert!(!r.settings.verify_tls);
        assert_eq!(r.settings.timeout_ms, 2500);
    }

    #[test]
    fn plain_data_gets_curls_default_content_type() {
        let (r, _) = parse_curl("curl https://f.test -d 'a=1' -d 'b=2'").unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(
            r.body,
            Body::Text {
                raw: "a=1&b=2".into()
            }
        );
        assert_eq!(
            r.headers,
            vec![KeyValue::new(
                "Content-Type",
                "application/x-www-form-urlencoded"
            )]
        );
    }

    #[test]
    fn data_urlencode_and_get() {
        let (r, _) =
            parse_curl("curl -G https://s.test/search --data-urlencode 'q=a b&c' -d n=5").unwrap();
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.body, Body::None);
        assert_eq!(
            r.params,
            vec![KeyValue::new("q", "a b&c"), KeyValue::new("n", "5")]
        );
    }

    #[test]
    fn json_flag_sets_headers() {
        let (r, _) = parse_curl("curl --json '{\"a\":1}' https://j.test").unwrap();
        assert_eq!(r.method, Method::Post);
        assert!(matches!(r.body, Body::Json { .. }));
        assert!(r.headers.iter().any(|h| h.key == "Accept"));
        assert!(r
            .headers
            .iter()
            .any(|h| h.key == "Content-Type" && h.value == "application/json"));
    }

    #[test]
    fn forms_head_cookies_and_misc() {
        let (r, notes) = parse_curl(
            "curl -F 'a=1' -F 'f=@/tmp/x.png;type=image/png' -b 'sid=1' -A ua -e https://r.test https://u.test",
        )
        .unwrap();
        assert_eq!(r.method, Method::Post);
        match &r.body {
            Body::FormData { fields } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(
                    fields[1].value,
                    FormValue::File {
                        path: "/tmp/x.png".into()
                    }
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(notes.iter().any(|n| n.contains(";type=image/png")));
        let keys: Vec<_> = r.headers.iter().map(|h| h.key.as_str()).collect();
        assert_eq!(keys, vec!["Cookie", "User-Agent", "Referer"]);

        let (h, _) = parse_curl("curl -I example.test").unwrap();
        assert_eq!(h.method, Method::Head);
        assert_eq!(h.url, "http://example.test", "curl defaults to http");
    }

    #[test]
    fn unknown_flags_become_notes_and_values_are_not_urls() {
        let (r, notes) =
            parse_curl("curl --proxy http://p:1 -o out.json --weird https://real.test extra")
                .unwrap();
        assert_eq!(r.url, "https://real.test");
        let all = notes.join("\n");
        assert!(all.contains("--proxy http://p:1"), "{all}");
        assert!(all.contains("--weird"), "{all}");
        assert!(all.contains("extra"), "{all}");
        assert!(!all.contains("out.json"), "-o is quiet: {all}");
    }

    #[test]
    fn errors() {
        assert_eq!(parse_curl("wget x").unwrap_err(), CurlError::NotCurl);
        assert_eq!(parse_curl("").unwrap_err(), CurlError::NotCurl);
        assert_eq!(parse_curl("curl -H 'a: b'").unwrap_err(), CurlError::NoUrl);
        assert_eq!(
            parse_curl("curl https://x.test -H").unwrap_err(),
            CurlError::MissingValue("-H".into())
        );
        assert_eq!(
            parse_curl("curl 'https://x.test").unwrap_err(),
            CurlError::Unbalanced
        );
    }

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("a%20b+c%2"), "a b c%2");
        assert_eq!(percent_encode_value("a b&c"), "a%20b%26c");
    }
}
