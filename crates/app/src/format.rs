//! Pure text formatting and parsing: byte sizes, tab titles, expiry labels, name
//! de-duplication, and the "what did the user type in that box" parsers. No UI and no
//! app state, which is what makes them directly unit-testable.

use crate::editor::{BodyMode, RawKind};
use at_core::RequestSettings;

pub(crate) fn human(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1048576.0)
    }
}

/// Longest tab title, in characters; longer names are cut with an ellipsis.
pub(crate) const TAB_TITLE_MAX: usize = 24;

/// Tab button text for a request name: trimmed, "Untitled" when empty, cut with an ellipsis.
pub(crate) fn tab_title(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "Untitled".into();
    }
    if name.chars().count() > TAB_TITLE_MAX {
        let cut: String = name.chars().take(TAB_TITLE_MAX - 1).collect();
        format!("{cut}…")
    } else {
        name.to_string()
    }
}

/// Shorten `s` to `max` characters, marking the cut with an ellipsis.
pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('\u{2026}');
    out
}

/// Import/export notes appended to a status message: ". 2 notes: a; b".
pub(crate) fn notes(warnings: &[String]) -> String {
    if warnings.is_empty() {
        String::new()
    } else {
        format!(". {} note(s): {}", warnings.len(), warnings.join("; "))
    }
}

/// `name`, or `name 2`, `name 3`, ... — the first one `taken` says is free.
pub(crate) fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = if name.trim().is_empty() {
        "Imported"
    } else {
        name.trim()
    };
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !taken(n))
        .unwrap_or_else(|| base.to_string())
}

/// "in 5 min", "in 3 h", "in 2 d", "session" for the cookie list.
pub(crate) fn expires_label(now_secs: i64, expires: Option<i64>) -> String {
    let Some(at) = expires else {
        return "session".into();
    };
    let left = at - now_secs;
    match left {
        i64::MIN..=0 => "expired".into(),
        1..=59 => "in <1 min".into(),
        60..=3599 => format!("in {} min", left / 60),
        3600..=86_399 => format!("in {} h", left / 3600),
        _ => format!("in {} d", left / 86_400),
    }
}

/// Timeout box text -> milliseconds. Blank means the default; anything unparsable keeps
/// `fallback` so a half-typed value never silently becomes "no timeout".
pub(crate) fn parse_timeout(text: &str, fallback: u64) -> u64 {
    let t = text.trim();
    if t.is_empty() {
        return RequestSettings::default().timeout_ms;
    }
    t.parse().unwrap_or(fallback)
}

/// Whole number from an options box, clamped; blank or invalid gives `default`.
pub(crate) fn parse_count(text: &str, default: u64, min: u64, max: u64) -> u64 {
    text.trim()
        .parse::<u64>()
        .map(|n| n.clamp(min, max))
        .unwrap_or(default)
}

/// Short tab-label hint for the body: "Body", "Body (XML)", "Body (form)".
pub(crate) fn body_label(mode: BodyMode, kind: RawKind) -> String {
    match mode {
        BodyMode::None => "Body".into(),
        BodyMode::Raw => format!("Body ({})", kind.label()),
        BodyMode::UrlEncoded => "Body (urlencoded)".into(),
        BodyMode::FormData => "Body (form-data)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::merge_vars;
    use at_core::EnvVar;

    #[test]
    fn clip_counts_characters_not_bytes() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abcd", 3), "abc\u{2026}");
        // a multi-byte character is never cut in half
        assert_eq!(clip("h\u{e9}llo", 2), "h\u{e9}\u{2026}");
        assert_eq!(clip("", 0), "");
    }

    #[test]
    fn tab_titles() {
        assert_eq!(tab_title("Get users"), "Get users");
        assert_eq!(tab_title("  padded  "), "padded");
        assert_eq!(tab_title(""), "Untitled");
        assert_eq!(tab_title("   "), "Untitled");
        let long = "a".repeat(TAB_TITLE_MAX + 10);
        let t = tab_title(&long);
        assert_eq!(t.chars().count(), TAB_TITLE_MAX);
        assert!(t.ends_with('…'));
        // exactly at the limit is left alone
        let exact = "b".repeat(TAB_TITLE_MAX);
        assert_eq!(tab_title(&exact), exact);
        // multi-byte characters are counted as characters, never split
        let wide = "é".repeat(TAB_TITLE_MAX + 5);
        assert_eq!(tab_title(&wide).chars().count(), TAB_TITLE_MAX);
    }

    #[test]
    fn cookie_expiry_labels() {
        let now = 1_000_000;
        assert_eq!(expires_label(now, None), "session");
        assert_eq!(expires_label(now, Some(now - 1)), "expired");
        assert_eq!(expires_label(now, Some(now + 30)), "in <1 min");
        assert_eq!(expires_label(now, Some(now + 600)), "in 10 min");
        assert_eq!(expires_label(now, Some(now + 7200)), "in 2 h");
        assert_eq!(expires_label(now, Some(now + 86_400 * 3)), "in 3 d");
    }

    #[test]
    fn import_helpers() {
        assert_eq!(notes(&[]), "");
        assert_eq!(notes(&["a".into(), "b".into()]), ". 2 note(s): a; b");

        let taken = ["Prod", "Prod 2"];
        let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
        assert_eq!(unique_name("Dev", is_taken), "Dev");
        assert_eq!(unique_name("prod", is_taken), "prod 3");
        assert_eq!(unique_name("  ", is_taken), "Imported");

        let existing = vec![EnvVar::new("a", "1"), EnvVar::new("b", "2")];
        let (merged, replaced) = merge_vars(
            &existing,
            vec![EnvVar::new("b", "20"), EnvVar::new("c", "3")],
        );
        assert_eq!(replaced, 1);
        assert_eq!(
            merged,
            vec![
                EnvVar::new("a", "1"),
                EnvVar::new("b", "20"),
                EnvVar::new("c", "3")
            ]
        );
    }
}
