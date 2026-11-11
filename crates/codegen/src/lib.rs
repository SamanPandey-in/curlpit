//! Code generation (FR-GEN): turn a built request into a snippet for another tool or language,
//! and read a cURL command back into a request.
//!
//! Generators take an [`at_core::Built`], the request exactly as the engine would send it
//! (params encoded into the URL, auth turned into headers), so snippets behave the same as
//! pressing Send.
use at_core::{Built, BuiltBody, BuiltPart, Method};

mod curl_import;
pub use curl_import::{parse_curl, CurlError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Curl,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Rust,
    Php,
}

impl Lang {
    pub const ALL: [Lang; 7] = [
        Lang::Curl,
        Lang::Python,
        Lang::JavaScript,
        Lang::TypeScript,
        Lang::Go,
        Lang::Rust,
        Lang::Php,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Lang::Curl => "cURL",
            Lang::Python => "Python",
            Lang::JavaScript => "JavaScript",
            Lang::TypeScript => "TypeScript",
            Lang::Go => "Go",
            Lang::Rust => "Rust",
            Lang::Php => "PHP",
        }
    }
}

pub fn generate(b: &Built, lang: Lang) -> String {
    match lang {
        Lang::Curl => curl(b),
        Lang::Python => python(b),
        Lang::JavaScript => javascript(b),
        Lang::TypeScript => typescript(b),
        Lang::Go => go(b),
        Lang::Rust => rust(b),
        Lang::Php => php(b),
    }
}

fn body_text(b: &Built) -> Option<String> {
    match &b.body {
        BuiltBody::Bytes(bytes) if !bytes.is_empty() => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => None,
    }
}

fn parts(b: &Built) -> Option<&[BuiltPart]> {
    match &b.body {
        BuiltBody::Multipart(p) => Some(p),
        _ => None,
    }
}

/// A double-quoted string literal that is valid in Python, JavaScript, TypeScript and Go
/// (JSON string syntax is a subset of all four).
fn dq(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// The names that appear more than once, in first-seen order. Object and dict literals key on
/// exact string equality, so only an exact repeat collapses; `X-A` next to `x-a` survives even
/// though HTTP treats the two as one header.
fn duplicates<'a, I: Iterator<Item = &'a str>>(names: I) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    let mut dups: Vec<&str> = Vec::new();
    for n in names {
        if seen.contains(&n) {
            if !dups.contains(&n) {
                dups.push(n);
            }
        } else {
            seen.push(n);
        }
    }
    dups
}

/// Header names `b` sends twice. A generator that keys headers by name has to either switch to a
/// form that keeps them or say out loud that it cannot (D29: a snippet must not disagree with
/// Send).
fn header_dups(b: &Built) -> Vec<&str> {
    duplicates(b.headers.iter().map(|(k, _)| k.as_str()))
}

fn part_name(p: &BuiltPart) -> &str {
    match p {
        BuiltPart::Text { name, .. } | BuiltPart::File { name, .. } => name,
    }
}

// ---------------------------------------------------------------- cURL

/// Single-quote for POSIX shells: `it's` -> `'it'\''s'`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn curl(b: &Built) -> String {
    let mut lines = Vec::new();
    let first = match b.method {
        Method::Get => format!("curl {}", shell_quote(&b.url)),
        Method::Head => format!("curl --head {}", shell_quote(&b.url)),
        m => format!("curl -X {} {}", m.as_str(), shell_quote(&b.url)),
    };
    lines.push(first);
    for (k, v) in &b.headers {
        lines.push(format!("  -H {}", shell_quote(&format!("{k}: {v}"))));
    }
    if let Some(t) = body_text(b) {
        lines.push(format!("  --data-raw {}", shell_quote(&t)));
    }
    if let Some(ps) = parts(b) {
        for p in ps {
            let f = match p {
                BuiltPart::Text { name, value } => format!("{name}={value}"),
                BuiltPart::File { name, path } => format!("{name}=@{path}"),
            };
            lines.push(format!("  -F {}", shell_quote(&f)));
        }
    }
    lines.join(" \\\n")
}

// ---------------------------------------------------------------- Python (requests)

