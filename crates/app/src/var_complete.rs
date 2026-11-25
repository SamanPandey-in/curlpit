//! `{{variable}}` assistance in text inputs (D21's "later", G-13): what each `{{…}}` token
//! refers to, the completion list `{{` opens, and the gpui-component hooks that draw them.
//!
//! The logic is pure and unit-tested (`catalog`, `completions`, `open_token`, `tokens`); the
//! rest plugs it into gpui-component 0.5's `InputState::lsp` providers, which work on plain
//! single-line inputs too:
//! - **highlighting** is a `DocumentColorProvider`: each token gets a tinted box painted behind
//!   its text (accent = resolved, error = unresolved, muted = `$dynamic`). The input exposes no
//!   way to recolour the text itself outside code-editor mode, and a code editor brings line
//!   numbers, multi-line behaviour and a tree-sitter grammar the URL field doesn't want.
//! - **autocomplete** is a `CompletionProvider` driving the input's own completion menu (up/down,
//!   Enter, Esc built in; Tab is added by [`AcceptVarCompletion`] in [`var_input`]).
//! - **hover** shows the token's value and scope (secrets masked).
//!
//! The variables in effect are published as a gpui global, [`VarCatalog`], by the workspace on
//! every render, so the providers need no handle back to the workspace.

use crate::actions::AcceptVarCompletion;
use crate::theme::Palette;
use at_core::{EnvVar, DYNAMIC_VARIABLES};
use gpui::prelude::*;
use gpui::{
    div, App, Context, Div, Entity, EntityId, EntityInputHandler, Global, Hsla, Rgba, Task,
};
use gpui::{Result, Window};
use gpui_component::input::{
    CompletionProvider, DocumentColorProvider, Enter, HoverProvider, InputState,
};
use gpui_component::{Rope, RopeExt as _};
use lsp_types::{
    Color, ColorInformation, CompletionContext, CompletionItem, CompletionResponse,
    CompletionTextEdit, Hover, HoverContents, MarkupContent, MarkupKind, TextEdit,
};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

/// Where a variable's value comes from. The order is the completion ranking: the scope that
/// wins at send time comes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum VarScope {
    Environment,
    /// Collection variables exist in `at_core::Variables` but are not wired to the UI yet
    /// (D25); the label is ready for when they are.
    #[allow(dead_code)]
    Collection,
    Global,
    Dynamic,
}

impl VarScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            VarScope::Environment => "environment",
            VarScope::Collection => "collection",
            VarScope::Global => "global",
            VarScope::Dynamic => "dynamic",
        }
    }
}

/// One variable in effect. For `Dynamic` entries `value` is the description of the format.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VarEntry {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) secret: bool,
    pub(crate) scope: VarScope,
}

/// The variables in effect, one entry per name: enabled environment variables override
/// enabled globals (D25), and the `$dynamic` names fill in for anything not defined by the
/// user (a user variable named `$uuid` shadows the dynamic one, as in `at_core::Variables`).
pub(crate) fn catalog(globals: &[EnvVar], env: &[EnvVar]) -> Vec<VarEntry> {
    let mut out: Vec<VarEntry> = Vec::new();
    let mut put = |v: &EnvVar, scope: VarScope| {
        if !v.enabled || v.key.trim().is_empty() {
            return;
        }
        let e = VarEntry {
            name: v.key.clone(),
            value: v.value.clone(),
            secret: v.secret,
            scope,
        };
        match out.iter_mut().find(|o| o.name == v.key) {
            Some(o) => *o = e,
            None => out.push(e),
        }
    };
    for v in globals {
        put(v, VarScope::Global);
    }
    for v in env {
        put(v, VarScope::Environment);
    }
    for (name, desc) in DYNAMIC_VARIABLES {
        if !out.iter().any(|o| o.name == *name) {
            out.push(VarEntry {
                name: name.to_string(),
                value: desc.to_string(),
                secret: false,
                scope: VarScope::Dynamic,
            });
        }
    }
    out
}

/// Longest value shown in the completion list or a hover before it is cut with `…`.
const VALUE_MAX: usize = 40;

