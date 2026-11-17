//! The command palette's command set and fuzzy ranking (FR-KEY-01, FR-KEY-02).
//!
//! The commands and the ranking are pure data and are unit-tested here. Opening the
//! palette, drawing it and running a chosen command live in `workspace::palette` and
//! `workspace::render::palette`.
use at_core::fuzzy_score;
use gpui_component::ThemeMode;

use crate::sidebar::{MainView, SideView};

/// What a chosen command does. Data only, so the whole list is cheap to rebuild on open.
#[derive(Clone)]
pub(crate) enum CommandKind {
    Send,
    Save,
    NewTab,
    CloseTab,
    ReopenTab,
    Find,
    Import,
    ImportClipboard,
    ExportPostman,
    ExportJson,
    Go(MainView),
    Side(SideView),
    Theme(ThemeMode),
    ThemeSystem,
    /// Switch the active environment, or `None` for no environment.
    Env(Option<usize>),
}

/// One row in the palette: what it says, how it is grouped, and what it does.
#[derive(Clone)]
pub(crate) struct Command {
    pub label: String,
    pub category: &'static str,
    pub kind: CommandKind,
}

impl Command {
    pub(crate) fn new(label: impl Into<String>, category: &'static str, kind: CommandKind) -> Self {
        Self {
            label: label.into(),
            category,
            kind,
        }
    }
}

/// Commands that are always available, independent of runtime state.
pub(crate) fn static_commands() -> Vec<Command> {
    use CommandKind::*;
    vec![
        Command::new("Send request", "Request", Send),
        Command::new("Save request", "Request", Save),
        Command::new("New tab", "Request", NewTab),
        Command::new("Close tab", "Request", CloseTab),
        Command::new("Reopen closed tab", "Request", ReopenTab),
        Command::new("Find in response", "Request", Find),
        Command::new("Go to Requests", "Navigate", Go(MainView::Request)),
        Command::new("Go to Environments", "Navigate", Go(MainView::Environments)),
        Command::new("Go to Runner", "Navigate", Go(MainView::Runner)),
        Command::new("Go to Cookies", "Navigate", Go(MainView::Cookies)),
        Command::new("Go to Settings", "Navigate", Go(MainView::Settings)),
        Command::new("Show Collections", "Navigate", Side(SideView::Collections)),
        Command::new("Show History", "Navigate", Side(SideView::History)),
        Command::new("Import file\u{2026}", "Data", Import),
        Command::new("Import from clipboard", "Data", ImportClipboard),
        Command::new(
            "Export collection as Postman\u{2026}",
            "Data",
            ExportPostman,
        ),
        Command::new("Export collection as JSON\u{2026}", "Data", ExportJson),
        Command::new("Theme: Light", "Theme", Theme(ThemeMode::Light)),
        Command::new("Theme: Dark", "Theme", Theme(ThemeMode::Dark)),
        Command::new("Theme: Follow system", "Theme", ThemeSystem),
    ]
}

/// Rank commands against `query` by the better of matching the label alone or the
/// "category label" pair, best first. An empty query keeps the given order; commands that
/// do not match at all are dropped.
pub(crate) fn ranked(commands: &[Command], query: &str) -> Vec<usize> {
    let q = query.trim();
    if q.is_empty() {
        return (0..commands.len()).collect();
    }
    let mut scored: Vec<(i32, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let on_label = fuzzy_score(q, &c.label);
            let on_category = fuzzy_score(q, &format!("{} {}", c.category, c.label));
            on_label.max(on_category).map(|s| (s, i))
        })
        .collect();
    // Best score first; ties keep the original order (stable by index).
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(cmds: &[Command], order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| cmds[i].label.clone()).collect()
    }

    #[test]
    fn empty_query_keeps_every_command_in_order() {
        let c = static_commands();
        assert_eq!(ranked(&c, ""), (0..c.len()).collect::<Vec<_>>());
        assert_eq!(ranked(&c, "   "), (0..c.len()).collect::<Vec<_>>());
    }

    #[test]
    fn an_exact_word_ranks_its_command_first() {
        let c = static_commands();
        assert_eq!(c[ranked(&c, "send")[0]].label, "Send request");
        assert_eq!(c[ranked(&c, "runner")[0]].label, "Go to Runner");
    }

    #[test]
    fn the_category_is_searchable_too() {
        // "theme dark" matches only through the category+label pairing.
        let c = static_commands();
        assert_eq!(c[ranked(&c, "theme dark")[0]].label, "Theme: Dark");
    }

    #[test]
    fn nonsense_matches_nothing() {
        let c = static_commands();
        assert!(ranked(&c, "zzzqqq").is_empty());
    }

    #[test]
    fn initials_find_a_command() {
        let c = static_commands();
        let got = labels(&c, &ranked(&c, "nt"));
        assert!(
            got.iter().any(|l| l == "New tab"),
            "nt should find New tab: {got:?}"
        );
    }
}