fn python(b: &Built) -> String {
    let mut s = String::from("import requests\n\n");
    s.push_str(&format!("url = {}\n", dq(&b.url)));
    let mut args = vec![dq(b.method.as_str()), "url".to_string()];
    if !b.headers.is_empty() {
        // `requests` only takes a mapping of headers, so a repeated name genuinely cannot be
        // expressed. Say so rather than silently sending something else.
        let dups = header_dups(b);
        if !dups.is_empty() {
            s.push_str(&format!(
                "# NOTE: requests takes headers as a mapping, so {} repeated header name{} \
                 cannot be\n# represented here and only the last value of each is sent: {}\n",
                dups.len(),
                if dups.len() == 1 { "" } else { "s" },
                dups.join(", ")
            ));
        }
        s.push_str("headers = {\n");
        for (k, v) in &b.headers {
            s.push_str(&format!("    {}: {},\n", dq(k), dq(v)));
        }
        s.push_str("}\n");
        args.push("headers=headers".into());
    }
    if let Some(t) = body_text(b) {
        s.push_str(&format!("data = {}\n", dq(&t)));
        args.push("data=data.encode(\"utf-8\")".into());
    }
    if let Some(ps) = parts(b) {
        let texts: Vec<_> = ps
            .iter()
            .filter_map(|p| match p {
                BuiltPart::Text { name, value } => Some((name, value)),
                _ => None,
            })
            .collect();
        let files: Vec<_> = ps
            .iter()
            .filter_map(|p| match p {
                BuiltPart::File { name, path } => Some((name, path)),
                _ => None,
            })
            .collect();
        if !texts.is_empty() {
            // A dict would keep only the last of a repeated name; `requests` also accepts a list
            // of tuples, which keeps every one.
            if duplicates(texts.iter().map(|(k, _)| k.as_str())).is_empty() {
                s.push_str("data = {\n");
                for (k, v) in &texts {
                    s.push_str(&format!("    {}: {},\n", dq(k), dq(v)));
                }
                s.push_str("}\n");
            } else {
                s.push_str("data = [\n");
                for (k, v) in &texts {
                    s.push_str(&format!("    ({}, {}),\n", dq(k), dq(v)));
                }
                s.push_str("]\n");
            }
            args.push("data=data".into());
        }
        if !files.is_empty() {
            if duplicates(files.iter().map(|(k, _)| k.as_str())).is_empty() {
                s.push_str("files = {\n");
                for (k, p) in &files {
                    s.push_str(&format!("    {}: open({}, \"rb\"),\n", dq(k), dq(p)));
                }
                s.push_str("}\n");
            } else {
                s.push_str("files = [\n");
                for (k, p) in &files {
                    s.push_str(&format!("    ({}, open({}, \"rb\")),\n", dq(k), dq(p)));
                }
                s.push_str("]\n");
            }
            args.push("files=files".into());
        }
    }
    s.push_str(&format!(
        "\nresponse = requests.request({})\n",
        args.join(", ")
    ));
    s.push_str("print(response.status_code)\nprint(response.text)\n");
    s
}

// ---------------------------------------------------------------- JavaScript (fetch)