/// The value as the UI may show it: secrets masked, long values cut, empty spelled out.
pub(crate) fn display_value(e: &VarEntry) -> String {
    if e.secret {
        return "••••••".to_string();
    }
    if e.value.is_empty() {
        return "(empty)".to_string();
    }
    let flat = crate::ui::one_line(&e.value);
    if flat.chars().count() > VALUE_MAX {
        let cut: String = flat.chars().take(VALUE_MAX - 1).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// One row of the completion list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) name: String,
    /// Current value (masked for secrets) or, for a dynamic name, its format.
    pub(crate) value: String,
    pub(crate) scope: VarScope,
    /// `name` starts with the typed prefix exactly, so the prefix can be highlighted.
    pub(crate) prefix_match: bool,
}

/// Most rows the completion list shows.
const MAX_ITEMS: usize = 50;

/// The completion list for what was typed after `{{`. Case-insensitive; the leading `$` of a
/// dynamic name is optional (`uuid` finds `$uuid`). Ranked exact match, then prefix, then
/// substring; within a rank by scope (environment first, dynamic last), then by name.
pub(crate) fn completions(prefix: &str, vars: &[VarEntry]) -> Vec<Item> {
    let typed = prefix.trim().to_lowercase();
    let mut ranked: Vec<(u8, &VarEntry)> = vars
        .iter()
        .filter_map(|e| {
            let full = e.name.to_lowercase();
            let bare = full.trim_start_matches('$');
            let rank = if typed.is_empty() {
                1
            } else if full == typed || bare == typed {
                0
            } else if full.starts_with(&typed) || bare.starts_with(&typed) {
                1
            } else if full.contains(&typed) {
                2
            } else {
                return None;
            };
            Some((rank, e))
        })
        .collect();
    ranked.sort_by(|(ra, a), (rb, b)| {
        ra.cmp(rb)
            .then(a.scope.cmp(&b.scope))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let typed_raw = prefix.trim();
    ranked
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(_, e)| Item {
            name: e.name.clone(),
            value: display_value(e),
            scope: e.scope,
            prefix_match: !typed_raw.is_empty() && e.name.starts_with(typed_raw),
        })
        .collect()
}

/// If the text before the cursor ends inside an open `{{…` (no `}}` yet, no spaces after the
/// name started), the byte offset of that `{{` and the name typed so far.
pub(crate) fn open_token(before: &str) -> Option<(usize, &str)> {
    let start = before.rfind("{{")?;
    let typed = before[start + 2..].trim_start();
    let ok = typed
        .chars()
        .all(|c| !c.is_whitespace() && c != '{' && c != '}');
    ok.then_some((start, typed))
}

/// How a `{{…}}` token is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Resolved,
    Unresolved,
    Dynamic,
}

/// A `{{name}}` token in `text`: its byte range (braces included), the trimmed name, and its
/// kind. Scanning matches `at_core::Variables::resolve`, so what is coloured is what is sent.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub(crate) range: Range<usize>,
    pub(crate) name: String,
    pub(crate) kind: TokenKind,
}

