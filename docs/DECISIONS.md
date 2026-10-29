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
