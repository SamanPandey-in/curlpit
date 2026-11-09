# Technical decisions (made on your behalf, Rust-related)

Each is reversible; the cost of reversing is noted. Items marked **PROVISIONAL** get confirmed by a Phase 0 measurement.

| # | Decision | Why | Reversal cost |
|---|---|---|---|
| D1 | Rust **stable** toolchain via `rust-toolchain.toml`, edition 2021, MSRV 1.80 | Reproducible builds; avoids nightly breakage. If GPUI needs a newer compiler, bump MSRV only | Trivial |
| D2 | Crate names `at-core`, `at-http`, ... (not `core`) | A crate called `core` collides with Rust's built-in `core` | Rename |
| D3 | Core crates depend only on `serde` + `thiserror` | Keeps core tiny and fast to compile; encoders (base64, percent-encoding) are ~30 lines, hand-written and tested | Swap in crates later |
| D4 | HTTP: `reqwest` + **rustls** (no OpenSSL), HTTP/2, gzip/brotli/zstd | Same TLS stack on all 3 OSes, no system library headaches, easy single-binary distribution. Per-phase timings (FR-ENG-08) later via a custom `hyper` client | Medium, isolated in `at-http` |
| D5 | Async: `tokio` multi-thread for networking only; UI stays on GPUI's executor, bridged by channels | GPUI has its own executor; mixing prevents UI stalls | Low |
| D6 | Cancellation: `tokio_util::CancellationToken` + `select!` (drops the connection) | Immediate cancel (FR-ENG-03) | Low |
| D7 | Large bodies: stream chunks; spill to a temp file above 10 MB | Memory bound (NFR-PERF-05) | Low |
| D8 | Release profile: `opt-level=3`, fat LTO, 1 codegen unit, `panic=abort`, stripped | Speed first, size second; abort-on-panic shrinks binary. (`opt-level="s"` is an option if size gets tight) | Trivial |
| D9 | Collection file format: **TOML**, one file per request **(PROVISIONAL, final in Phase 3)** | Human-readable, diff-friendly. JSON bodies embed fine as TOML multi-line strings | Medium |
| D10 | Script engine: **`rquickjs` (QuickJS)** **(PROVISIONAL, spike in Phase 0)** | Smallest and fastest to start of the realistic options; real JS for Postman-script compatibility | Medium, isolated in `at-script` |
| D11 | History store: SQLite via `rusqlite` (bundled), WAL mode | Capped, queryable, crash-safe | Medium |
| D12 | Errors: `thiserror` in libraries; no `unwrap` in non-test code paths | Never panic the app (SRS 5.1) | n/a |
| D13 | Allocator: `mimalloc` on the app binary only, decided after measuring | Often lowers RSS and speeds allocs; verify, don't assume | Trivial |
| D14 | UI: **GPUI pinned to a git revision; `gpui-component` evaluated** **(PROVISIONAL, spikes A/C)** | Per the plan's go/no-go | Isolated in `app` crate |

