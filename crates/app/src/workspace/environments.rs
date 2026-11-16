//! Environments and global variables (FR-ENV-01..05): which variables are in effect,
//! loading a scope into the editor, saving it back, and creating, activating or deleting
//! an environment. Switching scope always autosaves, so edits are never lost.

use crate::editor::{collect_env, EnvRow};
use crate::sidebar::{MainView, Scope};
use crate::ui::Status;
use crate::workspace::Workspace;
use at_core::{to_map, EnvVar, Environment, Variables};
use at_store::Settings;
use gpui::prelude::*;
use gpui::{App, Context, Window};
use gpui_component::input::InputState;

impl Workspace {
    /// Variables in effect: globals, overridden by the active environment.
    pub(crate) fn variables(&self) -> Variables {
        let environment = self
            .active_env
            .and_then(|i| self.envs.get(i))
            .map(|(e, _)| to_map(&e.variables))
            .unwrap_or_default();
        Variables {
            global: to_map(&self.globals),
            environment,
            ..Default::default()
        }
    }

    pub(crate) fn reload_envs(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.load_globals() {
            Ok(g) => self.globals = g,
            Err(e) => self.status = Status::Error(format!("Global variables not loaded: {e}")),
        }
        self.envs = store
            .list_environments()
            .into_iter()
            .map(|e| (e.env, e.path))
            .collect();
        let want = store.load_settings().active_environment;
        self.active_env = want.and_then(|n| self.envs.iter().position(|(e, _)| e.name == n));
    }

    pub(crate) fn persist_active(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        let settings = Settings {
            active_environment: self
                .active_env
                .and_then(|i| self.envs.get(i))
                .map(|(e, _)| e.name.clone()),
            ..Default::default()
        };
        if let Err(e) = store.save_settings(&settings) {
            self.status = Status::Error(e.to_string());
        }
    }

    /// Top-bar picker: none -> first environment -> ... -> none.
    pub(crate) fn cycle_env(&mut self, cx: &mut Context<Self>) {
        self.active_env = match self.active_env {
            None if !self.envs.is_empty() => Some(0),
            Some(i) if i + 1 < self.envs.len() => Some(i + 1),
            _ => None,
        };
        self.persist_active();
        cx.notify();
    }

    pub(crate) fn new_env_row(window: &mut Window, cx: &mut Context<Self>, v: &EnvVar) -> EnvRow {
        let key = cx.new(|cx| InputState::new(window, cx).default_value(v.key.clone()));
        let secret = v.secret;
        let value = cx.new(|cx| {
            let s = InputState::new(window, cx).default_value(v.value.clone());
            if secret {
                s.masked(true)
            } else {
                s
            }
        });
        EnvRow {
            enabled: v.enabled,
            secret,
            revealed: false,
            key,
            value,
        }
    }

