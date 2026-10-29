//! Environments and global variables (FR-ENV-01, FR-ENV-02, FR-ENV-04).
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

/// One variable. `secret` values are masked in the UI and left out of exports (FR-ENV-04).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl EnvVar {
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            secret: false,
            enabled: true,
        }
    }
}

/// A named set of variables (Local, Staging, Production, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    #[serde(default)]
    pub variables: Vec<EnvVar>,
}

/// Enabled variables with a non-empty key, as a lookup map. Keys are trimmed (matching how
/// `{{ name }}` is trimmed when resolving); a later duplicate overrides an earlier one.
pub fn to_map(vars: &[EnvVar]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for v in vars {
        let k = v.key.trim();
        if v.enabled && !k.is_empty() {
            m.insert(k.to_string(), v.value.clone());
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_skips_disabled_and_empty_keys() {
        let vars = vec![
            EnvVar::new("host", "a.test"),
            EnvVar {
                enabled: false,
                ..EnvVar::new("off", "1")
            },
            EnvVar::new("  ", "ignored"),
            EnvVar::new(" padded ", "p"),
        ];
        let m = to_map(&vars);
        assert_eq!(m.len(), 2);
        assert_eq!(m["host"], "a.test");
        assert_eq!(m["padded"], "p");
    }

    #[test]
    fn later_duplicate_wins() {
        let vars = vec![EnvVar::new("k", "1"), EnvVar::new("k", "2")];
        assert_eq!(to_map(&vars)["k"], "2");
    }

    #[test]
    fn json_defaults() {
        let v: EnvVar = serde_json::from_str(r#"{"key":"a"}"#).unwrap();
        assert!(v.enabled && !v.secret && v.value.is_empty());
    }
}
