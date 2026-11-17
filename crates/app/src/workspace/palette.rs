//! The command palette: opening it, filtering as you type, moving the selection and
//! running the chosen command (FR-KEY-01, FR-KEY-02). The command set and ranking live in
//! [`crate::commands`]; the drawing lives in [`super::render`]'s `palette` module.
use gpui::{Context, Window};

use crate::commands::{ranked, static_commands, Command, CommandKind};
use crate::sidebar::MainView;
use crate::workspace::Workspace;

/// Everything on screen while the palette is open. The query text itself lives in
/// `Workspace::palette_query` (an `InputState`, like the other search boxes).
pub(crate) struct PaletteState {
    pub(crate) commands: Vec<Command>,
    /// Indices into `commands`, best match first.
    pub(crate) order: Vec<usize>,
    /// Position within `order` of the highlighted row.
    pub(crate) selected: usize,
}

impl Workspace {
    pub(crate) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(cx);
        } else {
            self.open_palette(window, cx);
        }
    }

    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let commands = self.build_commands();
        let order = ranked(&commands, "");
        self.palette = Some(PaletteState {
            commands,
            order,
            selected: 0,
        });
        self.palette_query.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn close_palette(&mut self, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            cx.notify();
        }
    }

    /// The static commands plus one row per environment (to switch the active one).
    fn build_commands(&self) -> Vec<Command> {
        let mut v = static_commands();
        v.push(Command::new(
            "Environment: none",
            "Environment",
            CommandKind::Env(None),
        ));
        for (ix, (e, _)) in self.envs.iter().enumerate() {
            v.push(Command::new(
                format!("Environment: {}", e.name),
                "Environment",
                CommandKind::Env(Some(ix)),
            ));
        }
        v
    }

    /// Re-rank against the current query text; called on every keystroke.
    pub(crate) fn palette_filter(&mut self, cx: &mut Context<Self>) {
        let query = self.palette_query.read(cx).value().to_string();
        if let Some(p) = self.palette.as_mut() {
            p.order = ranked(&p.commands, &query);
            p.selected = 0;
            cx.notify();
        }
    }

    /// Move the highlighted row by `delta`, wrapping at both ends.
    pub(crate) fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(p) = self.palette.as_mut() {
            if p.order.is_empty() {
                return;
            }
            let n = p.order.len() as isize;
            p.selected = (((p.selected as isize + delta) % n + n) % n) as usize;
            cx.notify();
        }
    }

    /// Run the highlighted command (Enter).
    pub(crate) fn palette_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_selected(self.selected_pos(), window, cx);
    }

    /// Run the command on a clicked row.
    pub(crate) fn palette_click(
        &mut self,
        pos: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_selected(Some(pos), window, cx);
    }

    fn selected_pos(&self) -> Option<usize> {
        self.palette.as_ref().map(|p| p.selected)
    }

    fn run_selected(&mut self, pos: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let kind = pos.and_then(|pos| {
            self.palette
                .as_ref()
                .and_then(|p| p.order.get(pos).map(|&i| p.commands[i].kind.clone()))
        });
        self.close_palette(cx);
        if let Some(kind) = kind {
            self.run_command(kind, window, cx);
        }
    }

    fn run_command(&mut self, kind: CommandKind, window: &mut Window, cx: &mut Context<Self>) {
        match kind {
            CommandKind::Send => self.send_active(cx),
            CommandKind::Save => self.on_save(window, cx),
            CommandKind::NewTab => {
                if self.main_view == MainView::Request {
                    self.add_tab(window, cx);
                }
            }
            CommandKind::CloseTab => {
                if self.main_view == MainView::Request {
                    let ix = self.active;
                    self.close_tab(ix, window, cx);
                }
            }
            CommandKind::ReopenTab => self.reopen_tab(window, cx),
            CommandKind::Find => {
                if self.main_view == MainView::Request && self.tab().view.is_some() {
                    self.open_find(window, cx);
                }
            }
            CommandKind::Import => self.on_import(window, cx),
            CommandKind::ImportClipboard => self.import_from_clipboard(window, cx),
            CommandKind::ExportPostman => self.on_export_collection(window, cx),
            CommandKind::ExportJson => self.on_export_collection_json(window, cx),
            CommandKind::Go(MainView::Settings) => self.open_settings(window, cx),
            CommandKind::Go(view) => {
                self.main_view = view;
                cx.notify();
            }
            CommandKind::Side(view) => {
                self.side_view = view;
                cx.notify();
            }
            CommandKind::Theme(mode) => {
                crate::theme::apply(mode, cx);
                cx.notify();
            }
            CommandKind::ThemeSystem => {
                crate::theme::sync(window, cx);
                cx.notify();
            }
            CommandKind::Env(which) => {
                self.active_env = which;
                self.persist_active();
                cx.notify();
            }
        }
    }
}