    /// Fill the editor with the saved variables of `scope` (plus one spare empty row).
    pub(crate) fn load_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let (name, vars) = match scope {
            Scope::Globals => (String::new(), self.globals.clone()),
            Scope::Env(ix) => match self.envs.get(ix) {
                Some((e, _)) => (e.name.clone(), e.variables.clone()),
                None => (String::new(), Vec::new()),
            },
        };
        let mut rows: Vec<EnvRow> = vars
            .iter()
            .map(|v| Self::new_env_row(window, cx, v))
            .collect();
        rows.push(Self::new_env_row(window, cx, &EnvVar::new("", "")));
        self.env_rows = rows;
        self.env_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.env_scope = scope;
        self.confirm_delete = false;
    }

    pub(crate) fn env_has_changes(&self, cx: &App) -> bool {
        let vars = collect_env(&self.env_rows, cx);
        match self.env_scope {
            Scope::Globals => vars != self.globals,
            Scope::Env(ix) => match self.envs.get(ix) {
                Some((e, _)) => {
                    vars != e.variables || self.env_name.read(cx).value().trim() != e.name
                }
                None => false,
            },
        }
    }

    /// Save what the editor shows. Returns false (with the reason in the status) on failure.
    pub(crate) fn save_env_scope(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(store) = self.store.clone() else {
            self.status = Status::Error("Storage is unavailable on this system".into());
            return false;
        };
        let vars = collect_env(&self.env_rows, cx);
        match self.env_scope {
            Scope::Globals => match store.save_globals(&vars) {
                Ok(()) => {
                    self.globals = vars;
                    self.status = Status::Info("Global variables saved".into());
                    true
                }
                Err(e) => {
                    self.status = Status::Error(e.to_string());
                    false
                }
            },
            Scope::Env(ix) => {
                let name = self.env_name.read(cx).value().trim().to_string();
                if name.is_empty() {
                    self.status = Status::Error("Give the environment a name".into());
                    return false;
                }
                if self
                    .envs
                    .iter()
                    .enumerate()
                    .any(|(i, (e, _))| i != ix && e.name.eq_ignore_ascii_case(&name))
                {
                    self.status =
                        Status::Error(format!("An environment named \"{name}\" already exists"));
                    return false;
                }
                let old = self.envs.get(ix).map(|(_, p)| p.clone());
                let env = Environment {
                    name,
                    variables: vars,
                };
                match store.save_environment(&env, old.as_deref()) {
                    Ok(path) => {
                        if ix < self.envs.len() {
                            self.envs[ix] = (env, path);
                        }
                        self.persist_active();
                        self.status = Status::Info("Environment saved".into());
                        true
                    }
                    Err(e) => {
                        self.status = Status::Error(e.to_string());
                        false
                    }
                }
            }
        }
    }

    pub(crate) fn select_scope(
        &mut self,
        scope: Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.env_scope == scope {
            return;
        }
        // Switching never loses edits: save first; if that fails, stay where we are.
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        self.load_scope(scope, window, cx);
        cx.notify();
    }

    pub(crate) fn open_envs(&mut self, cx: &mut Context<Self>) {
        self.main_view = MainView::Environments;
        cx.notify();
    }

    pub(crate) fn close_envs(&mut self, cx: &mut Context<Self>) {
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        self.main_view = MainView::Request;
        cx.notify();
    }

    pub(crate) fn on_env_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.env_has_changes(cx) && !self.save_env_scope(cx) {
            cx.notify();
            return;
        }
        let Some(store) = self.store.clone() else {
            self.status = Status::Error("Storage is unavailable on this system".into());
            cx.notify();
            return;
        };
        let mut n = self.envs.len() + 1;
        let name = loop {
            let cand = format!("Environment {n}");
            if !self.envs.iter().any(|(e, _)| e.name == cand) {
                break cand;
            }
            n += 1;
        };
        let env = Environment {
            name,
            variables: Vec::new(),
        };
        match store.save_environment(&env, None) {
            Ok(path) => {
                self.envs.push((env, path));
                let ix = self.envs.len() - 1;
                self.load_scope(Scope::Env(ix), window, cx);
                self.status =
                    Status::Info("Environment created; rename it and add variables".into());
            }
            Err(e) => self.status = Status::Error(e.to_string()),
        }
        cx.notify();
    }

    pub(crate) fn on_env_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Scope::Env(ix) = self.env_scope else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let Some((_, path)) = self.envs.get(ix).cloned() else {
            return;
        };
        match store.delete_environment(&path) {
            Ok(()) => {
                self.envs.remove(ix);
                self.active_env = match self.active_env {
                    Some(a) if a == ix => None,
                    Some(a) if a > ix => Some(a - 1),
                    other => other,
                };
                self.persist_active();
                self.load_scope(Scope::Globals, window, cx);
                self.status = Status::Info("Environment deleted".into());
            }
            Err(e) => self.status = Status::Error(e.to_string()),
        }
        cx.notify();
    }
}