pub(crate) fn tokens(text: &str, vars: &[VarEntry]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut base = 0;
    while let Some(open) = text[base..].find("{{") {
        let start = base + open;
        let Some(close) = text[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + close + 2;
        let name = text[start + 2..end - 2].trim();
        if !name.is_empty() {
            let kind = match vars.iter().find(|e| e.name == name) {
                Some(e) if e.scope == VarScope::Dynamic => TokenKind::Dynamic,
                Some(_) => TokenKind::Resolved,
                None => TokenKind::Unresolved,
            };
            out.push(Token {
                range: start..end,
                name: name.to_string(),
                kind,
            });
        }
        base = end;
    }
    out
}

// ---------------------------------------------------------------------------------------
// gpui side
// ---------------------------------------------------------------------------------------

/// The variables in effect, published by the workspace for the input providers. `gen` changes
/// whenever they do, so inputs know their highlighting is stale.
#[derive(Default)]
pub(crate) struct VarCatalog {
    entries: Vec<VarEntry>,
    gen: u64,
    /// Inputs whose highlighting was recomputed for `gen`.
    fresh: HashMap<EntityId, u64>,
}

impl Global for VarCatalog {}

fn entries(cx: &App) -> &[VarEntry] {
    cx.try_global::<VarCatalog>()
        .map(|c| c.entries.as_slice())
        .unwrap_or_default()
}

/// Publish the variables in effect (cheap when nothing changed).
pub(crate) fn sync(new: Vec<VarEntry>, cx: &mut App) {
    if !cx.has_global::<VarCatalog>() {
        cx.set_global(VarCatalog::default());
    }
    let c = cx.global_mut::<VarCatalog>();
    if c.entries != new {
        c.entries = new;
        c.gen += 1;
        c.fresh.clear();
    }
}

/// Give `input` variable highlighting, hover and `{{` completion (idempotent).
pub(crate) fn attach(input: &Entity<InputState>, cx: &mut App) {
    if input.read(cx).lsp.completion_provider.is_some() {
        return;
    }
    input.update(cx, |s, _| {
        let p = Rc::new(VarProvider);
        s.lsp.completion_provider = Some(p.clone());
        s.lsp.hover_provider = Some(p.clone());
        s.lsp.document_color_provider = Some(p);
    });
}

/// Recompute `input`'s highlighting if the variables changed since it was last coloured.
///
/// gpui-component recolours only when the text changes, so this makes an empty edit at the
/// cursor. Only inputs that contain a `{{` and are on screen pay for it, once per change of the
/// variables. (Side effect: one no-op step in that input's undo history.)
pub(crate) fn refresh(input: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    let id = input.entity_id();
    let Some(gen) = cx.try_global::<VarCatalog>().map(|c| c.gen) else {
        return;
    };
    if cx.global::<VarCatalog>().fresh.get(&id) == Some(&gen) {
        return;
    }
    cx.global_mut::<VarCatalog>().fresh.insert(id, gen);
    let text = input.read(cx).value();
    if !text.contains("{{") {
        return;
    }
    input.update(cx, |s, cx| {
        let at = s.cursor().min(text.len());
        if !text.is_char_boundary(at) {
            return;
        }
        let u16 = text[..at].encode_utf16().count();
        s.replace_text_in_range(Some(u16..u16), "", window, cx);
    });
}

/// Wrap an input so Tab accepts the open completion (Up/Down/Enter/Esc are the menu's own).
/// When no menu is open, Tab carries on to whatever else is bound to it.
pub(crate) fn var_input(input: &Entity<InputState>, child: impl IntoElement) -> Div {
    let input = input.clone();
    div()
        .w_full()
        .key_context("VarInput")
        .on_action(move |_: &AcceptVarCompletion, window, cx| {
            let took = input.update(cx, |s, cx| {
                s.handle_action_for_context_menu(Box::new(Enter { secondary: false }), window, cx)
            });
            if took {
                cx.stop_propagation();
            } else {
                cx.propagate();
            }
        })
        .child(child)
}

/// The one provider behind all three hooks; it reads [`VarCatalog`] when asked.
struct VarProvider;

fn lsp_range(text: &Rope, r: &Range<usize>) -> lsp_types::Range {
    lsp_types::Range {
        start: text.offset_to_position(r.start),
        end: text.offset_to_position(r.end),
    }
}

fn lsp_color(c: Hsla) -> Color {
    let c: Rgba = c.into();
    Color {
        red: c.r,
        green: c.g,
        blue: c.b,
        alpha: c.a,
    }
}

impl CompletionProvider for VarProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>> {
        let s = text.to_string();
        let off = offset.min(s.len());
        let none = || Task::ready(Ok(CompletionResponse::Array(vec![])));
        if !s.is_char_boundary(off) {
            return none();
        }
        let Some((start, typed)) = open_token(&s[..off]) else {
            return none();
        };
        // Typing inside an existing `{{…}}` replaces its closing braces too.
        let end = if s[off..].starts_with("}}") {
            off + 2
        } else {
            off
        };
        let range = lsp_range(text, &(start..end));
        let items = completions(typed, entries(cx))
            .into_iter()
            .map(|it| CompletionItem {
                // The menu highlights `filter_text.len()` bytes of the label as the match.
                filter_text: Some(if it.prefix_match {
                    typed.to_string()
                } else {
                    String::new()
                }),
                detail: Some(format!("{}  ·  {}", it.value, it.scope.label())),
                // gpui-component lays the menu out as a row beside this panel when there is room
                // to the right, and the list then shrinks to its 120 px minimum, cutting the
                // detail. The panel is a fixed 320 px, so the full value and scope stay readable.
                documentation: Some(lsp_types::Documentation::String(format!(
                    "{} = {}  ({})",
                    it.name,
                    it.value,
                    it.scope.label()
                ))),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: format!("{{{{{}}}}}", it.name),
                })),
                label: it.name,
                ..Default::default()
            })
            .collect();
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    /// Asked on every edit; `completions` decides from the text whether the cursor is inside
    /// an open `{{`, and an empty answer closes the menu.
    fn is_completion_trigger(&self, _: usize, _: &str, _: &mut Context<InputState>) -> bool {
        true
    }
}

