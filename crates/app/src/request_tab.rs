//! One open request: its editor widgets, where it is saved, what the last response was,
//! and the Find state over that response. Tabs are independent, so everything that is
//! per-request lives here rather than on `Workspace`.

use crate::editor::{
    collect, collect_form, AuthKind, BodyMode, FormRow, KvRow, RawKind, ReqTab, Which,
};
use crate::format::{human, parse_timeout, tab_title};
use crate::response::{
    content_type, display_body, find_cache_stale, ResTab, ViewData, FIND_MAX, VIEW_MAX_BYTES,
};
use crate::script::ScriptReport;
use crate::tree::TreeView;
use crate::ui::{char_w, Status};
use crate::workspace::Workspace;
use at_core::lines::{count_chars, find_all};
use at_core::{ApiKeyLocation, Auth, Body, FormField, FormValue, Method, Request, RequestSettings};
use at_http::{HttpError, ResponseBody};
use gpui::prelude::*;
use gpui::{App, Context, Entity, ScrollStrategy, Subscription, UniformListScrollHandle, Window};
use gpui_component::input::{InputEvent, InputState};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

/// A closed tab, kept so it can be reopened with its edits.
pub(crate) struct ClosedTab {
    pub(crate) path: Option<PathBuf>,
    pub(crate) save_dir: Option<PathBuf>,
    pub(crate) draft: Request,
    pub(crate) saved: Request,
}

/// Everything that belongs to one open request: its editor, where it is saved, and the last
/// response. Tabs are independent, so a request can be in flight in one while you edit another.
pub(crate) struct RequestTab {
    /// Stable identity (indices shift when tabs close); in-flight results find their tab by it.
    pub(crate) id: u64,
    pub(crate) name: Entity<InputState>,
    pub(crate) method: Method,
    pub(crate) url: Entity<InputState>,
    pub(crate) body: Entity<InputState>,
    pub(crate) params: Vec<KvRow>,
    pub(crate) headers: Vec<KvRow>,
    pub(crate) req_tab: ReqTab,
    // ---- body editor ----
    pub(crate) body_mode: BodyMode,
    pub(crate) raw_kind: RawKind,
    pub(crate) form: Vec<KvRow>,
    pub(crate) multipart: Vec<FormRow>,
    // ---- auth editor (one input per field, so switching type keeps what was typed) ----
    pub(crate) auth_kind: AuthKind,
    pub(crate) auth_user: Entity<InputState>,
    pub(crate) auth_pass: Entity<InputState>,
    pub(crate) auth_token: Entity<InputState>,
    pub(crate) auth_key: Entity<InputState>,
    pub(crate) auth_value: Entity<InputState>,
    pub(crate) auth_in_query: bool,
    /// Secret auth fields are shown in clear text.
    pub(crate) auth_reveal: bool,
    // ---- settings ----
    pub(crate) timeout: Entity<InputState>,
    pub(crate) follow_redirects: bool,
    pub(crate) verify_tls: bool,
    // ---- scripts ----
    pub(crate) pre_script: Entity<InputState>,
    pub(crate) test_script: Entity<InputState>,
    /// The Scripts tab shows the test script (else the pre-request one).
    pub(crate) scripts_show_tests: bool,
    pub(crate) report: Option<ScriptReport>,
    /// The file this tab is saved to, if any.
    pub(crate) current_path: Option<PathBuf>,
    /// Folder the next Save goes to when the tab has no file yet.
    pub(crate) save_dir: Option<PathBuf>,
    /// The editor contents as of the last open/save; the tab is "unsaved" when they differ.
    pub(crate) saved: Request,
    /// The request as last loaded; supplies the id and any auth type without an editor yet.
    pub(crate) base: Request,
    /// The close button was clicked once on an unsaved tab; a second click discards it.
    pub(crate) confirm_close: bool,
    pub(crate) status: Status,
    pub(crate) view: Option<ViewData>,
    pub(crate) cancel: Option<CancellationToken>,
    pub(crate) res_tab: ResTab,
    pub(crate) resp_headers: Vec<(String, String)>,
    pub(crate) wrap: bool,
    /// Horizontal scroll of the response text in px (0 = left edge). The gutter never moves.
    pub(crate) scroll_x: f32,
    /// Shown next to the status, e.g. unresolved `{{variables}}` in the last request.
    pub(crate) warning: Option<String>,
    pub(crate) copied: bool,
    // ---- big bodies and Find ----
    /// The spilled file holding the full body, when it was too big for memory.
    pub(crate) body_file: Option<PathBuf>,
    /// A spilled body waiting to be loaded into the viewer.
    pub(crate) loading_file: Option<PathBuf>,
    /// Bumped per response, so a slow background load never replaces a newer response.
    pub(crate) view_gen: u64,
    /// Bumped every time `view` is replaced -- which happens more often than `view_gen`
    /// changes, because a spilled body first shows a "Loading..." placeholder and is then
    /// swapped for the real bytes inside the same response. Caches derived from the shown
    /// body (Find matches, the JSON tree) key on this, never on `view_gen` or on the
    /// buffer's address.
    pub(crate) view_epoch: u64,
    pub(crate) list_scroll: UniformListScrollHandle,
    /// The response as a JSON tree (FR-RES-03). `None` until the Tree view is first opened;
    /// `Err` holds why this body has no tree. `tree_gen` says which response it belongs to.
    pub(crate) tree: Option<Result<TreeView, String>>,
    pub(crate) tree_gen: u64,
    pub(crate) tree_scroll: UniformListScrollHandle,
    pub(crate) find: Entity<InputState>,
    pub(crate) find_open: bool,
    /// Byte offsets of matches in the viewer's data.
    pub(crate) matches: Vec<usize>,
    pub(crate) match_ix: usize,
    /// (view epoch, query) the matches belong to.
    pub(crate) matches_key: (u64, String),
    pub(crate) scroll_to_match: bool,
    pub(crate) _sub: Subscription,
    pub(crate) _find_sub: Subscription,
}

