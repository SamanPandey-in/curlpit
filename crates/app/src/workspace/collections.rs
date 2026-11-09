//! The collection tree on disk: rebuilding the sidebar from the store, what a click on a
//! row does, opening a saved request into a tab, and saving the active tab back to a file.

use crate::sidebar::{push_nodes, ItemKind, MainView, SidebarItem};
use crate::ui::Status;
use crate::workspace::Workspace;
use at_store::Store;
use gpui::{Context, Window};
use std::path::PathBuf;

impl Workspace {
    pub(crate) fn refresh_sidebar(&mut self) {
        self.items.clear();
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.load_all() {
            Ok(trees) => {
                for t in &trees {
                    self.items.push(SidebarItem {
                        depth: 0,
                        label: t.name.clone(),
                        kind: ItemKind::Collection,
                        path: t.dir.clone(),
                    });
                    push_nodes(&mut self.items, &t.children, 1);
                }
            }
            Err(e) => self.tab_mut().status = Status::Error(e.to_string()),
        }
    }

    pub(crate) fn on_sidebar_click(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.items.get(ix).cloned() else {
            return;
        };
        match item.kind {
            ItemKind::Collection | ItemKind::Folder => {
                let tab = self.tab_mut();
                tab.save_dir = Some(item.path);
                tab.current_path = None;
                tab.status = Status::Info("New requests will be saved here".into());
            }
            ItemKind::Broken(e) => self.tab_mut().status = Status::Error(e),
            ItemKind::Request => self.open_request(item.path, window, cx),
        }
        cx.notify();
    }

    /// Open a saved request: focus its tab if it is already open, reuse an untouched blank tab,
    /// otherwise open a new tab.
    pub(crate) fn open_request(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.leave_env_page(cx) {
            return;
        }
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| t.current_path.as_ref() == Some(&path))
        {
            self.activate_tab(ix, window, cx);
            return;
        }
        let req = match Store::load_request(&path) {
            Ok(r) => r,
            Err(e) => {
                self.tab_mut().status = Status::Error(e.to_string());
                return;
            }
        };
        if !self.tab().is_pristine(cx) {
            self.add_tab(window, cx);
        }
        let tab = self.tab_mut();
        tab.apply_request(req, window, cx);
        tab.save_dir = path.parent().map(|d| d.to_path_buf());
        tab.current_path = Some(path);
        tab.confirm_close = false;
        tab.status = Status::Info("Opened".into());
    }

    pub(crate) fn on_new_collection(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let n = self
            .items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Collection))
            .count()
            + 1;
        match store.create_collection(&format!("Collection {n}")) {
            Ok(c) => {
                self.refresh_sidebar();
                let tab = self.tab_mut();
                tab.save_dir = Some(c.dir);
                tab.current_path = None;
                tab.status = Status::Info("Collection created; Save puts requests in it".into());
            }
            Err(e) => self.tab_mut().status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    pub(crate) fn on_save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // On the environments page, Cmd/Ctrl+S saves the variables being edited.
        if self.main_view == MainView::Environments {
            self.save_env_scope(cx);
            cx.notify();
            return;
        }
        let Some(store) = self.store.clone() else {
            self.tab_mut().status = Status::Error("Storage is unavailable on this system".into());
            cx.notify();
            return;
        };
        let mut req = self.tab().current_request(cx);
        if req.name.trim().is_empty() {
            req.name = "Untitled request".into();
        }
        let dir = match (&self.tab().current_path, &self.tab().save_dir) {
            (Some(p), _) => p.parent().map(|d| d.to_path_buf()),
            (None, Some(d)) => Some(d.clone()),
            _ => None,
        };
        let dir = match dir {
            Some(d) => d,
            None => match store.load_all() {
                Ok(all) if !all.is_empty() => all[0].dir.clone(),
                _ => match store.create_collection("My Collection") {
                    Ok(c) => c.dir,
                    Err(e) => {
                        self.tab_mut().status = Status::Error(e.to_string());
                        cx.notify();
                        return;
                    }
                },
            },
        };
        match Store::save_request(&dir, &req, self.tab().current_path.as_deref()) {
            Ok(path) => {
                let saved = self.tab().current_request(cx);
                let tab = self.tab_mut();
                tab.save_dir = path.parent().map(|d| d.to_path_buf());
                tab.status = Status::Info(format!("Saved to {}", path.display()));
                tab.current_path = Some(path);
                tab.saved = saved;
                tab.confirm_close = false;
                self.refresh_sidebar();
            }
            Err(e) => self.tab_mut().status = Status::Error(e.to_string()),
        }
        cx.notify();
    }
}
