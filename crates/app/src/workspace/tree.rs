//! Driving the response's JSON tree from the UI: indexing the body once per response,
//! folding and unfolding, and the three copy actions (FR-RES-03).

use crate::format::human;
use crate::response::ResTab;
use crate::tree::{TreeCopy, TreeView, TREE_DEFAULT_DEPTH, TREE_MAX_BYTES};
use crate::ui::Status;
use crate::workspace::Workspace;
use at_core::json_tree::{CollapseState, JsonTree};
use gpui::{ClipboardItem, Context};
use std::sync::Arc;

impl Workspace {
    /// Index the shown body as JSON the first time the Tree view needs it, once per response.
    /// Called from `render`, so a new response in an open Tree view reindexes by itself.
    pub(crate) fn ensure_tree(&mut self) {
        let tab = self.tab_mut();
        // The epoch, not `view_gen`: a spilled body is shown as a "Loading..." placeholder
        // first and swapped for the real bytes inside the same response, and the tree has to
        // be rebuilt when that happens or it would keep the placeholder's parse error.
        if tab.res_tab != ResTab::Tree || (tab.tree.is_some() && tab.tree_gen == tab.view_epoch) {
            return;
        }
        tab.tree_gen = tab.view_epoch;
        tab.tree = Some(match tab.view.as_ref() {
            None => Err("Send a request to see the response".to_string()),
            Some(v) if v.data.len() > TREE_MAX_BYTES => Err(format!(
                "This response is {}, too big for the tree view. The Body view still shows it.",
                human(v.data.len() as u64)
            )),
            Some(v) => match JsonTree::parse(&v.data[..]) {
                Ok(t) => {
                    let mut tv = TreeView {
                        collapsed: CollapseState::with_default_depth(&t, TREE_DEFAULT_DEPTH),
                        tree: Arc::new(t),
                        rows: Arc::new(Vec::new()),
                        selected: None,
                    };
                    tv.rebuild_rows();
                    Ok(tv)
                }
                Err(e) => Err(format!("This response can't be shown as a JSON tree: {e}")),
            },
        });
    }

    /// Clicking a row selects it; clicking a container also folds or unfolds it.
    pub(crate) fn on_tree_row(&mut self, ix: u32, cx: &mut Context<Self>) {
        if let Some(Ok(tv)) = self.tab_mut().tree.as_mut() {
            tv.selected = Some(ix);
            let container = tv
                .tree
                .nodes()
                .get(ix as usize)
                .map(|n| n.kind.is_container())
                .unwrap_or(false);
            if container {
                tv.collapsed.toggle(ix);
                tv.rebuild_rows();
            }
        }
        cx.notify();
    }

    pub(crate) fn tree_fold(&mut self, expand: bool, cx: &mut Context<Self>) {
        if let Some(Ok(tv)) = self.tab_mut().tree.as_mut() {
            if expand {
                tv.collapsed.expand_all();
            } else {
                tv.collapsed.collapse_all(&tv.tree);
            }
            tv.rebuild_rows();
        }
        cx.notify();
    }

    /// Copy value, copy JSON path, or copy the whole document (FR-RES-03).
    pub(crate) fn tree_copy(&mut self, what: TreeCopy, cx: &mut Context<Self>) {
        let picked = {
            let tab = self.tab();
            match (tab.tree.as_ref(), tab.view.as_ref()) {
                (Some(Ok(tv)), Some(v)) => {
                    let src = &v.data[..];
                    match what {
                        TreeCopy::All => Ok((tv.tree.value_text(src, 0), "Copied the whole JSON.")),
                        TreeCopy::Value => match tv.selected {
                            Some(ix) => Ok((tv.tree.value_text(src, ix), "Copied the value.")),
                            None => Err("Click a row in the tree first."),
                        },
                        TreeCopy::Path => match tv.selected {
                            Some(ix) => Ok((tv.tree.path(src, ix), "Copied the JSON path.")),
                            None => Err("Click a row in the tree first."),
                        },
                    }
                }
                _ => Err("There is no JSON tree to copy from."),
            }
        };
        match picked {
            Ok((text, msg)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.tab_mut().status = Status::Info(msg.to_string());
            }
            Err(msg) => self.tab_mut().status = Status::Error(msg.to_string()),
        }
        cx.notify();
    }
}