impl RequestTab {
    /// A new tab. `blank` gives an empty request; otherwise the httpbin starter example.
    pub(crate) fn create(
        id: u64,
        blank: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> RequestTab {
        let (url_text, body_text, params) = if blank {
            ("", "", vec![Workspace::new_row(window, cx, "", "")])
        } else {
            (
                "https://httpbin.org/get",
                "{\n  \"hello\": \"world\"\n}",
                vec![Workspace::new_row(window, cx, "hello", "world")],
            )
        };
        let url = cx.new(|cx| InputState::new(window, cx).default_value(url_text));
        let body = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(true)
                .default_value(body_text)
        });
        let name = cx.new(|cx| InputState::new(window, cx).default_value("New request"));
        let headers = vec![Workspace::new_row(window, cx, "", "")];
        let form = vec![Workspace::new_row(window, cx, "", "")];
        let multipart = vec![Workspace::new_form_row(
            window,
            cx,
            &FormField {
                key: String::new(),
                value: FormValue::Text {
                    value: String::new(),
                },
                enabled: true,
            },
        )];
        let field = |window: &mut Window, cx: &mut Context<Workspace>, hint: &str, secret: bool| {
            let hint = hint.to_string();
            cx.new(|cx| {
                let s = InputState::new(window, cx).placeholder(hint);
                if secret {
                    s.masked(true)
                } else {
                    s
                }
            })
        };
        let auth_user = field(window, cx, "Username", false);
        let auth_pass = field(window, cx, "Password", true);
        let auth_token = field(window, cx, "Token", true);
        let auth_key = field(
            window,
            cx,
            "Header or query parameter name, e.g. X-API-Key",
            false,
        );
        let auth_value = field(window, cx, "Value", true);
        let timeout = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("30000")
                .default_value(RequestSettings::default().timeout_ms.to_string())
        });
        let script_editor = |window: &mut Window, cx: &mut Context<Workspace>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("javascript")
                    .line_number(true)
            })
        };
        let pre_script = script_editor(window, cx);
        let test_script = script_editor(window, cx);
        let find = cx.new(|cx| InputState::new(window, cx).placeholder("Find in response"));
        // Enter: next match; the secondary Enter (Cmd/Ctrl+Enter): previous.
        let find_sub = cx.subscribe_in(
            &find,
            window,
            move |this, _state, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { secondary } = event {
                    this.find_step(id, if *secondary { -1 } else { 1 }, cx);
                }
            },
        );
        // Enter in the URL box sends (but never cancels an in-flight request).
        let sub = cx.subscribe_in(
            &url,
            window,
            move |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && this.tabs.iter().any(|t| t.id == id && t.cancel.is_none())
                {
                    this.send_tab(id, cx);
                }
            },
        );
        let mut tab = RequestTab {
            id,
            name,
            method: Method::Get,
            url,
            body,
            params,
            headers,
            req_tab: ReqTab::Params,
            body_mode: BodyMode::Raw,
            raw_kind: RawKind::Json,
            form,
            multipart,
            auth_kind: AuthKind::None,
            auth_user,
            auth_pass,
            auth_token,
            auth_key,
            auth_value,
            auth_in_query: false,
            auth_reveal: false,
            timeout,
            follow_redirects: true,
            verify_tls: true,
            pre_script,
            test_script,
            scripts_show_tests: false,
            report: None,
            current_path: None,
            save_dir: None,
            saved: Request::default(),
            base: Request::default(),
            confirm_close: false,
            status: Status::Idle,
            view: None,
            cancel: None,
            res_tab: ResTab::Body,
            resp_headers: Vec::new(),
            wrap: false,
            scroll_x: 0.0,
            warning: None,
            copied: false,
            body_file: None,
            loading_file: None,
            view_gen: 0,
            view_epoch: 0,
            list_scroll: UniformListScrollHandle::new(),
            tree: None,
            tree_gen: 0,
            tree_scroll: UniformListScrollHandle::new(),
            find,
            find_open: false,
            matches: Vec::new(),
            match_ix: 0,
            matches_key: (0, String::new()),
            scroll_to_match: false,
            _sub: sub,
            _find_sub: find_sub,
        };
        tab.saved = tab.current_request(cx);
        tab
    }

    pub(crate) fn rows_mut(&mut self, which: Which) -> &mut Vec<KvRow> {
        match which {
            Which::Params => &mut self.params,
            Which::Headers => &mut self.headers,
            Which::Form => &mut self.form,
        }
    }

    pub(crate) fn text(input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().to_string()
    }

    /// The request as currently shown in the editor.
    pub(crate) fn current_request(&self, cx: &App) -> Request {
        let auth = match self.auth_kind {
            AuthKind::None => Auth::None,
            AuthKind::Basic => Auth::Basic {
                username: Self::text(&self.auth_user, cx),
                password: Self::text(&self.auth_pass, cx),
            },
            AuthKind::Bearer => Auth::Bearer {
                token: Self::text(&self.auth_token, cx),
            },
            AuthKind::ApiKey => Auth::ApiKey {
                key: Self::text(&self.auth_key, cx),
                value: Self::text(&self.auth_value, cx),
                location: if self.auth_in_query {
                    ApiKeyLocation::Query
                } else {
                    ApiKeyLocation::Header
                },
            },
            AuthKind::Kept => self.base.auth.clone(),
        };
        let body = match self.body_mode {
            BodyMode::None => Body::None,
            BodyMode::Raw => {
                let text = Self::text(&self.body, cx);
                if text.trim().is_empty() {
                    Body::None
                } else {
                    self.raw_kind.body(text)
                }
            }
            BodyMode::UrlEncoded => Body::UrlEncoded {
                fields: collect(&self.form, cx),
            },
            BodyMode::FormData => Body::FormData {
                fields: collect_form(&self.multipart, cx),
            },
        };
        Request {
            id: self.base.id.clone(),
            name: Self::text(&self.name, cx),
            method: self.method,
            url: Self::text(&self.url, cx).trim().to_string(),
            pre_request_script: Self::text(&self.pre_script, cx),
            test_script: Self::text(&self.test_script, cx),
            params: collect(&self.params, cx),
            headers: collect(&self.headers, cx),
            body,
            auth,
            settings: RequestSettings {
                timeout_ms: parse_timeout(
                    &Self::text(&self.timeout, cx),
                    self.base.settings.timeout_ms,
                ),
                follow_redirects: self.follow_redirects,
                verify_tls: self.verify_tls,
            },
        }
    }

    /// Describes an auth type that has no editor yet (kept as loaded).
    pub(crate) fn kept_auth(&self) -> Option<&'static str> {
        match (&self.auth_kind, &self.base.auth) {
            (AuthKind::Kept, Auth::Jwt { .. }) => Some("JWT"),
            (AuthKind::Kept, Auth::Custom { .. }) => Some("custom auth headers"),
            _ => None,
        }
    }

    pub(crate) fn set_auth_masked(&self, window: &mut Window, cx: &mut Context<Workspace>) {
        let masked = !self.auth_reveal;
        for f in [&self.auth_pass, &self.auth_token, &self.auth_value] {
            f.update(cx, |s, cx| s.set_masked(masked, window, cx));
        }
    }

    pub(crate) fn is_dirty(&self, cx: &App) -> bool {
        self.current_request(cx) != self.saved
    }

    /// Never edited, saved or sent: safe to reuse for opening a request from the sidebar.
    pub(crate) fn is_pristine(&self, cx: &App) -> bool {
        self.current_path.is_none() && self.view.is_none() && !self.is_dirty(cx)
    }

    /// Text for the tab button (without the unsaved marker).
    pub(crate) fn title(&self, cx: &App) -> String {
        tab_title(self.name.read(cx).value().as_ref())
    }

    /// Put a loaded request into the editor and remember it as the saved state.
    pub(crate) fn apply_request(
        &mut self,
        req: Request,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        self.base = req.clone();
        self.method = req.method;
        let url = req.url.clone();
        self.url.update(cx, |s, cx| s.set_value(url, window, cx));
        let name = req.name.clone();
        self.name.update(cx, |s, cx| s.set_value(name, window, cx));
        // body: mode, raw text, urlencoded rows, form-data rows
        let (mode, kind, body_text) = match &req.body {
            Body::None => (BodyMode::Raw, RawKind::Json, String::new()),
            Body::Json { raw } => (BodyMode::Raw, RawKind::Json, raw.clone()),
            Body::Xml { raw } => (BodyMode::Raw, RawKind::Xml, raw.clone()),
            Body::Html { raw } => (BodyMode::Raw, RawKind::Html, raw.clone()),
            Body::Text { raw } => (BodyMode::Raw, RawKind::Text, raw.clone()),
            Body::UrlEncoded { .. } => (BodyMode::UrlEncoded, RawKind::Json, String::new()),
            Body::FormData { .. } => (BodyMode::FormData, RawKind::Json, String::new()),
        };
        self.body_mode = mode;
        self.raw_kind = kind;
        self.body
            .update(cx, |s, cx| s.set_value(body_text, window, cx));
        let mut form: Vec<KvRow> = match &req.body {
            Body::UrlEncoded { fields } => fields
                .iter()
                .map(|kv| {
                    let mut r = Workspace::new_row(window, cx, &kv.key, &kv.value);
                    r.enabled = kv.enabled;
                    r
                })
                .collect(),
            _ => Vec::new(),
        };
        form.push(Workspace::new_row(window, cx, "", ""));
        let blank = FormField {
            key: String::new(),
            value: FormValue::Text {
                value: String::new(),
            },
            enabled: true,
        };
        let mut multipart: Vec<FormRow> = match &req.body {
            Body::FormData { fields } => fields
                .iter()
                .map(|f| Workspace::new_form_row(window, cx, f))
                .collect(),
            _ => Vec::new(),
        };
        multipart.push(Workspace::new_form_row(window, cx, &blank));
        self.form = form;
        self.multipart = multipart;

        // auth: fill the matching fields, clear the rest
        let (kind, user, pass, token, key, value, in_query) = match &req.auth {
            Auth::None => (AuthKind::None, "", "", "", "", "", false),
            Auth::Basic { username, password } => (
                AuthKind::Basic,
                username.as_str(),
                password.as_str(),
                "",
                "",
                "",
                false,
            ),
            Auth::Bearer { token } => (AuthKind::Bearer, "", "", token.as_str(), "", "", false),
            Auth::ApiKey {
                key,
                value,
                location,
            } => (
                AuthKind::ApiKey,
                "",
                "",
                "",
                key.as_str(),
                value.as_str(),
                *location == ApiKeyLocation::Query,
            ),
            Auth::Jwt { .. } | Auth::Custom { .. } => (AuthKind::Kept, "", "", "", "", "", false),
        };
        self.auth_kind = kind;
        self.auth_in_query = in_query;
        self.auth_reveal = false;
        for (input, v) in [
            (&self.auth_user, user),
            (&self.auth_pass, pass),
            (&self.auth_token, token),
            (&self.auth_key, key),
            (&self.auth_value, value),
        ] {
            let v = v.to_string();
            input.update(cx, |s, cx| s.set_value(v, window, cx));
        }
        self.set_auth_masked(window, cx);

        // settings
        let t = req.settings.timeout_ms.to_string();
        self.timeout.update(cx, |s, cx| s.set_value(t, window, cx));
        self.follow_redirects = req.settings.follow_redirects;
        self.verify_tls = req.settings.verify_tls;

        // scripts
        let (pre, test) = (req.pre_request_script.clone(), req.test_script.clone());
        self.pre_script
            .update(cx, |s, cx| s.set_value(pre, window, cx));
        self.test_script
            .update(cx, |s, cx| s.set_value(test, window, cx));
        let mut params: Vec<KvRow> = req
            .params
            .iter()
            .map(|kv| {
                let mut r = Workspace::new_row(window, cx, &kv.key, &kv.value);
                r.enabled = kv.enabled;
                r
            })
            .collect();
        if params.is_empty() {
            params.push(Workspace::new_row(window, cx, "", ""));
        }
        let mut headers: Vec<KvRow> = req
            .headers
            .iter()
            .map(|kv| {
                let mut r = Workspace::new_row(window, cx, &kv.key, &kv.value);
                r.enabled = kv.enabled;
                r
            })
            .collect();
        if headers.is_empty() {
            headers.push(Workspace::new_row(window, cx, "", ""));
        }
        self.params = params;
        self.headers = headers;
        self.saved = self.current_request(cx);
    }

    pub(crate) fn set_view(&mut self, data: Vec<u8>) {
        self.set_view_data(ViewData::new(data));
    }

    pub(crate) fn set_view_data(&mut self, v: ViewData) {
        self.view = Some(v);
        // the one place `view` is replaced, so the one place the epoch has to move
        self.view_epoch += 1;
        self.scroll_x = 0.0;
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    /// Recompute Find matches when the query or the shown body changed.
    pub(crate) fn refresh_matches(&mut self, cx: &App) {
        let query = self.find.read(cx).value().to_string();
        if !find_cache_stale(&self.matches_key, self.view_epoch, &query) {
            return;
        }
        self.matches = match &self.view {
            Some(v) if !query.is_empty() => find_all(&v.data, &query, FIND_MAX),
            _ => Vec::new(),
        };
        self.match_ix = 0;
        self.scroll_to_match = !self.matches.is_empty();
        self.matches_key = (self.view_epoch, query);
    }

    /// Bring the current match into view: scroll the list to its row and, without wrap, the
    /// text sideways so the match isn't off to the right.
    pub(crate) fn reveal_match(&mut self, avail: f32) {
        self.scroll_to_match = false;
        let (Some(v), Some(&off)) = (self.view.as_ref(), self.matches.get(self.match_ix)) else {
            return;
        };
        let rows = v.rows(self.wrap);
        let row = rows.row_of(off);
        self.list_scroll.scroll_to_item(row, ScrollStrategy::Center);
        if !self.wrap {
            let start = rows.starts[row] as usize;
            let x = count_chars(&v.data[start..off]) as f32 * char_w();
            if x < self.scroll_x || x > self.scroll_x + avail - 60.0 {
                self.scroll_x = (x - avail / 3.0).max(0.0);
            }
        }
    }

    pub(crate) fn max_scroll_x(&self, avail: f32) -> f32 {
        match &self.view {
            Some(v) => (v.longest_chars as f32 * char_w() + 24.0 - avail).max(0.0),
            None => 0.0,
        }
    }

    pub(crate) fn apply(&mut self, result: Result<at_http::HttpResponse, HttpError>) {
        match result {
            Ok(resp) => {
                let mut hs = resp.headers.clone();
                hs.sort_by(|a, b| a.0.cmp(&b.0));
                self.resp_headers = hs;
                let ct = content_type(&resp.headers);
                self.view_gen += 1;
                match resp.body {
                    ResponseBody::Memory(b) => {
                        self.body_file = None;
                        self.set_view(display_body(&b, &ct));
                    }
                    ResponseBody::File { path, len } => {
                        // Large bodies were streamed to disk; load them off the UI thread.
                        self.body_file = Some(path.clone());
                        if len <= VIEW_MAX_BYTES {
                            self.loading_file = Some(path);
                            self.set_view(
                                format!("Loading the {} response body…", human(len)).into_bytes(),
                            );
                        } else {
                            self.set_view(
                                format!(
                                    "The response body is {}, too large to show here.\nIt was saved to:\n{}\n\nUse Save… to keep a copy.",
                                    human(len),
                                    path.display()
                                )
                                .into_bytes(),
                            );
                        }
                    }
                }
                self.status = Status::Done {
                    code: resp.status,
                    text: resp.status_text,
                    ms: resp.elapsed.as_millis(),
                    size: resp.size,
                };
            }
            Err(HttpError::Cancelled) => self.status = Status::Error("Cancelled".into()),
            Err(e) => self.status = Status::Error(e.to_string()),
        }
    }
}