/// What hovering a token says.
pub(crate) fn hover_text(token: &Token, vars: &[VarEntry]) -> String {
    let entry = vars.iter().find(|e| e.name == token.name);
    match (token.kind, entry) {
        (TokenKind::Dynamic, Some(e)) => format!(
            "`{}` · dynamic: {}, generated fresh on every send",
            e.name, e.value
        ),
        (TokenKind::Resolved, Some(e)) => format!(
            "`{}` = `{}` · {}",
            e.name,
            display_value(e),
            e.scope.label()
        ),
        _ => format!(
            "`{}` is not defined in any scope. Use **+ {}** under the URL bar to create it.",
            token.name, token.name
        ),
    }
}

impl HoverProvider for VarProvider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<Hover>>> {
        let s = text.to_string();
        let vars = entries(cx);
        let hover = tokens(&s, vars)
            .into_iter()
            .find(|t| t.range.contains(&offset))
            .map(|t| Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: hover_text(&t, vars),
                }),
                range: Some(lsp_range(text, &t.range)),
            });
        Task::ready(Ok(hover))
    }
}

impl DocumentColorProvider for VarProvider {
    fn document_colors(
        &self,
        text: &Rope,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<ColorInformation>>> {
        let p = Palette::of(cx);
        let s = text.to_string();
        let colors = tokens(&s, entries(cx))
            .into_iter()
            .map(|t| ColorInformation {
                range: lsp_range(text, &t.range),
                color: lsp_color(match t.kind {
                    TokenKind::Resolved => p.text_accent().alpha(0.22),
                    TokenKind::Unresolved => p.status_error().alpha(0.30),
                    TokenKind::Dynamic => p.text_muted().alpha(0.22),
                }),
            })
            .collect();
        Task::ready(Ok(colors))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(k: &str, v: &str) -> EnvVar {
        EnvVar::new(k, v)
    }

    fn sample() -> Vec<VarEntry> {
        let mut token = var("token", "s3cr3t-value");
        token.secret = true;
        let mut off = var("disabled", "x");
        off.enabled = false;
        let globals = vec![var("host", "global.test"), var("hostname", "h"), off];
        let env = vec![var("host", "env.test"), token, var("userId", "42")];
        catalog(&globals, &env)
    }

    fn names(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn catalog_applies_precedence_and_skips_disabled() {
        let c = sample();
        let host = c.iter().find(|e| e.name == "host").unwrap();
        assert_eq!(host.value, "env.test");
        assert_eq!(host.scope, VarScope::Environment);
        assert!(c.iter().all(|e| e.name != "disabled"));
        assert_eq!(c.iter().filter(|e| e.name == "host").count(), 1);
        assert!(c
            .iter()
            .any(|e| e.name == "$uuid" && e.scope == VarScope::Dynamic));
    }

    #[test]
    fn user_variable_shadows_dynamic_entry() {
        let c = catalog(&[var("$uuid", "fixed")], &[]);
        let e: Vec<_> = c.iter().filter(|e| e.name == "$uuid").collect();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].scope, VarScope::Global);
        assert_eq!(e[0].value, "fixed");
    }

    #[test]
    fn empty_prefix_lists_everything_user_scopes_first() {
        let items = completions("", &sample());
        assert_eq!(items.len(), 4 + DYNAMIC_VARIABLES.len());
        // environment, then global, then dynamic
        assert_eq!(
            &names(&items)[..4],
            &["host", "token", "userId", "hostname"]
        );
        assert!(items[4..].iter().all(|i| i.scope == VarScope::Dynamic));
    }

    #[test]
    fn ranking_exact_then_prefix_then_substring() {
        let c = catalog(
            &[var("host", "a"), var("hostname", "b"), var("api_host", "c")],
            &[],
        );
        assert_eq!(
            names(&completions("host", &c)),
            ["host", "hostname", "api_host"]
        );
        // case-insensitive
        assert_eq!(names(&completions("HOST", &c))[0], "host");
        // substring matches include dynamic names, but user variables rank first
        assert_eq!(
            names(&completions("NAME", &c)),
            ["hostname", "$randomFirstName", "$randomLastName"]
        );
    }

    #[test]
    fn filtering_narrows_as_you_type() {
        let c = sample();
        assert_eq!(names(&completions("us", &c)), ["userId"]);
        assert!(completions("zzz", &c).is_empty());
        // `$` is optional for dynamic names; typing it works too
        assert_eq!(names(&completions("uuid", &c))[0], "$uuid");
        assert_eq!(names(&completions("$uuid", &c))[0], "$uuid");
        let rnd = completions("$random", &c);
        assert_eq!(rnd.len(), 9);
        assert!(rnd.iter().all(|i| i.name.starts_with("$random")));
    }

    #[test]
    fn secrets_are_masked_and_long_values_cut() {
        let items = completions("tok", &sample());
        assert_eq!(items[0].value, "••••••");
        assert!(!items[0].value.contains("s3cr3t"));
        let long = "x".repeat(200);
        let c = catalog(&[var("big", &long), var("blank", "")], &[]);
        let big = &completions("big", &c)[0];
        assert_eq!(big.value.chars().count(), VALUE_MAX);
        assert!(big.value.ends_with('…'));
        assert_eq!(completions("blank", &c)[0].value, "(empty)");
    }

    #[test]
    fn scope_labels_and_prefix_flag() {
        let items = completions("ho", &sample());
        assert_eq!(items[0].scope.label(), "environment");
        assert!(items[0].prefix_match);
        let dynamic = &completions("guid", &sample())[0];
        assert_eq!(dynamic.scope.label(), "dynamic");
        assert_eq!(dynamic.value, "a v4 UUID");
        // `guid` is a prefix of `$guid` only after dropping `$`: no exact-prefix highlight
        assert!(!dynamic.prefix_match);
    }

    #[test]
    fn open_token_detection() {
        assert_eq!(open_token("https://{{"), Some((8, "")));
        assert_eq!(open_token("https://{{ho"), Some((8, "ho")));
        assert_eq!(open_token("{{ ho"), Some((0, "ho")));
        assert_eq!(open_token("{{a}}/{{b"), Some((6, "b")));
        assert_eq!(open_token("{{a}}/"), None);
        assert_eq!(open_token("{{a b"), None);
        assert_eq!(open_token("{{a}"), None);
        assert_eq!(open_token("no braces"), None);
        assert_eq!(open_token("é{{$u"), Some((2, "$u")));
    }

    #[test]
    fn tokens_classify_like_resolve() {
        let c = sample();
        let t = tokens("{{host}}/{{ $uuid }}/{{nope}}/{{$foo}}/{{}}/{{open", &c);
        let got: Vec<(&str, TokenKind)> = t.iter().map(|t| (t.name.as_str(), t.kind)).collect();
        assert_eq!(
            got,
            [
                ("host", TokenKind::Resolved),
                ("$uuid", TokenKind::Dynamic),
                ("nope", TokenKind::Unresolved),
                ("$foo", TokenKind::Unresolved),
            ]
        );
        assert_eq!(t[0].range, 0..8);
        assert_eq!(t[1].range, 9..20);
    }

    #[test]
    fn hover_masks_secrets() {
        let c = sample();
        let t = &tokens("{{token}}", &c)[0];
        let h = hover_text(t, &c);
        assert!(h.contains("••••••") && !h.contains("s3cr3t"), "{h}");
        let t = &tokens("{{missing}}", &c)[0];
        assert!(hover_text(t, &c).contains("not defined"));
    }
}