fn javascript(b: &Built) -> String {
    let mut s = String::new();
    let multipart = parts(b);
    let has_files = multipart
        .map(|ps| ps.iter().any(|p| matches!(p, BuiltPart::File { .. })))
        .unwrap_or(false);
    if has_files {
        s.push_str("import fs from \"node:fs\";\n\n");
    }
    if let Some(ps) = multipart {
        s.push_str("const form = new FormData();\n");
        for p in ps {
            match p {
                BuiltPart::Text { name, value } => {
                    s.push_str(&format!("form.append({}, {});\n", dq(name), dq(value)))
                }
                BuiltPart::File { name, path } => s.push_str(&format!(
                    "form.append({}, await fs.openAsBlob({}), {});\n",
                    dq(name),
                    dq(path),
                    dq(file_name(path))
                )),
            }
        }
        s.push('\n');
    }
    s.push_str(&format!(
        "const response = await fetch({}, {{\n",
        dq(&b.url)
    ));
    s.push_str(&format!("  method: {},\n", dq(b.method.as_str())));
    if !b.headers.is_empty() {
        if header_dups(b).is_empty() {
            s.push_str("  headers: {\n");
            for (k, v) in &b.headers {
                s.push_str(&format!("    {}: {},\n", dq(k), dq(v)));
            }
            s.push_str("  },\n");
        } else {
            // An object literal keeps only the last of a repeated name. `fetch` also takes the
            // pairs form, and `Headers` appends rather than replaces, so every one is sent.
            s.push_str("  headers: [\n");
            for (k, v) in &b.headers {
                s.push_str(&format!("    [{}, {}],\n", dq(k), dq(v)));
            }
            s.push_str("  ],\n");
        }
    }
    if let Some(t) = body_text(b) {
        s.push_str(&format!("  body: {},\n", dq(&t)));
    }
    if multipart.is_some() {
        s.push_str("  body: form,\n");
    }
    s.push_str("});\n\nconsole.log(response.status);\nconsole.log(await response.text());\n");
    s
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

// ---------------------------------------------------------------- TypeScript (fetch)

/// The same request as [`javascript`], but with the annotations a `tsc --strict` reader wants:
/// `Record<string, string>` headers, a `RequestInit` options object and a typed `Response`.
/// Files take the same `fs.openAsBlob` route (Node 19.8+) through a named import, so the
/// snippet type-checks without `esModuleInterop`.
fn typescript(b: &Built) -> String {
    let mut s = String::new();
    let multipart = parts(b);
    let text = body_text(b);
    let has_files = multipart
        .map(|ps| ps.iter().any(|p| matches!(p, BuiltPart::File { .. })))
        .unwrap_or(false);
    if has_files {
        s.push_str("import { openAsBlob } from \"node:fs\";\n\n");
    }
    if let Some(ps) = multipart {
        s.push_str("const form: FormData = new FormData();\n");
        for p in ps {
            match p {
                BuiltPart::Text { name, value } => {
                    s.push_str(&format!("form.append({}, {});\n", dq(name), dq(value)))
                }
                BuiltPart::File { name, path } => s.push_str(&format!(
                    "form.append({}, await openAsBlob({}), {});\n",
                    dq(name),
                    dq(path),
                    dq(file_name(path))
                )),
            }
        }
        s.push('\n');
    }
    s.push_str(&format!("const url = {};\n", dq(&b.url)));
    if !b.headers.is_empty() {
        if header_dups(b).is_empty() {
            s.push_str("const headers: Record<string, string> = {\n");
            for (k, v) in &b.headers {
                s.push_str(&format!("  {}: {},\n", dq(k), dq(v)));
            }
            s.push_str("};\n");
        } else {
            // `Record<string, string>` cannot hold a repeated name; the pairs form can, and is
            // just as much a `HeadersInit`.
            s.push_str("const headers: [string, string][] = [\n");
            for (k, v) in &b.headers {
                s.push_str(&format!("  [{}, {}],\n", dq(k), dq(v)));
            }
            s.push_str("];\n");
        }
    }
    if let Some(t) = &text {
        s.push_str(&format!("const body: string = {};\n", dq(t)));
    }
    s.push_str("\nconst options: RequestInit = {\n");
    s.push_str(&format!("  method: {},\n", dq(b.method.as_str())));
    if !b.headers.is_empty() {
        s.push_str("  headers,\n");
    }
    if text.is_some() {
        s.push_str("  body,\n");
    }
    if multipart.is_some() {
        s.push_str("  body: form,\n");
    }
    s.push_str("};\n\nconst res: Response = await fetch(url, options);\n");
    s.push_str("console.log(res.status);\nconsole.log(await res.text());\n");
    s
}

// ---------------------------------------------------------------- Go (net/http)

fn go(b: &Built) -> String {
    let text = body_text(b);
    let multipart = parts(b);
    let has_files = multipart
        .map(|ps| ps.iter().any(|p| matches!(p, BuiltPart::File { .. })))
        .unwrap_or(false);

    let mut imports = vec!["fmt", "io", "net/http"];
    if text.is_some() {
        imports.push("strings");
    }
    if multipart.is_some() {
        imports.extend(["bytes", "mime/multipart"]);
    }
    if has_files {
        imports.extend(["os", "path/filepath"]);
    }
    imports.sort();

    let mut s = String::from("package main\n\nimport (\n");
    for i in &imports {
        s.push_str(&format!("\t\"{i}\"\n"));
    }
    s.push_str(")\n\nfunc main() {\n");
    let body_expr = if let Some(t) = &text {
        format!("strings.NewReader({})", dq(t))
    } else if multipart.is_some() {
        "&buf".to_string()
    } else {
        "nil".to_string()
    };
    if let Some(ps) = multipart {
        s.push_str("\tvar buf bytes.Buffer\n\tw := multipart.NewWriter(&buf)\n");
        for p in ps {
            match p {
                BuiltPart::Text { name, value } => {
                    s.push_str(&format!("\tw.WriteField({}, {})\n", dq(name), dq(value)))
                }
                BuiltPart::File { name, path } => s.push_str(&format!(
                    "\t{{\n\t\tf, err := os.Open({p})\n\t\tif err != nil {{\n\t\t\tpanic(err)\n\t\t}}\n\t\tpart, err := w.CreateFormFile({n}, filepath.Base({p}))\n\t\tif err != nil {{\n\t\t\tpanic(err)\n\t\t}}\n\t\tio.Copy(part, f)\n\t\tf.Close()\n\t}}\n",
                    p = dq(path),
                    n = dq(name)
                )),
            }
        }
        s.push_str("\tw.Close()\n\n");
    }
    s.push_str(&format!(
        "\treq, err := http.NewRequest({}, {}, {body_expr})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n",
        dq(b.method.as_str()),
        dq(&b.url)
    ));
    for (k, v) in &b.headers {
        s.push_str(&format!("\treq.Header.Add({}, {})\n", dq(k), dq(v)));
    }
    if multipart.is_some() {
        s.push_str("\treq.Header.Set(\"Content-Type\", w.FormDataContentType())\n");
    }
    s.push_str(
        "\n\tresp, err := http.DefaultClient.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer resp.Body.Close()\n\tout, _ := io.ReadAll(resp.Body)\n\tfmt.Println(resp.Status)\n\tfmt.Println(string(out))\n}\n",
    );
    s
}

// ---------------------------------------------------------------- Rust (reqwest)

/// A Rust string literal: a raw string when the text has quotes or backslashes, else a normal one.
pub fn rust_str(s: &str) -> String {
    let needs_escape = s.chars().any(|c| c.is_control() && c != '\n' && c != '\t');
    if !needs_escape && (s.contains('"') || s.contains('\\')) {
        let mut hashes = 1;
        while s.contains(&format!("\"{}", "#".repeat(hashes))) {
            hashes += 1;
        }
        let h = "#".repeat(hashes);
        return format!("r{h}\"{s}\"{h}");
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn rust(b: &Built) -> String {
    let multipart = parts(b);
    let mut features = vec!["\"blocking\""];
    if multipart.is_some() {
        features.push("\"multipart\"");
    }
    let mut s = format!(
        "// Cargo.toml: reqwest = {{ version = \"0.12\", features = [{}] }}\n\nfn main() -> Result<(), Box<dyn std::error::Error>> {{\n",
        features.join(", ")
    );
    if let Some(ps) = multipart {
        s.push_str("    let form = reqwest::blocking::multipart::Form::new()");
        for p in ps {
            match p {
                BuiltPart::Text { name, value } => s.push_str(&format!(
                    "\n        .text({}, {})",
                    rust_str(name),
                    rust_str(value)
                )),
                BuiltPart::File { name, path } => s.push_str(&format!(
                    "\n        .file({}, {})?",
                    rust_str(name),
                    rust_str(path)
                )),
            }
        }
        s.push_str(";\n");
    }
    s.push_str("    let response = reqwest::blocking::Client::new()\n");
    s.push_str(&format!(
        "        .request(reqwest::Method::{}, {})\n",
        b.method.as_str(),
        rust_str(&b.url)
    ));
    for (k, v) in &b.headers {
        s.push_str(&format!(
            "        .header({}, {})\n",
            rust_str(k),
            rust_str(v)
        ));
    }
    if let Some(t) = body_text(b) {
        s.push_str(&format!("        .body({})\n", rust_str(&t)));
    }
    if multipart.is_some() {
        s.push_str("        .multipart(form)\n");
    }
    s.push_str(
        "        .send()?;\n    println!(\"{}\", response.status());\n    println!(\"{}\", response.text()?);\n    Ok(())\n}\n",
    );
    s
}

// ---------------------------------------------------------------- PHP (cURL)

/// A PHP double-quoted string literal. Double quotes are the only PHP form that can spell a
/// control character, so every value uses them; the cost is interpolation, cancelled by escaping
/// every `$` (which also neutralises `${` and `{$`, as both need a bare `$`). `\xHH` is always
/// written with two digits so a following hex digit can't be swallowed, and C1 controls use
/// `\u{..}` because `\xHH` above 0x7f would emit one raw byte instead of UTF-8. Everything else,
/// multi-byte UTF-8 included, is kept literally.
pub fn php_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{b}' => out.push_str("\\v"),
            '\u{c}' => out.push_str("\\f"),
            '\u{1b}' => out.push_str("\\e"),
            c if c.is_control() && (c as u32) < 0x80 => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn php(b: &Built) -> String {
    let multipart = parts(b);
    let mut s = String::from("<?php\n\n$ch = curl_init();\ncurl_setopt_array($ch, [\n");
    s.push_str(&format!("    CURLOPT_URL => {},\n", php_str(&b.url)));
    s.push_str(&format!(
        "    CURLOPT_CUSTOMREQUEST => {},\n",
        php_str(b.method.as_str())
    ));
    s.push_str("    CURLOPT_RETURNTRANSFER => true,\n");
    if b.method == Method::Head {
        // CURLOPT_CUSTOMREQUEST alone would leave curl waiting for a body a HEAD never has.
        s.push_str("    CURLOPT_NOBODY => true,\n");
    }
    if !b.headers.is_empty() {
        s.push_str("    CURLOPT_HTTPHEADER => [\n");
        for (k, v) in &b.headers {
            s.push_str(&format!("        {},\n", php_str(&format!("{k}: {v}"))));
        }
        s.push_str("    ],\n");
    }
    if let Some(t) = body_text(b) {
        s.push_str(&format!("    CURLOPT_POSTFIELDS => {},\n", php_str(&t)));
    }
    if let Some(ps) = multipart {
        // A PHP array has unique keys and CURLOPT_POSTFIELDS has no list form, so a repeated
        // part name cannot be expressed without hand-building the multipart body.
        let dups = duplicates(ps.iter().map(part_name));
        if !dups.is_empty() {
            s.push_str(&format!(
                "    // NOTE: a PHP array has unique keys, so {} repeated part name{} cannot be\n\
                 \x20   // represented here and only the last value of each is sent: {}\n",
                dups.len(),
                if dups.len() == 1 { "" } else { "s" },
                dups.join(", ")
            ));
        }
        s.push_str("    CURLOPT_POSTFIELDS => [\n");
        for p in ps {
            match p {
                BuiltPart::Text { name, value } => s.push_str(&format!(
                    "        {} => {},\n",
                    php_str(name),
                    php_str(value)
                )),
                BuiltPart::File { name, path } => s.push_str(&format!(
                    "        {} => new CURLFile({}),\n",
                    php_str(name),
                    php_str(path)
                )),
            }
        }
        s.push_str("    ],\n");
    }
    s.push_str("]);\n\n$response = curl_exec($ch);\n");
    s.push_str("$status = curl_getinfo($ch, CURLINFO_RESPONSE_CODE);\ncurl_close($ch);\n\n");
    s.push_str("echo $status, \"\\n\";\necho $response, \"\\n\";\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use at_core::{build, Body, FormField, FormValue, KeyValue, Request, Variables};

    fn built(req: Request) -> Built {
        build(&req, &Variables::default()).unwrap()
    }

    fn post_json() -> Built {
        built(Request {
            method: Method::Post,
            url: "https://api.test/users".into(),
            params: vec![KeyValue::new("q", "a b")],
            headers: vec![KeyValue::new("X-Note", "it's \"quoted\"")],
            body: Body::Json {
                raw: "{\"name\": \"O'Brien\", \"path\": \"C:\\\\x\"}".into(),
            },
            ..Default::default()
        })
    }

    fn multipart() -> Built {
        built(Request {
            method: Method::Put,
            url: "https://up.test/u".into(),
            body: Body::FormData {
                fields: vec![
                    FormField {
                        key: "note".into(),
                        value: FormValue::Text { value: "hi".into() },
                        enabled: true,
                    },
                    FormField {
                        key: "file".into(),
                        value: FormValue::File {
                            path: "/tmp/a b.png".into(),
                        },
                        enabled: true,
                    },
                ],
            },
            ..Default::default()
        })
    }

    #[test]
    fn curl_snippet_quotes_safely() {
        let s = generate(&post_json(), Lang::Curl);
        assert!(
            s.starts_with("curl -X POST 'https://api.test/users?q=a%20b'"),
            "{s}"
        );
        assert!(s.contains("-H 'X-Note: it'\\''s \"quoted\"'"), "{s}");
        assert!(s.contains("-H 'Content-Type: application/json'"), "{s}");
        assert!(s.contains("--data-raw '{\"name\": \"O'\\''Brien\""), "{s}");
        let get = generate(
            &built(Request {
                url: "https://x.test".into(),
                ..Default::default()
            }),
            Lang::Curl,
        );
        assert_eq!(get, "curl 'https://x.test'");
    }

    #[test]
    fn curl_multipart() {
        let s = generate(&multipart(), Lang::Curl);
        assert!(s.contains("-X PUT"));
        assert!(s.contains("-F 'note=hi'"));
        assert!(s.contains("-F 'file=@/tmp/a b.png'"));
    }

    #[test]
    fn python_snippet() {
        let s = generate(&post_json(), Lang::Python);
        assert!(s.contains("url = \"https://api.test/users?q=a%20b\""));
        assert!(s.contains("\"X-Note\": \"it's \\\"quoted\\\"\""), "{s}");
        assert!(s.contains("response = requests.request(\"POST\", url, headers=headers, data=data.encode(\"utf-8\"))"), "{s}");
        let m = generate(&multipart(), Lang::Python);
        assert!(
            m.contains("\"file\": open(\"/tmp/a b.png\", \"rb\")"),
            "{m}"
        );
        assert!(m.contains("data=data, files=files"), "{m}");
    }

    #[test]
    fn javascript_snippet() {
        let s = generate(&post_json(), Lang::JavaScript);
        assert!(s.contains("await fetch(\"https://api.test/users?q=a%20b\", {"));
        assert!(s.contains("method: \"POST\""));
        assert!(s.contains("body: \"{\\\"name\\\""), "{s}");
        assert!(!s.contains("node:fs"));
        let m = generate(&multipart(), Lang::JavaScript);
        assert!(m.starts_with("import fs from \"node:fs\";"));
        assert!(
            m.contains("fs.openAsBlob(\"/tmp/a b.png\"), \"a b.png\""),
            "{m}"
        );
        assert!(m.contains("body: form"));
        assert!(
            !m.contains("Content-Type"),
            "multipart boundary header is left to fetch"
        );
    }

    #[test]
    fn go_snippet_imports_only_what_it_uses() {
        let get = generate(
            &built(Request {
                url: "https://x.test".into(),
                ..Default::default()
            }),
            Lang::Go,
        );
        assert!(get.contains("http.NewRequest(\"GET\", \"https://x.test\", nil)"));
        assert!(!get.contains("\"strings\""));
        let s = generate(&post_json(), Lang::Go);
        assert!(s.contains("\"strings\""));
        assert!(s.contains("strings.NewReader("));
        let m = generate(&multipart(), Lang::Go);
        for i in [
            "\"bytes\"",
            "\"mime/multipart\"",
            "\"os\"",
            "\"path/filepath\"",
        ] {
            assert!(m.contains(i), "{i} missing in {m}");
        }
        assert!(m.contains("w.FormDataContentType()"));
    }

    #[test]
    fn rust_snippet_and_literals() {
        let s = generate(&post_json(), Lang::Rust);
        assert!(s.contains("reqwest::Method::POST"));
        assert!(s.contains(".body(r#\"{\"name\": \"O'Brien\""), "{s}");
        let m = generate(&multipart(), Lang::Rust);
        assert!(m.contains("\"multipart\""));
        assert!(m.contains(".file(\"file\", \"/tmp/a b.png\")?"));
        assert_eq!(rust_str("plain"), "\"plain\"");
        assert_eq!(rust_str("say \"hi\""), "r#\"say \"hi\"\"#");
        assert_eq!(rust_str("a\"#b"), "r##\"a\"#b\"##");
        assert_eq!(rust_str("x\u{1}y\"z"), "\"x\\u{1}y\\\"z\"");
    }

    #[test]
    fn every_language_handles_every_method_and_empty_body() {
        for m in [Method::Get, Method::Head, Method::Delete, Method::Options] {
            let b = built(Request {
                method: m,
                url: "https://m.test".into(),
                ..Default::default()
            });
            for lang in Lang::ALL {
                let s = generate(&b, lang);
                assert!(s.contains("https://m.test"), "{lang:?} {m:?}");
            }
        }
    }

    #[test]
    fn curl_roundtrip_sends_the_same_request() {
        for b in [post_json(), multipart()] {
            let snippet = generate(&b, Lang::Curl);
            let (req, notes) = parse_curl(&snippet).unwrap();
            assert!(notes.is_empty(), "{notes:?}");
            let again = built(req);
            assert_eq!(again.method, b.method);
            assert_eq!(again.url, b.url);
            assert_eq!(again.headers, b.headers);
            assert_eq!(again.body, b.body);
        }
    }
}
