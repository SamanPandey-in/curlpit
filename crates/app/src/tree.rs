//! The JSON tree view of a response (FR-RES-03): its fold state, the cached list of
//! visible rows, and how one row's value is rendered. Kept apart from the raw-bytes
//! viewer in `response`, because the two index the same body in different ways.

use crate::format::clip;
use at_core::json_tree::{CollapseState, JsonTree, Kind};
use std::sync::Arc;

/// What the tree's copy buttons put on the clipboard (FR-RES-03).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TreeCopy {
    Value,
    Path,
    All,
}

/// Containers deeper than this start folded, per FR-RES-03.
pub(crate) const TREE_DEFAULT_DEPTH: u16 = 2;

/// Longest scalar drawn on one tree row. The whole value is still what "Copy value" copies.
pub(crate) const TREE_VALUE_MAX: usize = 300;

/// Largest body the tree view indexes. The index is built in one pass on the UI thread, so
/// this stays well above the 10 MB the budget asks for (NFR-PERF-05) without risking a stall
/// on a body the Body view can still show.
pub(crate) const TREE_MAX_BYTES: usize = 16 * 1024 * 1024;

/// One tab's JSON tree of the response (FR-RES-03).
pub(crate) struct TreeView {
    /// Shared so the render closure clones a pointer, not 150k nodes.
    pub(crate) tree: Arc<JsonTree>,
    pub(crate) collapsed: CollapseState,
    /// The visible rows and whether each one is folded, cached so render never re-walks the
    /// whole tree. Rebuilt only when the fold state changes.
    pub(crate) rows: Arc<Vec<(u32, bool)>>,
    pub(crate) selected: Option<u32>,
}

impl TreeView {
    pub(crate) fn rebuild_rows(&mut self) {
        let collapsed = &self.collapsed;
        self.rows = Arc::new(
            self.tree
                .visible(collapsed)
                .into_iter()
                .map(|ix| (ix, collapsed.is_collapsed(ix)))
                .collect(),
        );
    }
}

/// What one tree row shows next to its key: a folded summary for containers, the clipped
/// literal for scalars (strings quoted, so they can't be read as numbers). A container must
/// never go through `value_text`, which returns its whole subtree -- that is for "Copy value",
/// one node at a time.
pub(crate) fn tree_value_display(t: &JsonTree, src: &[u8], ix: u32) -> String {
    let Some(node) = t.nodes().get(ix as usize) else {
        return String::new();
    };
    if node.kind.is_container() {
        return t.summary(ix);
    }
    let text = t.value_text(src, ix);
    if node.kind == Kind::String {
        format!("\"{}\"", clip(&text, TREE_VALUE_MAX))
    } else {
        clip(&text, TREE_VALUE_MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use at_core::json_tree::{JsonTree, Kind};

    #[test]
    fn tree_rows_show_summaries_scalars_and_clipped_strings() {
        let src = br#"{"n":1,"s":"x","b":true,"z":null,"arr":[1,2]}"#;
        let t = JsonTree::parse(src).expect("parses");

        // document order: 0 root, then one node per member
        assert_eq!(tree_value_display(&t, src, 1), "1");
        assert_eq!(
            tree_value_display(&t, src, 2),
            "\"x\"",
            "strings are quoted"
        );
        assert_eq!(tree_value_display(&t, src, 3), "true");
        assert_eq!(tree_value_display(&t, src, 4), "null");

        // a container shows its summary, never its whole subtree
        let root = tree_value_display(&t, src, 0);
        assert_eq!(root, t.summary(0));
        assert!(root.contains('5'), "summary counts the fields: {root}");
        let arr = t
            .nodes()
            .iter()
            .position(|n| n.kind == Kind::Array)
            .expect("the array");
        let arr_row = tree_value_display(&t, src, arr as u32);
        assert_eq!(arr_row, t.summary(arr as u32));
        assert_eq!(arr_row, "[2 items]");
        assert_ne!(arr_row, "[1,2]", "a summary, not the raw span");
        // out of range is empty, never a panic
        assert_eq!(tree_value_display(&t, src, 9_999), "");

        // long strings are clipped for the row; Copy value still has all of it
        let long = format!("{{\"k\":\"{}\"}}", "x".repeat(TREE_VALUE_MAX + 50));
        let lt = JsonTree::parse(long.as_bytes()).expect("parses");
        let shown = tree_value_display(&lt, long.as_bytes(), 1);
        // the ellipsis sits inside the closing quote
        assert!(shown.starts_with('"'), "quoted: {}", &shown[..4]);
        assert!(
            shown.ends_with("\u{2026}\""),
            "clipped inside the quotes: {}",
            &shown[shown.len() - 6..]
        );
        assert_eq!(
            shown.chars().count(),
            TREE_VALUE_MAX + 3,
            "two quotes and the ellipsis"
        );
        assert_eq!(
            lt.value_text(long.as_bytes(), 1).chars().count(),
            TREE_VALUE_MAX + 50
        );
    }
}
