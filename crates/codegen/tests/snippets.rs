//! Snapshot tests for every [`Lang`] in [`Lang::ALL`] against a fixture matrix of tricky
//! requests (FR-CG-01/FR-CG-02, PLAN Phase 4).
//!
//! Each fixture is one `#[test]` that snapshots all languages, so a failure names the fixture
//! and the snapshot files read as `snippets__<fixture>__<lang>.snap`. Review a new `.snap` by
//! asking one question: would a real interpreter for that language read the literal back as the
//! exact bytes `Built` holds?
use std::time::{Duration, Instant};

use at_codegen::{generate, Lang};
use at_core::{
    build, ApiKeyLocation, Auth, Body, Built, FormField, FormValue, KeyValue, Method, Request,
    Variables,
};

fn built(req: Request) -> Built {
    build(&req, &Variables::default()).unwrap()
}

/// Snapshot the snippet this fixture produces in every language.
fn all_langs(fixture: &str, b: &Built) {
    for lang in Lang::ALL {
        let slug = lang.label().to_ascii_lowercase();
        insta::assert_snapshot!(format!("{fixture}__{slug}"), generate(b, lang));
    }
}

fn get(url: &str) -> Request {
    Request {
        method: Method::Get,
        url: url.into(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- fixtures

#[test]
fn get_with_params() {
    let b = built(Request {
        params: vec![
            KeyValue::new("q", "a b & c=d"),
            KeyValue::new("page", "2"),
            KeyValue::new("tag", "caf\u{e9}/\u{1f600}"),
        ],
        ..get("https://api.test/search")
    });
    all_langs("get_with_params", &b);
}

#[test]
fn post_json() {
    let b = built(Request {
        method: Method::Post,
        body: Body::Json {
            raw: "{\n  \"name\": \"Ada\",\n  \"tags\": [1, 2, 3],\n  \"ok\": true\n}".into(),
        },
        ..get("https://api.test/users")
    });
    all_langs("post_json", &b);
}

/// Every quoting hazard at once: both quote characters, a backtick, a bare `$` plus the two PHP
/// interpolation openers, a `{{placeholder}}` that must survive verbatim, a literal backslash,
/// newlines, tabs and the C0 controls VT/FF/ESC/SOH plus DEL.
///
/// `\r` and NUL are deliberately *not* here: see `nul_and_cr_in_every_literal` below, which
/// asserts them directly instead of through a snapshot file.
#[test]
fn tricky_body() {
    let raw = concat!(
        "single ' double \" backtick ` dollar $ interp {$var} brace ${var}\n",
        "placeholder {{notavar}} backslash \\ not-an-escape \\n \\x41 \\u{41}\n",
        "tab\there\ttwo\tvertical\u{b}form\u{c}escape\u{1b}[0m\n",
        "soh\u{1}del\u{7f}done"
    );
    let b = built(Request {
        method: Method::Post,
        headers: vec![KeyValue::new("X-Tricky", "v\u{1}alue\twith\ttabs")],
        body: Body::Text { raw: raw.into() },
        ..get("https://api.test/tricky")
    });
    all_langs("tricky_body", &b);
}

/// Non-BMP code points (U+1F600, U+1D4B3 — each a surrogate *pair* in the UTF-16 that JS and TS
/// string literals are made of), a ZWJ emoji sequence, a combining accent, RTL text and
/// astral-plane text all have to come through as literal UTF-8, not as escapes.
#[test]
fn unicode_body() {
    let raw = concat!(
        "{\"emoji\": \"\u{1f600}\u{1f3f3}\u{fe0f}\u{200d}\u{1f308}\", ",
        "\"zwj\": \"\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\", ",
        "\"math\": \"\u{1d4b3}\u{1d4c7}\", ",
        "\"combining\": \"e\u{301}cole\", ",
        "\"cjk\": \"\u{6f22}\u{5b57}\", ",
        "\"rtl\": \"\u{645}\u{631}\u{62d}\u{628}\u{627}\"}"
    );
    let b = built(Request {
        method: Method::Post,
        headers: vec![KeyValue::new("X-Emoji", "\u{1f600}")],
        body: Body::Json { raw: raw.into() },
        ..get("https://api.test/unicode")
    });
    all_langs("unicode_body", &b);
}

#[test]
fn urlencoded_body() {
    let b = built(Request {
        method: Method::Post,
        body: Body::UrlEncoded {
            fields: vec![
                KeyValue::new("user", "o'brien"),
                KeyValue::new("note", "a \"quoted\" & $igned value"),
            ],
        },
        ..get("https://api.test/form")
    });
    all_langs("urlencoded_body", &b);
}

#[test]
fn auth_basic() {
    let b = built(Request {
        auth: Auth::Basic {
            username: "ada".into(),
            password: "p@ss:w'rd".into(),
        },
        ..get("https://api.test/me")
    });
    all_langs("auth_basic", &b);
}

#[test]
fn auth_bearer() {
    let b = built(Request {
        auth: Auth::Bearer {
            token: "eyJhbGciOiJIUzI1NiJ9.e30.sig".into(),
        },
        ..get("https://api.test/me")
    });
    all_langs("auth_bearer", &b);
}

#[test]
fn auth_jwt() {
    let b = built(Request {
        auth: Auth::Jwt {
            token: "eyJhbGciOiJIUzI1NiJ9.e30.sig".into(),
            prefix: "JWT".into(),
        },
        ..get("https://api.test/me")
    });
    all_langs("auth_jwt", &b);
}

#[test]
fn auth_api_key_header() {
    let b = built(Request {
        auth: Auth::ApiKey {
            key: "X-Api-Key".into(),
            value: "k'ey \"42\"".into(),
            location: ApiKeyLocation::Header,
        },
        ..get("https://api.test/me")
    });
    all_langs("auth_api_key_header", &b);
}

#[test]
fn auth_api_key_query() {
    let b = built(Request {
        auth: Auth::ApiKey {
            key: "api_key".into(),
            value: "k ey&42".into(),
            location: ApiKeyLocation::Query,
        },
        ..get("https://api.test/me?v=1")
    });
    all_langs("auth_api_key_query", &b);
}

#[test]
fn auth_custom() {
    let b = built(Request {
        auth: Auth::Custom {
            headers: vec![
                KeyValue::new("X-Sig", "abc'def"),
                KeyValue::new("X-Ts", "1699999999"),
            ],
        },
        ..get("https://api.test/me")
    });
    all_langs("auth_custom", &b);
}

#[test]
fn multipart_text_and_file() {
    let b = built(Request {
        method: Method::Put,
        body: Body::FormData {
            fields: vec![
                FormField {
                    key: "note".into(),
                    value: FormValue::Text {
                        value: "it's \"fine\" $here".into(),
                    },
                    enabled: true,
                },
                FormField {
                    key: "skipped".into(),
                    value: FormValue::Text {
                        value: "not sent".into(),
                    },
                    enabled: false,
                },
                FormField {
                    key: "upload".into(),
                    value: FormValue::File {
                        path: "/tmp/a b's \"file\".png".into(),
                    },
                    enabled: true,
                },
            ],
        },
        ..get("https://up.test/u")
    });
    all_langs("multipart_text_and_file", &b);
}

#[test]
fn header_with_quote() {
    let b = built(Request {
        headers: vec![
            KeyValue::new("X-Quote", "he said \"hi\" and it's 'fine'"),
            KeyValue::new("X-Dollar", "cost $5 {$x} ${y} `tick`"),
            KeyValue::new("X-Backslash", "C:\\path\\to\\thing"),
        ],
        ..get("https://api.test/headers")
    });
    all_langs("header_with_quote", &b);
}

/// Two routes to the same hazard: a request header written twice, and an `Auth::Custom` pair that
/// repeats a name. `Built::headers` keeps all five, so a generator that keys headers by name has
/// to change shape (JS/TS) or admit the gap out loud (Python).
#[test]
fn duplicate_headers() {
    let b = built(Request {
        headers: vec![
            KeyValue::new("Accept", "application/json"),
            KeyValue::new("Accept", "text/plain; q=0.5"),
        ],
        auth: Auth::Custom {
            headers: vec![
                KeyValue::new("X-Sig", "first"),
                KeyValue::new("X-Sig", "second"),
                KeyValue::new("X-Ts", "1699999999"),
            ],
        },
        ..get("https://api.test/dupes")
    });
    all_langs("duplicate_headers", &b);
}

/// `files[]`-style repeats: two text parts and two file parts sharing a name. Python splits text
/// and file parts into separate containers, PHP puts all four in one array.
#[test]
fn duplicate_multipart_keys() {
    let b = built(Request {
        method: Method::Post,
        body: Body::FormData {
            fields: vec![
                FormField {
                    key: "tag".into(),
                    value: FormValue::Text {
                        value: "one".into(),
                    },
                    enabled: true,
                },
                FormField {
                    key: "tag".into(),
                    value: FormValue::Text {
                        value: "two".into(),
                    },
                    enabled: true,
                },
                FormField {
                    key: "files[]".into(),
                    value: FormValue::File {
                        path: "/tmp/a.png".into(),
                    },
                    enabled: true,
                },
                FormField {
                    key: "files[]".into(),
                    value: FormValue::File {
                        path: "/tmp/b.png".into(),
                    },
                    enabled: true,
                },
            ],
        },
        ..get("https://up.test/batch")
    });
    all_langs("duplicate_multipart_keys", &b);
}

/// HEAD needs `CURLOPT_NOBODY` in PHP and `--head` in cURL, or the snippet sits waiting for a
/// body the server will never send.
#[test]
fn head_request() {
    let b = built(Request {
        method: Method::Head,
        headers: vec![KeyValue::new("If-None-Match", "\"etag-1\"")],
        ..get("https://api.test/resource")
    });
    all_langs("head_request", &b);
}

// ---------------------------------------------------------------- non-fixture guards

/// FNV-1a: a stable digest that does not change with the toolchain, unlike `DefaultHasher`.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A few hundred KB of body has to come out in linear time and linear size, so the snapshot is a
/// digest plus the head and tail rather than the whole snippet.
#[test]
fn large_body_has_no_quadratic_blowup() {
    let mut raw = String::with_capacity(512 * 1024);
    let mut i = 0u32;
    while raw.len() < 300 * 1024 {
        raw.push_str(&format!("{{\"i\":{i},\"s\":\"a'b\\\"c#d`e$f\"}}\n"));
        i += 1;
    }
    let b = built(Request {
        method: Method::Post,
        body: Body::Text { raw: raw.clone() },
        ..get("https://big.test/ingest")
    });

    let mut report = format!("input bytes: {}\n", raw.len());
    for lang in Lang::ALL {
        let start = Instant::now();
        let out = generate(&b, lang);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(10),
            "{} took {elapsed:?} for {} bytes",
            lang.label(),
            raw.len()
        );
        assert!(
            out.len() < raw.len() * 8,
            "{} produced {} bytes from {}",
            lang.label(),
            out.len(),
            raw.len()
        );
        report.push_str(&format!(
            "{:<11} bytes={:<8} fnv1a={:016x}\n  head={:?}\n  tail={:?}\n",
            lang.label(),
            out.len(),
            fnv1a(&out),
            &out[..70.min(out.len())],
            &out[out.len().saturating_sub(70)..]
        ));
    }
    insta::assert_snapshot!("large_body__digest", report);
}

/// NUL and CR are asserted here rather than snapshotted: a `.snap` file holding a raw NUL would
/// read as binary to git, and insta normalises line endings, so a raw CR could not round-trip
/// through the file either.
#[test]
fn nul_and_cr_in_every_literal() {
    let b = built(Request {
        method: Method::Post,
        body: Body::Text {
            raw: "a\u{0}b\rc".into(),
        },
        ..get("https://nul.test/raw")
    });
    for (lang, nul) in [
        (Lang::Python, "\\u0000"),
        (Lang::JavaScript, "\\u0000"),
        (Lang::TypeScript, "\\u0000"),
        (Lang::Go, "\\u0000"),
        (Lang::Rust, "\\u{0}"),
        (Lang::Php, "\\x00"),
    ] {
        let s = generate(&b, lang);
        assert!(s.contains(nul), "{} lost the NUL: {s:?}", lang.label());
        assert!(s.contains("\\r"), "{} lost the CR: {s:?}", lang.label());
        assert!(
            !s.contains('\u{0}') && !s.contains('\r'),
            "{} left a raw control byte in the snippet: {s:?}",
            lang.label()
        );
    }
    // cURL is the one exception, and it is not fixable: a POSIX `argv` cannot carry a NUL byte,
    // so the byte stays verbatim and the shell truncates the argument there.
    let c = generate(&b, Lang::Curl);
    assert!(c.contains('\u{0}'), "{c:?}");
}
