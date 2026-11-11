//! Postman import and export (FR-IMP): choosing what to export, detecting what was
//! imported, writing the file, and the page bookkeeping around it. `leave_env_page` lives
//! here because every one of these flows has to get off the environments page first.

use crate::format::{notes, unique_name};
use crate::sidebar::{ItemKind, MainView, Scope, SideView};
use crate::ui::Status;
use crate::workspace::{merge_vars, Workspace};
use at_interop::{
    export_collection, export_environment, import_postman, read_collection, write_collection,
    Imported,
};
use at_store::slug;
use gpui::{Context, PathPromptOptions, Window};
use std::path::PathBuf;

impl Workspace {
    /// The collection the user is working in: the one holding the active tab's file, or the
    /// collection/folder last clicked in the sidebar.
    pub(crate) fn selected_collection(&self) -> Option<(String, PathBuf)> {
        let tab = self.tab();
        let here = tab.current_path.as_ref().or(tab.save_dir.as_ref())?;
        self.items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Collection))
            .find(|i| here.starts_with(&i.path))
            .map(|i| (i.label.clone(), i.path.clone()))
    }

    /// Where the save dialog starts: ~/Downloads if there is one, else the home folder.
    pub(crate) fn export_dir(&self) -> PathBuf {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        match home {
            Some(h) if h.join("Downloads").is_dir() => h.join("Downloads"),
            Some(h) => h,
            None => self
                .store
                .as_ref()
                .map(|s| s.data_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    pub(crate) fn on_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.into_iter().next() {
                    this.update_in(cx, |this, window, cx| this.import_file(&path, window, cx))
                        .ok();
                }
            }
        })
        .detach();
    }

    /// Import a Postman collection, environment or globals file (the kind is detected).
    pub(crate) fn import_file(
        &mut self,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let imported = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| import_postman(&t).map_err(|e| e.to_string()));
        let imported = match imported {
            Ok(i) => i,
            Err(e) => {
                self.notice(Status::Error(format!(
                    "Could not import {}: {e}",
                    path.display()
                )));
                cx.notify();
                return;
            }
        };
        let Some(store) = self.store.clone() else {
            self.notice(Status::Error(
                "Storage is unavailable on this system".into(),
            ));
            cx.notify();
            return;
        };
        match imported {
            Imported::Collection(col, warnings) => match write_collection(&store, &col) {
                Ok((_, sum)) => {
                    self.refresh_sidebar();
                    self.side_view = SideView::Collections;
                    self.notice(Status::Info(format!(
                        "Imported \"{}\": {} request(s), {} folder(s){}",
                        col.name,
                        sum.requests,
                        sum.folders,
                        notes(&warnings)
                    )));
                }
                Err(e) => self.notice(Status::Error(e.to_string())),
            },
            Imported::Environment(mut env, warnings) => {
                // Environment indices shift when one is added; save edits first, then reload.
                if self.main_view == MainView::Environments
                    && self.env_has_changes(cx)
                    && !self.save_env_scope(cx)
                {
                    cx.notify();
                    return;
                }
                env.name = unique_name(&env.name, |n| {
                    self.envs
                        .iter()
                        .any(|(e, _)| e.name.eq_ignore_ascii_case(n))
                });
                match store.save_environment(&env, None) {
                    Ok(_) => {
                        self.reload_envs();
                        if self.main_view == MainView::Environments {
                            self.load_scope(Scope::Globals, window, cx);
                        }
                        self.notice(Status::Info(format!(
                            "Imported environment \"{}\" with {} variable(s){}",
                            env.name,
                            env.variables.len(),
                            notes(&warnings)
                        )));
                    }
                    Err(e) => self.notice(Status::Error(e.to_string())),
                }
            }
            Imported::Globals(vars, warnings) => {
                if self.main_view == MainView::Environments
                    && self.env_has_changes(cx)
                    && !self.save_env_scope(cx)
                {
                    cx.notify();
                    return;
                }
                let (merged, replaced) = merge_vars(&self.globals, vars);
                let total = merged.len();
                match store.save_globals(&merged) {
                    Ok(()) => {
                        self.reload_envs();
                        if self.main_view == MainView::Environments {
                            self.load_scope(Scope::Globals, window, cx);
                        }
                        self.notice(Status::Info(format!(
                            "Imported global variables: {total} in total, {replaced} replaced{}",
                            notes(&warnings)
                        )));
                    }
                    Err(e) => self.notice(Status::Error(e.to_string())),
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn on_export_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((name, dir)) = self.selected_collection() else {
            self.notice(Status::Error(
                "Click a collection in the sidebar (or open one of its requests) to choose what to export"
                    .into(),
            ));
            cx.notify();
            return;
        };
        let (col, mut warnings) = match read_collection(&name, &dir) {
            Ok(x) => x,
            Err(e) => {
                self.notice(Status::Error(e.to_string()));
                cx.notify();
                return;
            }
        };
        let (text, more) = export_collection(&col);
        warnings.extend(more);
        let file = format!("{}.postman_collection.json", slug(&name));
        self.save_export(
            text,
            file,
            format!("Exported collection \"{name}\""),
            warnings,
            window,
            cx,
        );
    }

    pub(crate) fn on_export_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Export what's on screen: save pending edits first.
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        let (name, vars, globals) = match self.env_scope {
            Scope::Globals => ("Globals".to_string(), self.globals.clone(), true),
            Scope::Env(ix) => match self.envs.get(ix) {
                Some((e, _)) => (e.name.clone(), e.variables.clone(), false),
                None => return,
            },
        };
        let text = export_environment(&name, &vars, self.export_secrets, globals);
        let secrets = vars.iter().filter(|v| v.secret).count();
        let file = if globals {
            "globals.postman_globals.json".to_string()
        } else {
            format!("{}.postman_environment.json", slug(&name))
        };
        let mut warnings = Vec::new();
        if secrets > 0 && !self.export_secrets {
            warnings.push(format!(
                "{secrets} secret value(s) left empty (turn on \"Include secrets\" to export them)"
            ));
        }
        self.save_export(
            text,
            file,
            format!("Exported \"{name}\""),
            warnings,
            window,
            cx,
        );
    }

    /// Ask where to save, then write the file and report.
    pub(crate) fn save_export(
        &mut self,
        text: String,
        file_name: String,
        done: String,
        warnings: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_new_path(&self.export_dir(), Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let written = std::fs::write(&path, text);
            this.update(cx, |this, cx| {
                match written {
                    Ok(()) => this.notice(Status::Info(format!(
                        "{done} to {}{}",
                        path.display(),
                        notes(&warnings)
                    ))),
                    Err(e) => this.notice(Status::Error(format!(
                        "Could not write {}: {e}",
                        path.display()
                    ))),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Leave the environments page (autosaving it). False if the save failed and we stayed.
    pub(crate) fn leave_env_page(&mut self, cx: &mut Context<Self>) -> bool {
        match self.main_view {
            MainView::Environments => self.close_envs(cx),
            // the runner keeps running in the background; its page can be reopened
            MainView::Runner | MainView::Cookies => self.main_view = MainView::Request,
            MainView::Request => {}
        }
        self.main_view == MainView::Request
    }
}
