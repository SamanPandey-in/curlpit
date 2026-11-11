//! The request editor's own vocabulary: which editor tab is open, what the Body and Auth
//! tabs are editing, the editable key/value row types backed by `InputState`, and the
//! `collect*` functions that turn those rows back into `at_core` request data.

use at_core::{Body, EnvVar, FormField, FormValue, KeyValue, Method};
use gpui::{App, Entity};
use gpui_component::input::InputState;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ReqTab {
    Params,
    Headers,
    Body,
    Auth,
    Scripts,
    Settings,
}

/// What the Body tab edits.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum BodyMode {
    None,
    Raw,
    UrlEncoded,
    FormData,
}

/// Flavour of a raw body; decides the default Content-Type.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum RawKind {
    Json,
    Xml,
    Html,
    Text,
}

impl RawKind {
    pub(crate) const ALL: [RawKind; 4] =
        [RawKind::Json, RawKind::Xml, RawKind::Html, RawKind::Text];

    pub(crate) fn label(self) -> &'static str {
        match self {
            RawKind::Json => "JSON",
            RawKind::Xml => "XML",
            RawKind::Html => "HTML",
            RawKind::Text => "Text",
        }
    }

    pub(crate) fn body(self, raw: String) -> Body {
        match self {
            RawKind::Json => Body::Json { raw },
            RawKind::Xml => Body::Xml { raw },
            RawKind::Html => Body::Html { raw },
            RawKind::Text => Body::Text { raw },
        }
    }
}

/// What the Auth tab edits. `Kept` is an auth type without an editor yet (JWT, custom
/// headers) that came from a file: it is sent and saved unchanged.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum AuthKind {
    None,
    Basic,
    Bearer,
    ApiKey,
    Kept,
}

impl AuthKind {
    pub(crate) const EDITABLE: [AuthKind; 4] = [
        AuthKind::None,
        AuthKind::Basic,
        AuthKind::Bearer,
        AuthKind::ApiKey,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            AuthKind::None => "None",
            AuthKind::Basic => "Basic",
            AuthKind::Bearer => "Bearer token",
            AuthKind::ApiKey => "API key",
            AuthKind::Kept => "From file",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Which {
    Params,
    Headers,
    /// urlencoded body fields
    Form,
}

pub(crate) fn next_method(m: Method) -> Method {
    match m {
        Method::Get => Method::Post,
        Method::Post => Method::Put,
        Method::Put => Method::Patch,
        Method::Patch => Method::Delete,
        Method::Delete => Method::Head,
        Method::Head => Method::Options,
        _ => Method::Get,
    }
}

pub(crate) fn has_body(m: Method) -> bool {
    matches!(
        m,
        Method::Post | Method::Put | Method::Patch | Method::Delete
    )
}

/// One editable key/value row (params, headers, urlencoded fields).
pub(crate) struct KvRow {
    pub(crate) enabled: bool,
    pub(crate) key: Entity<InputState>,
    pub(crate) value: Entity<InputState>,
}

/// Rows -> key/values. Completely empty rows (the spare row at the bottom) are skipped so they
/// never get written into saved files.
pub(crate) fn collect(rows: &[KvRow], cx: &App) -> Vec<KeyValue> {
    rows.iter()
        .filter_map(|r| {
            let key = r.key.read(cx).value().to_string();
            let value = r.value.read(cx).value().to_string();
            if key.trim().is_empty() && value.is_empty() {
                return None;
            }
            Some(KeyValue {
                key,
                value,
                description: String::new(),
                enabled: r.enabled,
            })
        })
        .collect()
}

/// One form-data row: a text value or a file path.
pub(crate) struct FormRow {
    pub(crate) enabled: bool,
    pub(crate) is_file: bool,
    pub(crate) key: Entity<InputState>,
    /// The text value, or the file path when `is_file`.
    pub(crate) value: Entity<InputState>,
}

/// Form-data rows -> fields; completely empty rows are skipped like in `collect`.
pub(crate) fn collect_form(rows: &[FormRow], cx: &App) -> Vec<FormField> {
    rows.iter()
        .filter_map(|r| {
            let key = r.key.read(cx).value().to_string();
            let value = r.value.read(cx).value().to_string();
            if key.trim().is_empty() && value.is_empty() {
                return None;
            }
            Some(FormField {
                key,
                value: if r.is_file {
                    FormValue::File { path: value }
                } else {
                    FormValue::Text { value }
                },
                enabled: r.enabled,
            })
        })
        .collect()
}

/// One editable environment/global variable row.
pub(crate) struct EnvRow {
    pub(crate) enabled: bool,
    pub(crate) secret: bool,
    /// Secret values are masked unless the user clicked Show.
    pub(crate) revealed: bool,
    pub(crate) key: Entity<InputState>,
    pub(crate) value: Entity<InputState>,
}

/// Variables from the editor rows; rows without a key are skipped.
pub(crate) fn collect_env(rows: &[EnvRow], cx: &App) -> Vec<EnvVar> {
    rows.iter()
        .filter_map(|r| {
            let key = r.key.read(cx).value().trim().to_string();
            if key.is_empty() {
                return None;
            }
            Some(EnvVar {
                key,
                value: r.value.read(cx).value().to_string(),
                secret: r.secret,
                enabled: r.enabled,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{body_label, parse_timeout};
    use at_core::{Body, RequestSettings};

    #[test]
    fn editor_helpers() {
        assert_eq!(parse_timeout("5000", 1), 5000);
        assert_eq!(parse_timeout(" 0 ", 1), 0, "0 means no timeout");
        assert_eq!(parse_timeout("", 1), RequestSettings::default().timeout_ms);
        assert_eq!(
            parse_timeout("12s", 777),
            777,
            "half-typed keeps the last value"
        );
        assert_eq!(parse_timeout("-5", 777), 777);

        assert_eq!(body_label(BodyMode::None, RawKind::Json), "Body");
        assert_eq!(body_label(BodyMode::Raw, RawKind::Xml), "Body (XML)");
        assert_eq!(
            body_label(BodyMode::FormData, RawKind::Json),
            "Body (form-data)"
        );

        assert_eq!(
            RawKind::Html.body("<p/>".into()),
            Body::Html { raw: "<p/>".into() }
        );
        assert_eq!(RawKind::ALL.len(), 4);
    }
}
