//! `{{name}}` substitution with scope precedence (FR-ENV-02, FR-ENV-03).
use std::collections::HashMap;

/// Precedence, lowest to highest: global < collection < environment < runtime.
#[derive(Clone, Debug, Default)]
pub struct Variables {
    pub global: HashMap<String, String>,
    pub collection: HashMap<String, String>,
    pub environment: HashMap<String, String>,
    pub runtime: HashMap<String, String>,
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
                    match self.get(name) {
                        Some(v) => out.push_str(v),
                        None => {
                            out.push_str(&rest[start..start + 2 + end + 2]);
                            if !name.is_empty() && !unresolved.iter().any(|u| u == name) {
                                unresolved.push(name.to_string());
                            }
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
}