## Update after Phase 1 verification (your machine: MacBook Air M5, macOS 26.6, rustc 1.99.1)
- D1 stays; MSRV 1.80 is conservative (your toolchain is far newer). Will raise MSRV only if GPUI demands it.
- D15: GPUI from **crates.io (`gpui = "0.2"`)** for the spike rather than a git pin — simplest start; switch to a pinned git revision if the crates.io release lacks something we need (per my search, a crates.io release exists; its freshness vs. Zed's main branch is unverified).
- D16: `rustfmt` default config; run `cargo fmt --all` before every commit; CI enforces it.

## D17: macOS builds use gpui's `runtime_shaders` (no full Xcode)
Decision (your request): do not require the full Xcode app. Zed's own contributor docs still list Xcode, but GPUI ships a `runtime_shaders` feature (Zed documented it as the workaround on macOS 26) that compiles Metal shaders at startup instead of build time, needing only Command Line Tools. End users never need Xcode either way.
Cost: shader compile at launch. Verified by the spike's first-render number; if it exceeds budget I'll look at caching the compiled library on first run.
Caveat: CI on macOS runners will use the same flag so CI and local builds match.

## Decisions confirmed by Phase 0 measurements (2026-10-25)
- **D10 final: QuickJS (`rquickjs`)** — sandbox containment verified (loop, memory bomb, recursion, no forbidden globals). Resolved to rquickjs 0.14 in your build.
- **D14 provisional GO on GPUI**: 347 ms first render with runtime shaders, 70 MB idle. Still open: binary size, resize feel, and the 10 MB virtualized viewer in a real window (spike 4).
- **D18: line-offset index design is validated**: raw bytes + u32 line index costs ~1.4x the text size; search is tens of ms. No DOM, no per-line allocations.

## Spike 4 result and ecosystem risk (2026-10-25)
- **D14 upgraded: GPUI is GO for the viewer** (10 MB: 84 MB RSS flat; 50 MB: 130 MB; smooth).
- **R1 (new risk): GPUI ecosystem churn.** crates.io `gpui 0.2.2` is an older snapshot. Newer GPUI distributions split into several crates (`gpui_platform` etc.) and `gpui-component` has moved to a new "GPUI Kit" 0.6 with a different editor API. We stay on gpui 0.2.x + gpui-component 0.5.x for now, keep all GPUI code in the `app` crate, and treat an upgrade as a scheduled task (plan section 7), not a surprise.

## Spike 5 result and Phase 2 choices (2026-10-25)
- **D19: adopt `gpui-component` 0.5 for inputs/editors/buttons.** Verified on your Mac: JSON highlighting, line numbers, select/copy/paste, undo/redo, no lag on a ~9,500-line (1 MB) document. Known gaps, tracked: no auto-close of brackets/quotes, no auto-indent, no key-repeat when holding a key (likely the macOS press-and-hold setting, test below), find does not jump to matches, no `{{variable}}` highlighting.
- **D20: threading.** tokio runtime with 2 worker threads for networking only; results return over an `async-channel` and are applied on the UI thread via `cx.spawn`. UI never blocks.
- **D21: GUI crate excluded from `default-members`** so CI and `cargo test` stay fast; it builds with `-p at-app`. A separate CI job will build it later.
- **Variable highlighting plan:** a plain input can't colour `{{x}}`. Short-term: unresolved-variable warning in the status line; later: custom URL field or highlight ranges via the editor's highlighter API.
- **Key repeat (macOS):** to confirm it's the press-and-hold popup, run `defaults write -g ApplePressAndHoldEnabled -bool false`, restart the spike and hold a key. Undo that test with `defaults delete -g ApplePressAndHoldEnabled`. The shipped app will set this for its own domain at startup.

## D22: wrapping uses monospace column math, not text measurement (2026-10-27)
The viewer must stay a fixed-row-height virtual list (that is what keeps 50 MB at ~130 MB RAM and smooth). So wrapping is done by splitting rows at N characters in a precomputed row index (`at_core::lines`), where N comes from window width / estimated character width. Each row stores its logical line number, with a continuation flag, so numbers never shift. Trade-off: wide glyphs (CJK/emoji) can overflow; if the width estimate is off on some OS, we measure the real glyph advance via GPUI's text system.
Sticky gutter: horizontal scroll is handled by the app (not the list), so the gutter is simply not shifted.

## D23: key/value tables are one `Input` pair per row (2026-10-27)
Each row owns two `InputState` entities (key, value) — simple and fully editable. At send time rows are read into `KeyValue`s (disabled/empty-key rows are ignored by `at_core::build`). Cost: 2 small entities per row; fine for tens of rows. If someone pastes 1,000 headers we'll switch to a virtualized table.
Tabs are plain buttons (primary = selected) rather than gpui-component's TabBar, to keep the API surface small until styling is a priority.

## D24: storage format and behaviour (2026-10-27)
- **TOML, one file per request** (resolves open question 4): readable, diff-friendly, hand-editable. Each file has `schema_version`; newer-than-supported files are refused (never overwritten), unreadable files appear as ⚠ entries.
- File name = slug of the request name; collisions get `-2`, `-3`; renaming moves the file. A request keeps no id in its filename, so git diffs stay clean.
- Atomic writes: temp file in the same directory, fsync, rename.
- Data dir: OS standard location via `directories`; `API_TESTER_DATA_DIR` overrides it (testing, portable use).
- Sidebar is a plain scrolling list for now; it becomes a virtualized list before the 5,000-request target (NFR-PERF-07).

## D25: environments, globals and settings (2026-10-29)
- **Files:** `<data-dir>/environments/<slug>.toml` (one per environment: `schema_version`, `name`, `[[variables]]` with `key`, `value`, `secret`, `enabled`), `<data-dir>/globals.toml`, `<data-dir>/settings.toml` (just `active_environment` for now). Same atomic-write and schema-version rules as requests. Unreadable or too-new environment files are skipped, never overwritten by accident; a corrupt `globals.toml` shows an error instead of silently loading as empty.
- **Precedence:** global < active environment (collection and runtime scopes exist in `Variables` but are not wired to the UI yet).
- **Secrets:** masked in the editor with a Show/Hide toggle, but stored as plain text in the TOML for now. OS keychain storage is FR-ENV-07 (P1). Export is not built yet; when it is, secrets are excluded by default (FR-ENV-04).
- **Editing model:** the page edits one scope at a time; switching scope, creating an environment, leaving the page, or Cmd/Ctrl+S saves first (and stays put if the save fails), so edits are never lost silently. Delete needs a second click.
- **URL scheme default:** `https://` is prepended only if the URL has no `://` *after* variable substitution, so `{{base}}/users` works when `base` contains a scheme.
- **Empty spare rows** in Params/Headers are no longer written into saved request files.
- **Copy response body** button (pretty-printed text) added to the response bar.

## D26: request tabs (2026-10-29)
- **State model:** everything that belongs to one request (editor inputs, saved path, status line, response viewer, wrap/scroll, in-flight cancel token, unresolved-variable warning) moved from `Workspace` into a `RequestTab`. `Workspace` keeps the tabs, the active index, the sidebar, storage and environments. Tabs are fully independent, so requests can run in several tabs at once.
- **Identity:** each tab has a stable `id`; a finished request finds its tab by id, not index, so closing or reordering tabs never misroutes a response. If the tab is gone the result is dropped (closing a tab also cancels its request).
- **Unsaved detection:** a snapshot of the editor (`current_request()`) is taken on open and on save; the tab is unsaved when the current editor differs. Comparing editor-to-editor (not file-to-editor) means fields the UI can't show yet (auth, settings) never cause false `●`. Cost: one `Request` build per tab per render, which is fine for tens of tabs; if bodies get huge we'll switch to an edit counter driven by input events.
- **Closing:** an unsaved tab needs a second click (`Discard?`), the same two-step pattern as deleting an environment (D25). There is always at least one tab.
- **Opening from the sidebar:** focus the tab already showing that file, else reuse an untouched tab (never edited, saved or sent), else open a new one. No duplicates of the same file.
- **Clicking a collection/folder** still re-targets the active tab's next Save into that folder as a new file (behaviour from 3a, S6), i.e. it works as "save as" for that tab.
- **Not persisted yet:** open tabs aren't restored after restart and there is no quit warning for unsaved tabs; both are queued for after history.
- **Shortcuts:** Cmd/Ctrl+T new tab, Cmd/Ctrl+W close tab. No app menu exists yet, so Cmd+W doesn't conflict with a window-close item.
- Removed the write-only `current_col` / `SidebarItem::col` fields.

## D27: request history (2026-10-29)
- **Crate:** `at-history` (in `default-members`, so plain `cargo test` covers it) wraps SQLite via `rusqlite` 0.41 with the `bundled` feature (D11): the same SQLite on every OS, no system library. File: `<data-dir>/history.db`, WAL journal, `synchronous=NORMAL`. Schema version in `PRAGMA user_version`; a newer file is refused and left untouched, and a corrupt file turns history off with a message (the app keeps working).
- **What's recorded:** every request that actually left the app, including network errors, timeouts and cancels. Build errors such as an empty URL aren't recorded. Stored: time, method, URL, name, the full request as JSON, status, status text, elapsed time, real body size, error, response headers, and the body.
- **Secrets:** the request is stored *as typed*, with `{{variables}}` unresolved, so environment secrets never reach the history file. Literal values typed directly into a header are stored, the same as in a saved request file.
- **Caps:** newest 1,000 entries (pruned in the same transaction as each insert). Up to 1 MB of body per entry, with a `body_truncated` flag. Spilled (>10 MB) bodies aren't kept. The app copies at most cap+1 bytes off the response, so big bodies aren't duplicated in memory just for history.
- **Threading:** inserts run on the tokio network thread right after the response arrives (behind an `Arc<Mutex<History>>`); the UI thread only runs the small list/get queries. The UI refreshes the list when a result comes in while History is shown.
- **Opening an entry** restores the request and the stored response into a tab (same reuse rules as opening a saved request, D26). The tab isn't linked to a file, so Save creates a new request. Truncated JSON is shown raw (pretty-printing half a document is unreliable).
- **Search** is a SQL `LIKE` on URL and name, with `% _ \` escaped so user text matches literally. The sidebar shows up to 500 matches.
- Response-body display logic (`display_body`, `content_type`) is now shared by live responses and history.

## D28: import/export (2026-10-29)
- **Format:** Postman Collection v2.1 for collections (v2.0 also imports), and Postman environment/globals files. They're the de-facto interchange format and every major tool reads them. Our own TOML folders stay the at-rest format (D24).
- **Crate:** `at-interop` (in `default-members`), depending on `at-core` and `at-store`. Parsing uses `serde_json::Value` with every field optional, because real exports vary (v2.0 auth objects vs v2.1 arrays, URL as string or object, BOMs). Anything not representable becomes a warning shown to the user, never a silent drop.
- **Mapping choices:** the URL query moves into Params (the raw URL loses its `?…`), so `build` doesn't append it twice. Folder/collection auth is flattened into each request on import (we have no inherited auth yet). On export only enabled params go into `url.raw`, and all params into `url.query` with `disabled`. JWT and custom auth export as plain headers. GraphQL imports as a JSON body. Per-request timeouts don't exist in Postman and are reported on export.
- **Import never merges:** a collection always becomes a new directory, and environments get a unique name (` 2`). Globals are the exception: they merge by key, since there is only one globals scope.
- **Secrets on export:** environment export leaves secret values empty by default and says how many were left out. "Include secrets" is an explicit per-page toggle (FR-ENV-04).
- **Folder names:** `Store::create_folder` keeps readable names (only OS-forbidden characters are replaced), because folders display their directory name.
- **Data preservation fix:** `RequestTab` keeps the loaded request as `base`. `current_request()` takes auth, settings, id and urlencoded/form-data bodies from it, so they survive open → save and are sent. Raw bodies keep their kind (XML stays XML). The UI shows a one-line "Also sent: …" note until editors for these exist.
- `Store::data_dir()` added; history now uses it (fixes `history.db` being created inside `collections/`).

## D29: code generation and cURL import (2026-10-29)
- **Crate:** `at-codegen` (in `default-members`), depends only on `at-core`. Generators take `Built`, the exact request the engine sends, so a snippet can never disagree with Send (params, auth headers, default Content-Type, multipart).
- **Languages:** cURL (POSIX single-quoting), Python `requests`, JavaScript `fetch` (`fs.openAsBlob` for files, Node 19.8+), Go `net/http` (imports only what's used, multipart via `mime/multipart`), Rust `reqwest::blocking` (raw strings with enough `#`, `\u{..}` for control characters). String literals for Python/JS/Go use JSON escaping, which is valid in all three.
- **Placeholders:** `at_core::build_template()` skips only the "URL must start with http(s)" check and turns `%7B%7B…%7D%7D` back into `{{…}}` in the URL, so "Variables: {{kept}}" snippets are readable. Never used for sending.
- **Secret warning:** in "values" mode the UI checks whether any enabled secret variable is referenced by the request and warns before copying/sharing.
- **cURL import:** a bash-like tokenizer (`'…'`, `"…"`, `$'…'`, backslash escapes, `\`+newline continuations incl. CRLF). The supported flags are listed in doc 12. Unknown flags become notes; known value-taking flags we ignore still consume their value, so it isn't mistaken for the URL. Plain `-d` data is kept as a text body with curl's default `application/x-www-form-urlencoded` header (bytes sent are identical; no double-encoding). The URL's query string is kept verbatim in the URL rather than split into Params, to preserve exact encoding; `-G` data does go into Params. A missing scheme gets `http://` (curl's own default).
- **Round trip:** a unit test generates cURL from built requests (JSON and multipart), parses it back, rebuilds, and checks method, URL, headers and body are identical.
- **Pasting:** "+ cURL" reads the clipboard (explicit, not paste-sniffing in the URL box, which would fire on every keystroke while typing `curl …`). The result opens as an unsaved tab (`●`), so closing it asks first.
- `sendable()` in the app now holds the "add https:// after substitution / drop body for GET" rules, shared by Send and Code.

## D30: auth, body and settings editors (2026-10-29)
- **Editor state per tab:** body mode (None/Raw/urlencoded/form-data) plus raw kind (JSON/XML/HTML/Text), urlencoded rows (`KvRow`, same as params), form-data rows (`FormRow`: key, Text|File, value/path), auth type plus one input per auth field, and timeout/redirect/TLS settings. `current_request()` now builds the whole `Request` from the editor. `base` only supplies the `id` and auth types without an editor (JWT, custom headers), shown as "From file".
- **Keep-what-you-typed:** each mode and auth type has its own inputs, so switching back and forth doesn't lose text. Only the selected mode/type is saved and sent.
- **Body None vs empty Raw:** both send no body. A file with no body reopens as Raw JSON with an empty editor, same as before.
- **Timeout input:** milliseconds, matching the file format. Blank means the default (30 s), `0` means none, and unparsable text keeps the last valid value, so typing never flips a request to "no timeout" by accident.
- **Masking:** password, token and API-key value are masked by default with one Show/Hide per tab. They're still stored in plain text in request files, like environment secrets (D25); keychain storage is FR-ENV-07.
- **File picker:** form-data "Choose…" uses the native dialog; the chosen path lands in the row of the tab that asked (found by tab id, in case tabs changed meanwhile).
- Supersedes the 3e "Also sent: …" note (`hidden_parts`), which is removed.
- Known gap: the raw editor's highlighting stays JSON for XML/HTML/Text; gpui-component's `code_editor` language is fixed at creation.

## D31: session restore / hot exit (2026-10-29)
- **Model:** like VS Code's hot exit. Open tabs, including unsaved drafts, are persisted to `<data-dir>/session.toml` (`Session { schema_version, active, tabs: [SessionTab { path, save_dir, unsaved, draft: Request }] }`, in `at-store`, same atomic writes and schema rules). Since quitting can't lose work, there is **no** "save changes?" prompt on quit. The two-click Discard on closing a single tab stays.
- **When written:** a 3 s timer (diffed against the last written session, so idle means no writes), `on_window_should_close`, and `on_app_quit`. Worst case after a crash is ~3 s of typing. Writes happen on the UI thread; the file is small (request drafts only, no responses).
- **Restore rules:** a tab linked to a file loads the *file* as its saved state. If it had unsaved edits, the draft is shown on top, so the `●` and the Save/Discard choice survive. Outside edits to the file appear in tabs that had no edits. A file that disappeared reopens as an unsaved draft with a note. Unlinked tabs restore their draft (marked unsaved if it was).
- **Errors:** a corrupt session starts fresh with a note (the next write replaces it). A too-new one is left untouched and this run writes none. A write failure is reported once, then persistence stops for the run.
- **Reopen closed tab:** Cmd/Ctrl+Shift+T, last 20 per run, edits included (so a mistaken Discard is recoverable). Pristine blank tabs aren't recorded. If the file is already open, it just switches to that tab.
- **Not persisted:** responses (History has them), the selected sub-tab, window geometry, closed-tab stack.
- **Secrets:** drafts are stored as typed (`{{vars}}` unresolved), but literal passwords/tokens typed into a tab are in plain text in `session.toml`, the same as request files (D25/D30).

## D32: big responses, Save…, Find (2026-10-29)
- **Spilled bodies (>10 MB, D7)** are read and indexed (`ViewData::new`: rows plus longest row) on GPUI's background executor, then swapped in. Each response bumps the tab's `view_gen`, and a load only lands if the generation still matches, so a slow load never replaces a newer response. The cap is 200 MB in the viewer (the 50 MB spike measured ~130 MB RSS); above that the file path is shown and **Save…** copies it.
- Big bodies are shown **as received**: pretty-printing 50 MB on the fly isn't worth the time and memory. Normal (<10 MB) JSON is still pretty-printed.
- **Save…:** copies the original spilled file byte for byte; otherwise writes the displayed bytes (pretty JSON). The extension comes from the Content-Type.
- **Find:** `at_core::lines::find_all` (ASCII case-insensitive, non-overlapping, capped at 10,000) and `RowIndex::row_of` (binary search on row starts), both unit-tested. Matches are recomputed lazily in render when the (data pointer, query) key changes, so no extra subscriptions are needed. The current match's row is highlighted and scrolled to the centre via a `UniformListScrollHandle`; without wrap, `scroll_x` moves so the match column is visible. Per-tab state.
- **Cmd/Ctrl+F** is bound app-wide but text inputs keep their own binding, so the body editor's built-in find still works when it has focus.

## D33: pre-request and test scripts (2026-10-29)
- **Engine:** `at-script` on `rquickjs` 0.14 (D10). Each run gets a fresh runtime and `Context::full` (ES2023 built-ins, no `std`/`os` modules, so no file/network/process access). Limits: 5 s via an interrupt handler (disarmed before collecting results, so tests that ran before a time-out still show), 64 MB memory, 512 KB stack.
- **Bridge:** the `pm` API is plain JavaScript (`prelude.js`, compiled in with `include_str!`). Inputs go in as one JSON global, and `__finish()` returns JSON. The Rust side is ~100 lines with no per-function bindings, and the API's behaviour lives in one readable file. User code runs as a separate `eval`, so its syntax/runtime errors are reported with the line while the prelude stays intact.
- **Pipeline (`send_tab`):** pre-request script → `sendable` → `build` → send → test script, all in the network task. Scripts run on `tokio::task::spawn_blocking`. Build errors now come back from the task too (`SendOutcome`).
- **Variable semantics:** `pm.variables.set` is the runtime scope (highest precedence, this send only). `pm.environment`/`pm.globals` changes are applied to stored data as a **diff** (`merge_script_vars`): removed keys are deleted, changed/new ones set, secret flags and disabled variables kept, and concurrent tabs don't clobber each other. Without an active environment, env changes are dropped with a note. Known edge: if the Variables page is open with unsaved edits for the same environment while a script changes it, saving the page overwrites the script's change.
- **Request changes from pre-request scripts** (headers, URL, raw body) apply to the sent copy only, never the editor/file. Form bodies can't be changed by scripts yet.
- **Test script input:** response body text up to 10 MB; spilled (>10 MB) bodies aren't passed in (empty), noted in doc 16.
- **Storage:** `Request.pre_request_script` / `test_script` (TOML strings, omitted when empty, placed before the tables so files stay readable). This is an additive field: older files load unchanged, and the schema stays 1.
- **Postman import/export:** `event` scripts map both ways. Collection/folder scripts are flattened into requests, outer first (Postman's execution order), and a note reports how many levels were copied. This replaces the 3e "scripts were not imported" warning.
- **Not yet:** `pm.sendRequest`, `require` libraries, cookies, collection variables persistence, async tests, a collection runner.

## D34: collection runner (2026-10-29)
- **One pipeline:** the per-request work (pre-request script → build → send → test script → history) moved out of `send_tab` into `async fn execute(req, vars, env_name, token, history) -> (SendOutcome, Variables)`. Tabs and the runner share it, so a run behaves exactly like pressing Send.
- **Run order:** depth-first over `Store::load_tree`, the sidebar's order (folders first, then by name). Postman's own item order isn't preserved because our collections are folders of files; a custom order is a later item.
- **Variables:** the run keeps a working `Variables`. Each request's scripts' environment/global changes flow into the next request in the task, and are also sent to the UI with their (before, after) maps, which saves them via the same diff merge as tabs (D33). Runtime `pm.variables` are reset per request.
- **Failure rule:** a row fails on no response, script error, or failed test. A 4xx/5xx alone does not (same as Postman; tests express intent). "Stop on first failure" uses the same rule.
- **Cancellation:** one run token; each request gets a `child_token()`, so Stop aborts the in-flight request and the loop.
- **Progress:** an unbounded channel of `RunMsg::Row/Done`; the UI appends rows as they arrive. Runs continue while other pages are shown.
- **Scope choices:** runs read saved files (unsaved tab edits aren't included, so the run is reproducible), and runs don't write History (runs would flood it). Iterations are capped at 100 and the delay at 60 s.

## D35: cookie jar (2026-10-29)
- **Engine:** `at_http::CookieJar` wraps `cookie_store` 0.22 (RFC 6265, the same engine reqwest's built-in jar uses) behind a `Mutex`, and implements `reqwest::cookie::CookieStore`. `EngineConfig.cookies: Option<Arc<CookieJar>>` is passed as the client's `cookie_provider`, so cookies are stored and sent on every hop, including redirects. We wrap rather than use `reqwest::cookie::Jar` because that one can't list, delete or save cookies.
- **One jar for the app:** shared by all tabs and the runner (a login in one tab or an earlier runner request authenticates the next). Per-environment jars are a later option.
- **A `Cookie` header set on the request wins:** reqwest adds jar cookies only when the request has none (verified in reqwest 0.12.28 `cookie.rs`). An engine test asserts it.
- **Persistence:** `<data-dir>/cookies.json` (cookie_store's JSON format), written atomically and only when the jar changed (`take_changed`, checked after each send/run row and after edits). Session cookies are saved too, so logins survive a restart (Postman behaves the same); expired cookies are dropped on load. A corrupt file starts an empty jar with a note; the next save replaces it, since cookies are cache-like data.
- **UI:** a Cookies page grouped by domain with delete per cookie/domain/all. There's no manual editing yet.
- `execute()` now takes the jar, so tabs and the runner stay identical (D34).

## D36: JSON tree view (2026-11-09)
- **Offset index, not a parsed tree.** `at_core::json_tree` holds one 32-byte `Node` per JSON value with `u32` byte ranges into the original body — the same discipline as the line index (D18) and the pretty-printer. A `serde_json::Value` tree would cost roughly 10x the body and would break NFR-PERF-05's 250 MB ceiling at 10 MB, let alone 50 MB. A 2 MB body indexes to 156,003 nodes / 4.99 MB of index. `at-core` still depends only on `serde` + `thiserror` (D3): `serde_json` appears in `json_tree`'s tests only, where the parser is differentially tested against it for accept/reject agreement.
- **Iterative parser with an explicit stack, never recursive.** Response bodies are untrusted, so nesting must not be able to overflow the thread stack. Nesting deeper than `MAX_DEPTH = 1024` is `Err(TooDeep)`; 100k `[` returns in microseconds instead of crashing.
- **Two deliberate relaxations of RFC 8259,** both covered by tests that record the divergence from `serde_json`: a leading UTF-8 BOM is skipped, and raw control bytes inside strings are accepted — real servers emit both and the viewer still has to show the body. Everything else stays strict (trailing data and commas, leading zeros, `NaN`/`Infinity`, bad escapes, unterminated strings). Numbers are never parsed into a float; the span is kept verbatim, so big integers and `1e2` round-trip exactly. Lone surrogates decode to U+FFFD rather than erroring, for the same "must stay viewable" reason.
- **`value_text` on a container returns that subtree's raw source span**, which makes "copy value" work on any node and makes "copy all" simply `value_text(src, 0)`. The UI must therefore never call it to draw a row — `tree_value_display` uses `summary()` for containers (`{3 fields}`, `[12 items]`) and only scalars go through `value_text`, clipped to 300 characters for display.
- **`path()` computes an array index by scanning the parent's children** rather than storing it, which is what keeps `Node` at 32 bytes. Documented as click-only: it must not be called per visible row. Root path is `$`; descendants carry no prefix, so a field reads exactly as FR-RES-03 writes it (`data.users[0].name`), and identifier-unsafe keys are bracketed and escaped (`data["weird key"]`).
- **`CollapseState` is a `Vec<bool>` sized to the tree**, not a set: ~1 byte per node, no allocation per toggle, no hashing. Default is depth 2 per FR-RES-03.
- **App side:** `TreeView` keeps the tree in an `Arc` (so the render closure clones a pointer, not 150k nodes) plus a **cached list of visible rows**, rebuilt only when the fold state changes — otherwise every frame would re-walk the tree. Rows are a virtualized `uniform_list`; clicks reach the workspace through `cx.entity()`, since a `uniform_list` closure only gets an `&mut App`.
- **Built lazily, once per response**, from `render` when the Tree tab is showing, keyed on the tab's `view_gen` so a new response reindexes and a stale one never wins (the same generation guard as D32). **Cap: 16 MB**, comfortably above the 10 MB the budget names; above it the Body view is offered instead rather than stalling the UI thread. If the parse is ever measured as a visible hitch at 10 MB, the fix is D32's: move it to the background executor.
- **Colours are fixed values, not theme tokens** (keys blue, strings green, numbers peach, booleans mauve, null/containers grey), so a JSON type reads identically in the light and the dark theme. Same choice as the Find highlight below.

## D37: TypeScript, PHP, snapshot tests and Copy as cURL (2026-11-09)
- **FR-CG-01's language list is now complete** — cURL, JavaScript, TypeScript, Python, Go, PHP — with Rust kept as an extra. TypeScript is a real typed generator (`RequestInit`, typed headers, `Response`, no `any`) rather than the JavaScript output relabelled, and it imports `{ openAsBlob } from "node:fs"` because `@types/node` declares no default export for `fs`, so the named form type-checks under a bare `tsc --strict`.
- **PHP emits double-quoted literals everywhere, one convention.** Single quotes cannot express a control character at all, so they were never a real option. `php_str` escapes `\`, `"` and **every** `$` — escaping `$` neutralises both `${` and `{$` interpolation forms at once. `\xHH` is always two digits so a following hex digit can't be swallowed, and the C1 range uses `\u{..}` because `\x9f` would emit one raw byte instead of the two-byte UTF-8 for U+009F.
- **A generated HEAD snippet gets `CURLOPT_NOBODY => true`.** Uniformity with the other generators lost here deliberately: without it the snippet hangs waiting for a body curl will never receive, and D29's premise is that a snippet behaves like Send.
- **Duplicate names are no longer silently collapsed.** `Built::headers` is a `Vec` and multipart parts are a `Vec`, so the same name can legitimately appear twice (`Auth::Custom`, repeated `files[]`), but object/dict literals key on uniqueness and the last one won — which broke D29's "a snippet can never disagree with Send". `fetch` takes an array of pairs and `requests` takes a list of tuples, so JavaScript, TypeScript and Python now switch to those forms **only when a duplicate is actually present**, leaving every ordinary snippet byte-identical to before. Where a language genuinely cannot express it (a Python headers mapping, a PHP `CURLOPT_POSTFIELDS` array) the snippet carries a generated `NOTE:` comment naming the affected names — an honest gap beats a quiet lie. cURL, Go, Rust and PHP headers were already faithful; JS/TS `FormData.append` already was too. Python's urlencoded `data=` needed nothing: `build()` already hands over one percent-encoded string, so a list of tuples there would have *reduced* fidelity by re-encoding.
- **Snapshot tests, 113 of them** (`insta` 1.49, a dev-dependency of `at-codegen` only): 16 request shapes × 7 languages plus a large-body check, each `.snap` read by eye before acceptance. This is what the plan asks for in Phase 4 and it turns an escaping regression into a failing build. The large-body case snapshots a byte count and digest rather than a megabyte, and asserts output stays under 8x input — output is linear in every language (307 KB in → 307 K–410 K out).
- **Known limits, each pinned by a test rather than hidden:** cURL cannot carry a NUL byte in an argument (POSIX `argv` has no encoding for it), and `dq()` leaves DEL and the C1 controls raw in the Python/JavaScript/TypeScript/Go literals — legal and byte-exact, but invisible to a human reading the snippet, and cosmetically inconsistent with Rust's `\u{7f}` and PHP's `\x7f`. `duplicates` compares header names by exact equality, so `X-A` and `x-a` stay two entries, matching how an object literal keys them.
- **Copy as cURL (FR-IE-08)** is a top-bar button next to Save, sharing the Code view's generator through `snippet_for(lang)`, so the two can never disagree. It honours the values-or-`{{kept}}` toggle and warns in the status line when the copied command contains resolved secret values. Copying a saved request straight from the sidebar without opening it is still a later item.

## D38: criterion benchmarks and the budget gates (2026-11-09)
- **`benches/` is its own workspace member and is excluded from `default-members`.** The plan's section 2.1 puts `benches/` at the root and section 6 asks for criterion coverage, but criterion is the slowest thing in the workspace to compile, and D21 already established that slow targets stay out of the default set so `cargo test` and CI stay fast. Verified: a plain `cargo test` build log mentions criterion zero times.
- **criterion pinned to 0.5.1, not 0.8.2** — 0.8's `rust-version` is 1.86 and the workspace MSRV is 1.80 (D1). Built with `default-features = false` plus `cargo_bench_support`, since `plotters`/`rayon` only matter for `html_reports`, which we don't use.
- **Sample bodies are generated in process from a fixed seed**, never committed or downloaded, so a 10 MB body is byte-identical run to run (10,485,142 bytes / 439,411 rows) and numbers are comparable across runs.
- **The wall-clock budget gate is a normal `#[test]` marked `#[ignore]`.** A timing assertion would be flaky on a shared CI runner, so it never runs in `cargo test`; it is the command a human runs when the viewer or the index changes (`cargo test -p at-benches -- --ignored --nocapture`). It asserts the NFR-PERF-06 10 MB first-match search stays under the 500 ms hard limit — measured 23.5 ms debug, 6.4 ms release.
- **What is measured and what is not:** `cargo bench` uses `[profile.bench]` (plain `opt-level = 3`), *not* the shipping `[profile.release]` with fat LTO and one codegen unit, so the release gate figures are the ones that reflect what users get. Numbers are in `docs/BASELINE.md`. The CI budget gates section 6 ultimately wants — failing the build on a regression against a stored baseline — are still to wire up; this crate is the measurement half only.

## D39: the app crate is split by concern, not kept in one file (2026-11-09)
- `crates/app/src/main.rs` had grown to 6,411 lines and was the entire GUI crate. It is now 35 modules; the largest is 892 lines and `main.rs` is 79 (crate doc, module list, `main()`).
- **Layout:** `editor`, `response`, `tree`, `script`, `runner` and `sidebar` own one feature's state each; `format` and `ui` hold the pure helpers (string and number formatting, colours and metrics) with no UI types in their signatures; `request_tab` owns everything belonging to one open request (D26). `workspace/` splits what was a 3,290-line `impl Workspace` into 23 inherent `impl` blocks — one per concern, following the section markers the file already carried — and `workspace/render/` is the view layer, one file per panel.
- **Why this works in Rust:** inherent `impl` blocks for one type may live in several modules of the same crate, and a module's private items are visible to its descendants — so `Workspace`'s fields stay private in `workspace/mod.rs` and every `workspace::*` child still reaches them. Types shared across sibling modules are `pub(crate)`, which is purely internal in a binary crate; there is no bare `pub`.
- **Deliberately not done:** no traits, generics or indirection were introduced. `Workspace` being one central entity is how retained-mode GPUI works, and hiding it behind an interface would cost readability and buy nothing. The single-responsibility split is the whole win; each module carries a `//!` doc saying what it owns and why it is one unit, and each test moved next to the code it tests.
- Verified as a pure move: a token and comment multiset comparison against the previous `main.rs` found no code token and no comment lost or added, the only deltas being imports, module docs and a few rustfmt-inserted commas.
- Still outsized: `workspace/render/mod.rs` is a single 892-line `fn render`. Extracting `render_sidebar` and `render_viewer` would bring it to roughly 500 lines, but that invents new signatures and threads locals through them, so it is a change to make and review on its own rather than inside a move.
