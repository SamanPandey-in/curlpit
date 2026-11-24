//! The collection runner page's state (FR-RUN). The run order, the rows and the run loop
//! live in `at_runner`; `workspace::runner` starts runs and fills this in.

pub(crate) use at_runner::{RunResult, RunRow};
use gpui::Entity;
use gpui_component::input::InputState;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// The runner page's state.
pub(crate) struct Runner {
    /// Collection or folder being run: (display name, directory).
    pub(crate) target: Option<(String, PathBuf)>,
    pub(crate) iterations: Entity<InputState>,
    pub(crate) delay: Entity<InputState>,
    pub(crate) stop_on_fail: bool,
    pub(crate) rows: Vec<RunRow>,
    pub(crate) planned: usize,
    pub(crate) cancel: Option<CancellationToken>,
    pub(crate) started: Option<Instant>,
    pub(crate) elapsed: Option<Duration>,
    pub(crate) notes: Vec<String>,
    pub(crate) copied: bool,
}

impl Runner {
    /// (requests run, requests failed, tests passed, tests failed)
    pub(crate) fn counts(&self) -> (usize, usize, usize, usize) {
        at_runner::counts(&self.rows)
    }

    /// Plain-text report for the clipboard.
    pub(crate) fn summary_text(&self) -> String {
        let name = self.target.as_ref().map(|(n, _)| n.as_str()).unwrap_or("");
        at_runner::summary_text(name, &self.rows)
    }
}

#[cfg(test)]
mod tests {
    use crate::format::parse_count;

    #[test]
    fn runner_count_options_are_clamped() {
        assert_eq!(parse_count("3", 1, 1, 100), 3);
        assert_eq!(parse_count("", 1, 1, 100), 1);
        assert_eq!(parse_count("x", 1, 1, 100), 1);
        assert_eq!(parse_count("0", 1, 1, 100), 1, "clamped up");
        assert_eq!(parse_count("5000", 1, 1, 100), 100, "clamped down");
    }
}
