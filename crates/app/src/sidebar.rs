//! What the left-hand pane and the main pane are currently showing: the flattened
//! collection tree the sidebar lists, plus the small enums that say which pane is on
//! screen and which variable scope the environments page is editing.

use at_store::Node;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) enum ItemKind {
    Collection,
    Folder,
    Request,
    Broken(String),
}

/// One row of the flattened sidebar tree.
#[derive(Clone)]
pub(crate) struct SidebarItem {
    pub(crate) depth: usize,
    pub(crate) label: String,
    pub(crate) kind: ItemKind,
    /// Collection dir, folder dir, or request file.
    pub(crate) path: PathBuf,
}

pub(crate) fn push_nodes(items: &mut Vec<SidebarItem>, nodes: &[Node], depth: usize) {
    for n in nodes {
        match n {
            Node::Folder {
                name,
                path,
                children,
            } => {
                items.push(SidebarItem {
                    depth,
                    label: name.clone(),
                    kind: ItemKind::Folder,
                    path: path.clone(),
                });
                push_nodes(items, children, depth + 1);
            }
            Node::Request { name, path } => items.push(SidebarItem {
                depth,
                label: name.clone(),
                kind: ItemKind::Request,
                path: path.clone(),
            }),
            Node::Broken { name, path, error } => items.push(SidebarItem {
                depth,
                label: name.clone(),
                kind: ItemKind::Broken(error.clone()),
                path: path.clone(),
            }),
        }
    }
}

/// What the sidebar lists.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SideView {
    Collections,
    History,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MainView {
    Request,
    Environments,
    Runner,
    Cookies,
}

/// What the environments page is editing.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Scope {
    Globals,
    Env(usize),
}
