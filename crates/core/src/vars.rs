//! `{{name}}` substitution with scope precedence (FR-ENV-02, FR-ENV-03), plus the
//! `{{$dynamic}}` fallback (see [`crate::dynamic`]).
use crate::dynamic::{dynamic_value, is_dynamic};
use std::collections::HashMap;

/// Precedence, lowest to highest: global < collection < environment < runtime. Names that
/// start with `$` and match none of these fall back to a fresh dynamic value.
#[derive(Clone, Debug, Default)]
pub struct Variables {
    pub global: HashMap<String, String>,
    pub collection: HashMap<String, String>,
    pub environment: HashMap<String, String>,
    pub runtime: HashMap<String, String>,
    /// Leave known `{{$dynamic}}` names literal instead of generating values, and don't report
    /// them as unresolved. Code snippets set this: a snippet is re-rendered often, and a
    /// placeholder is the honest rendering of "a fresh value on every send".
    pub keep_dynamic: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub text: String,
    /// Names that had no value; they stay literal (`{{name}}`) in `text`.
    pub unresolved: Vec<String>,
}

impl Variables {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.runtime
            .get(name)
            .or_else(|| self.environment.get(name))
            .or_else(|| self.collection.get(name))
            .or_else(|| self.global.get(name))
            .map(|s| s.as_str())
    }

    pub fn resolve(&self, input: &str) -> Resolved {
        let mut out = String::with_capacity(input.len());
        let mut unresolved: Vec<String> = Vec::new();
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find("}}") {
                Some(end) => {
                    let name = after[..end].trim();
                    let literal = &rest[start..start + 2 + end + 2];
                    // User variables first; `$` names only as a fallback, fresh per occurrence.
                    if let Some(v) = self.get(name) {
                        out.push_str(v);
                    } else if is_dynamic(name) {
                        match dynamic_value(name).filter(|_| !self.keep_dynamic) {
                            Some(v) => out.push_str(&v),
                            None => out.push_str(literal),
                        }
                    } else {
                        out.push_str(literal);
                        if !name.is_empty() && !unresolved.iter().any(|u| u == name) {
                            unresolved.push(name.to_string());
                        }
                    }
                    rest = &after[end + 2..];
                }
                None => {
                    // Unterminated `{{`: keep the remainder literally.
                    out.push_str(&rest[start..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        Resolved {
            text: out,
            unresolved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Variables {
        let mut v = Variables::default();
        v.global.insert("host".into(), "global.test".into());
        v.global.insert("only_global".into(), "g".into());
        v.collection.insert("host".into(), "collection.test".into());
        v.environment.insert("host".into(), "env.test".into());
        v
    }

    #[test]
    fn precedence_runtime_wins() {
        let mut v = vars();
        assert_eq!(v.resolve("{{host}}").text, "env.test");
        v.runtime.insert("host".into(), "runtime.test".into());
        assert_eq!(v.resolve("{{host}}").text, "runtime.test");
        assert_eq!(v.resolve("{{only_global}}").text, "g");
    }

    #[test]
    fn unresolved_stay_literal_and_are_reported_once() {
        let r = vars().resolve("a {{missing}} b {{ missing }} c {{host}}");
        assert_eq!(r.text, "a {{missing}} b {{ missing }} c env.test");
        assert_eq!(r.unresolved, vec!["missing".to_string()]);
    }

    #[test]
    fn whitespace_in_name_is_trimmed() {
        assert_eq!(vars().resolve("{{ host }}").text, "env.test");
    }

    #[test]
    fn unterminated_is_literal() {
        assert_eq!(vars().resolve("x {{host").text, "x {{host");
    }

    #[test]
    fn no_vars_is_identity_and_unicode_safe() {
        assert_eq!(vars().resolve("héllo wörld ✓").text, "héllo wörld ✓");
    }

    const UUID: &str = r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}";

    #[test]
    fn dynamic_names_resolve_in_place() {
        let r = vars().resolve("{{host}}/u/{{$uuid}}?t={{ $timestamp }}&n={{$randomInt}}");
        assert!(r.unresolved.is_empty(), "{:?}", r.unresolved);
        let re =
            regex::Regex::new(&format!(r"^env\.test/u/{UUID}\?t=\d{{10}}&n=\d{{1,4}}$")).unwrap();
        assert!(re.is_match(&r.text), "{}", r.text);
    }

    #[test]
    fn each_occurrence_gets_its_own_value() {
        let r = Variables::default().resolve("{{$uuid}} {{$uuid}} {{$guid}}");
        let ids: Vec<&str> = r.text.split(' ').collect();
        assert_eq!(ids.len(), 3);
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[2]);
    }

    #[test]
    fn user_variable_shadows_dynamic_name() {
        let mut v = vars();
        v.global.insert("$uuid".into(), "fixed-global".into());
        assert_eq!(v.resolve("{{$uuid}}").text, "fixed-global");
        v.environment.insert("$uuid".into(), "fixed-env".into());
        assert_eq!(v.resolve("{{$uuid}}").text, "fixed-env");
    }

    #[test]
    fn unknown_dollar_name_stays_unresolved() {
        let r = vars().resolve("a {{$foo}} b {{$foo}} {{$uuid}}");
        assert!(r.text.starts_with("a {{$foo}} b {{$foo}} "), "{}", r.text);
        assert!(!r.text.contains("{{$uuid}}"), "{}", r.text);
        assert_eq!(r.unresolved, vec!["$foo".to_string()]);
    }

    #[test]
    fn keep_dynamic_leaves_placeholders_without_reporting_them() {
        let mut v = Variables {
            keep_dynamic: true,
            ..vars()
        };
        let r = v.resolve("{{host}}/{{$uuid}}/{{ $timestamp }}/{{$foo}}");
        assert_eq!(r.text, "env.test/{{$uuid}}/{{ $timestamp }}/{{$foo}}");
        assert_eq!(r.unresolved, vec!["$foo".to_string()]);
        // a user value still wins over the placeholder
        v.runtime.insert("$uuid".into(), "mine".into());
        assert_eq!(v.resolve("{{$uuid}}").text, "mine");
    }

    #[test]
    fn dynamic_values_reach_every_part_of_a_built_request() {
        use crate::models::{Auth, Body, KeyValue, Request};
        let kv = KeyValue::new;
        let req = Request {
            url: "https://x.test/{{$randomInt}}".into(),
            params: vec![kv("id", "{{$uuid}}")],
            headers: vec![kv("X-Req", "{{$guid}}")],
            auth: Auth::Bearer {
                token: "{{$randomUUID}}".into(),
            },
            body: Body::Json {
                raw: r#"{"at":"{{$isoTimestamp}}"}"#.into(),
            },
            ..Request::default()
        };
        let b = crate::build(&req, &Variables::default()).unwrap();
        assert!(b.unresolved.is_empty(), "{:?}", b.unresolved);
        assert!(
            !b.url.contains("%7B%7B") && !b.url.contains("{{"),
            "{}",
            b.url
        );
        assert!(b.headers.iter().all(|(_, v)| !v.contains("{{")));
        let crate::BuiltBody::Bytes(body) = b.body else {
            panic!("expected bytes");
        };
        assert!(!String::from_utf8(body).unwrap().contains("{{"));
        // code snippets keep the placeholders
        let snippet_vars = Variables {
            keep_dynamic: true,
            ..Variables::default()
        };
        let t = crate::build_template(&req, &snippet_vars).unwrap();
        assert!(
            t.url.starts_with("https://x.test/{{$randomInt}}"),
            "{}",
            t.url
        );
        assert!(t.unresolved.is_empty());
    }
}
