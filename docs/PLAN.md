# API Tester (GPUI) — Phased Implementation Plan

Version 0.1 (draft) · 23 October 2026 · Companion to the API Tester Software Requirements Specification (SRS). Requirement IDs such as FR-ENG-01 refer to the SRS.

## 1. Goals and guiding principles
The plan builds a native Rust and GPUI API client for macOS, Windows and Linux that stays within 100 MB installed and 250 MB of RAM while feeling instant, and that installs with one command.

1. **Measure first.** Phase 0 proves the size, memory and speed budgets with real prototypes before any feature work. The result is a go or no-go decision on GPUI.
2. **Core before UI.** All logic (models, variables, HTTP, scripting, formats, storage) lives in UI-independent crates. If GPUI proves unsuitable, only the thin app crate is replaced (egui or Slint are the fallbacks).
3. **Always shippable.** A minimal install pipeline exists from Phase 2, so every later phase ships through the same one-command path.
4. **Budgets are tests.** Binary size, startup time, memory and render speed are checked in CI and fail the build when they regress.
5. **Memory discipline by design.** Response bodies are shared, never copied; the viewer holds line offsets, not laid-out text; large bodies spill to disk; history stores truncated bodies.

## 2. Architecture and technology choices

### 2.1 Cargo workspace
```
api-tester/
  Cargo.toml                workspace
  crates/
    core/        models, variables, auth, request building, errors
    http/        HTTP engine, streaming, cancellation, timings
    protocols/   websocket, graphql, grpc
    script/      JavaScript sandbox, expect(), run results
    formats/     import/export (Postman, OpenAPI, HAR, cURL), code generation
    store/       collection files, settings, SQLite history, migrations
    app/         GPUI app: views, actions, keymap, theme
    cli/         headless runner (post-1.0)
  xtask/         release and benchmark helpers
  benches/       criterion benchmarks
  .github/workflows/
```

### 2.2 Candidate crates
These are starting points, not commitments. Confirm current versions and maintenance status in Phase 0 and record the final choices in the repository.

| Area | Candidate | Notes |
|---|---|---|
| UI | `gpui`, evaluate `gpui-component` | Pin to a known git revision; check whether a crates.io release exists. Look for virtual lists, tables, inputs and an editor widget |
| Async | `tokio` for networking on its own threads | Bridge results into GPUI tasks with channels |
| HTTP | `reqwest` with rustls first; a custom `hyper` client later | Per-phase timings (FR-ENG-08) need hyper-level hooks |
| WebSocket | `tokio-tungstenite` | |
| gRPC | `tonic`, `prost-reflect`, `protox`, reflection client | Dynamic messages from .proto files or server reflection |
| GraphQL | a small parser crate for formatting; introspection over HTTP | |
| Scripting | `rquickjs` (QuickJS) preferred; `boa_engine` or `rhai` as alternatives | Decided by Phase 0 benchmarks |
| Serialization | `serde`, `serde_json`, `toml`, a maintained YAML crate | |
| Large JSON | `sonic-rs` or `simd-json`, plus a byte-offset index | Lazy tree, no full in-memory DOM |
| Storage | `rusqlite` (bundled) for history, atomic file writes for the rest | |
| Editor and highlighting | widget from `gpui-component`, or a minimal editor on `ropey`; `tree-sitter` JSON and XML grammars | The biggest UI risk; see section 7 |
| Misc | `directories`, `keyring`, `mimalloc`, `shlex`, `openapiv3` | |
| Quality | `criterion`, `insta`, `wiremock`, `cargo-audit`, `cargo-deny` | |
| Release | `cargo-dist` | Generates the shell and PowerShell installers, checksums and release workflow |

## 3. Phase overview
Estimates assume one experienced Rust developer working full time. A team of two or three can run Phases 3 to 6 in parallel and cut the calendar time by roughly a third.

| Phase | Goal | Main SRS coverage | Est. |
|---|---|---|---|
| 0 | Foundations and spikes; go or no-go on GPUI | NFR budgets, decisions in SRS section 9 | 1–2 wk |
| 1 | Core engine | FR-ENG, FR-ENV-02/03, FR-AUTH-01/02 | 2 wk |
| 2 | App shell, first request end to end, install skeleton | FR-WS, FR-RB, FR-RES-01/02/04/06/07, FR-DIST-05 | 3 wk |
| 3 | Persistence, collections, environments, history | FR-COL, FR-ENV, FR-HIS, FR-PER | 3 wk |
| 4 | Response power features and code generation | FR-RES-03/05, FR-CG, cURL import and export | 2 wk |
| 5 | Import and export, palette, shortcuts, settings | FR-IE, FR-KEY, FR-SET | 2 wk |
| 6 | Scripting, tests and runner | FR-SCR, FR-RUN-01 to 05 | 3 wk |
| 7 | GraphQL, WebSocket, gRPC | FR-GQL, FR-WS-10 to 14, FR-GRPC | 4 wk |
| 8 | Distribution hardening and private beta | FR-DIST-01 to 05 | 2 wk |
| 9 | Polish and 1.0 release candidate | All P0 items, all NFRs | 2–3 wk |
| 10 | Post-1.0 releases | All P1 and P2 items | ongoing |

Release 1.0 is the end of Phase 9, roughly 24 to 28 weeks for a solo developer.

## 4. Phase details

### Phase 0 — Foundations and spikes (1–2 weeks)
**Goal:** replace assumptions with measurements.

**Deliverables**
- Cargo workspace, formatting and lint rules, and a CI matrix on macOS, Windows and Linux that builds and tests every push.
- GPUI hello-world window on all three operating systems. Record the minimum OS versions, GPU needs, Wayland and X11 behavior, and the GPUI revision used.
- **Spike A, viewer:** a virtualized viewer for a 10 MB JSON file with scrolling and search. Measure first paint, scroll smoothness, RSS and binary size.
- **Spike B, scripting:** the same sample script on two or three engines; compare binary size added, startup time, execution speed and memory.
- **Spike C, editor:** try the editor widget options for a multi-line body editor with line numbers and variable highlighting.
- Decisions recorded: script engine, collection file format (TOML or JSON), editor approach.
- Baseline numbers for installed size, idle RSS and cold start.

**Exit criteria:** hello-world plus viewer spike stay well inside the budgets (target idle RSS under 120 MB, binary under 40 MB). If they do not, run the fallback decision: keep the crates, swap the UI layer.

### Phase 1 — Core engine (2 weeks)
**Goal:** a tested engine with no UI.

**Deliverables**
- Data models from SRS section 6 with serde support and schema versions.
- Variable resolution with precedence (FR-ENV-02) and `{{name}}` substitution with unresolved tracking (FR-ENV-03).
- Request builder pipeline: URL with enabled params, headers, auth (none, basic, bearer, API key, JWT, custom), body serialization for all P0 body types including multipart (FR-RB-06, FR-AUTH-01).
- HTTP engine on rustls: HTTP/1.1 and HTTP/2, cancellation, timeout, redirect control, TLS verify toggle, streaming with disk spill above the threshold, structured errors, timing basics (FR-ENG-01 to 06).
- A tiny test CLI (`cargo run -p core-demo`) that sends a request from a JSON file.
- Unit tests for resolution, URL and header building, auth, and body encoding; HTTP integration tests against a local mock server.

**Exit criteria:** engine tests green on three OSes; app-added overhead per request measured and under 5 ms (NFR-PERF-03).

### Phase 2 — App shell and first request (3 weeks)
**Goal:** send a request and read the response in a real window, installable from a script.

**Deliverables**
- Main window layout with sidebar, tabs, request editor and response pane, resizable splits (FR-WS-01 to 04).
- Theme tokens with light and dark themes following the system (FR-SET-01).
- URL bar with method selector and variable highlighting, Send and Cancel (FR-RB-01 to 03).
- Params, headers and body editors, with params synced to the URL (FR-RB-04 to 08, FR-RB-10).
- Response viewer: status bar, Body and Headers tabs, pretty and raw toggle, virtualized rendering, loading and empty states (FR-RES-01, 02, 04, 06, 07).
- Toasts, confirm dialogs and error states (FR-WS-07).
- **Install skeleton:** release workflow with `cargo-dist` that builds all five target assets, produces checksums, and publishes a draft release; first working install scripts.

**Exit criteria:** a developer can run the install command on a clean machine of each OS, launch the app, send GET and POST requests and read a 1 MB JSON response within the render budget (NFR-PERF-04).

### Phase 3 — Persistence, collections, environments, history (3 weeks)
**Goal:** nothing is lost between launches, and work can be organized.

**Deliverables**
- Store crate: settings, session, collection files, environment files, atomic writes, debounced autosave, schema migration with backup (FR-PER-01 to 05).
- Collections tree with nested folders, rename, duplicate, delete, drag and move, collection variables, open into tabs, save with Ctrl/Cmd+S (FR-COL-01 to 06, FR-RB-11).
- Environments page, active environment selector, global variables, secret masking and reveal (FR-ENV-01, 04, 05).
- History in SQLite with cap, truncated and compressed bodies, search, delete, clear, reopen (FR-HIS-01 to 05).
- Session restore for tabs and pane sizes (FR-WS-05).

**Exit criteria:** kill the app mid-save and relaunch without corruption; sidebar with 5,000 requests opens and filters inside the budget (NFR-PERF-07); history database stays bounded.

### Phase 4 — Response power features and code generation (2 weeks)
**Goal:** make the response viewer and sharing features match the legacy app.

**Deliverables**
- JSON tree view with collapse, default depth 2, type colors, copy value, copy path, copy all (FR-RES-03).
- Body search with live match count and next and previous (FR-RES-05), meeting the 10 MB search budget (NFR-PERF-06).
- Code generation for cURL, JavaScript, TypeScript, Python, Go and PHP with a language picker, copy feedback and a variables option (FR-CG-01 to 04).
- cURL import and Copy as cURL (FR-IE-02, FR-IE-08 part).
- Snapshot tests for every language and a set of tricky inputs (quotes, newlines, unicode, large bodies).

**Exit criteria:** 10 MB JSON opens and scrolls at 60 fps within the RAM limit (NFR-PERF-05).

### Phase 5 — Import and export, command palette, shortcuts, settings (2 weeks)
**Goal:** easy migration in and out, and keyboard-first operation.

**Deliverables**
- Importers: Postman v2.1 with recursive folders, HAR, native JSON, with file picker, drag and drop and paste, and clean errors for malformed input (FR-IE-01, 03, 04, 09).
- Exporters: JSON and Postman v2.1 (FR-IE-06, 08).
- Command palette with fuzzy search and the P0 command set (FR-KEY-01, 02).
- Global shortcuts with Cmd on macOS and Ctrl elsewhere (FR-KEY-03, 04).
- Settings screen: timeout, TLS verification, history cap, proxy fields, data directory, no telemetry (FR-SET-02, 03).
- Round-trip tests: import a Postman collection, export it, import again, compare.

**Exit criteria:** a real exported Postman collection imports with all requests and folders intact.

### Phase 6 — Scripting, tests and runner (3 weeks)
**Goal:** pre-request scripts, assertions and sequential collection runs.

**Deliverables**
- Embedded JavaScript engine with the sandbox: `console`, `expect()`, mutable `vars`, `request` and `response` objects, time and memory limits, no file, network or process access (FR-SCR-01 to 05).
- All eleven matchers from the legacy app, with failure messages (FR-SCR-03).
- Script editors in the request builder, Run Tests button, automatic run after send, results in the response pane (FR-SCR-06).
- Runner view: collection picker, run and stop, per-request states, live stats, scripts executed and failed assertions marked failed (FR-RUN-01 to 05).
- Security tests: scripts that try to read files, open sockets or loop forever must be contained.

**Exit criteria:** a 1,000-request run keeps memory flat (NFR-PERF-08); sandbox tests pass.

### Phase 7 — GraphQL, WebSocket and gRPC (4 weeks)
**Goal:** reach protocol parity and go beyond the legacy gRPC-Web limitation.

**Deliverables**
- **GraphQL (week 1):** endpoint, query and variables editors, headers manager, prettify, response, recent queries, variable substitution (FR-GQL-01 to 04).
- **WebSocket (week 1–2):** connect and disconnect states, message log with direction colors, send text and JSON, auto-reconnect after 3 seconds, copy and clear (FR-WS-10 to 14).
- **gRPC (weeks 2–4):** native HTTP/2 client, load services from .proto files or reflection, JSON request mapped to protobuf, metadata, TLS, unary calls (FR-GRPC-01 to 04).
- Integration tests against a local echo WebSocket server and a local gRPC server.

**Exit criteria:** each protocol works end to end on all three OSes; gRPC unary calls succeed against both a reflection-enabled and a .proto-only server.

### Phase 8 — Distribution hardening and private beta (2 weeks)
**Goal:** the install experience is solid for strangers.

**Deliverables**
- Final install commands (section 5), tested on clean VMs: macOS arm64 and Intel, Windows 11, Ubuntu, Fedora and Arch.
- Checksum verification, per-user install without admin rights, launcher or menu entry, PATH shim, and a single-command uninstall that can keep user data (FR-DIST-01 to 04).
- Release process documented; tagged builds publish all assets automatically (FR-DIST-05).
- Private beta with a small group of developers; issue template; crash log location documented.

**Exit criteria:** zero blocking installer defects across the VM matrix; beta feedback triaged.

### Phase 9 — Polish and 1.0 release candidate (2–3 weeks)
**Goal:** meet every P0 requirement and every budget.

**Deliverables**
- Full P0 acceptance pass against the SRS tables, with any gaps fixed.
- Performance pass against all NFR-PERF items and the memory and size budgets on a reference machine per OS.
- Accessibility review (keyboard navigation, focus indicators) and documentation of what GPUI does not expose.
- User documentation: getting started, keyboard shortcuts, import guide, scripting reference.
- Release candidate, then 1.0.

**Exit criteria:** section 8 checklist is complete.

### Phase 10 — Post-1.0 releases
Grouped into release trains; reorder by user demand.

| Release | Contents |
|---|---|
| 1.1 | OAuth 2.0 and digest auth, OpenAPI and Swagger import, request diff, response comparison, JSON filtering, binary body and image preview, more code languages |
| 1.2 | Runner iterations and data files, run reports, collection-level inheritance, timing breakdown and redirect chain, proxy settings polish, GraphQL introspection and docs explorer, gRPC streaming |
| 1.3 | In-app updates, macOS signing and notarization, Windows signing, Homebrew, winget, Scoop, AUR, .deb, .rpm, AppImage, keychain-backed secrets, legacy api-tester-kit importer |
| 2.0 | Headless CLI runner for CI, built-in mock server, customizable keymap, AWS Signature v4, Windows arm64 and macOS universal builds, high-contrast theme |

## 5. Single-command installation

### 5.1 User-facing commands
Placeholders are used until the repository and host are chosen.

macOS and Linux:
```
curl -fsSL https://<install-host>/install.sh | sh
```

Windows (PowerShell):
```
irm https://<install-host>/install.ps1 | iex
```

Uninstall:
```
api-tester self-uninstall
```

### 5.2 What the installer does
1. Detects OS and CPU architecture and picks the matching release asset.
2. Downloads the asset and its SHA-256 checksum over HTTPS and verifies it; aborts on mismatch.
3. Installs under a per-user directory without administrator rights, then adds a PATH shim and a launcher (macOS app bundle or Applications entry, Windows Start Menu shortcut, Linux desktop entry).
4. Prints the installed version and how to launch and uninstall.

### 5.3 Release pipeline
- Tagging a version triggers the CI matrix: build, test, benchmark gates, then package.
- `cargo-dist` produces the archives, checksums, installer scripts and the GitHub release; the installer scripts are published with every release so the URL stays stable.
- Assets: macOS arm64 and x86_64, Windows x64, Linux x86_64 and aarch64.

### 5.4 Signing
Downloads made with curl or PowerShell are not marked as quarantined downloads in the same way browser downloads are, so unsigned builds can run from the installer path, but this must be verified on clean machines in Phase 8. Proper macOS signing and notarization and Windows code signing are scheduled in release 1.3. Both involve yearly costs for certificates or developer program membership.

### 5.5 Package managers (release 1.3)
Homebrew, winget, Scoop, AUR, .deb, .rpm, AppImage and `cargo install` follow once the install script path is stable.

## 6. Testing, benchmarks and budget gates

| Layer | Approach |
|---|---|
| Unit | Core, formats, codegen, script and store crates; snapshot tests for generated code and exports |
| Integration | Request flow against a local mock server; WebSocket echo server; local gRPC server; Postman round trips |
| UI | GPUI test harness for key flows where available; manual checklists per OS per release |
| Security | Script sandbox escape tests; installer tampering test (bad checksum must abort); `cargo-audit` and `cargo-deny` in CI |
| Performance | `criterion` benchmarks for JSON indexing, search and request building; scripted workload that records RSS |

**Gates in CI (fail the build if exceeded):** installed binary size, cold start time, idle RSS, RSS during the 10 MB response workload, RSS after 1,000 runner requests. Thresholds come from SRS section 5 and are tightened after Phase 0 produces the real baseline.

## 7. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| GPUI is pre-1.0 and changes often | Rework when upgrading | Pin a revision, isolate it in the app crate, upgrade on a schedule |
| No ready-made text editor widget fits | Delays Phases 2 and 4 | Spike C in Phase 0; fall back to a minimal editor on `ropey` with plain-text highlighting first |
| Linux GPU, Wayland or driver differences | Crashes or blank windows on some machines | Test matrix of distributions in Phase 8; document GPU requirements; check fallback behavior in Phase 0 |
| Script engine adds size or memory | Budget pressure | Phase 0 comparison; smallest engine that passes compatibility tests |
| Large responses blow the RAM limit | Breaks the 250 MB promise | Disk spill, line-offset index, lazy JSON tree, CI memory gate |
| gRPC dynamic protobuf is complex | Phase 7 overrun | Ship unary first; streaming moves to 1.2 |
| Accessibility limits in GPUI | Gaps for screen reader users | Document early; keyboard-first design; revisit as GPUI improves |
| Signing cost and setup time | Install warnings on some systems | Verify unsigned path in Phase 8; budget for certificates before 1.3 |
| Scope growth | Slips 1.0 | P0 list is fixed; new ideas go to the post-1.0 trains |

## 8. Definition of done for 1.0
- Every P0 requirement in the SRS passes its acceptance check on macOS, Windows and Linux.
- All NFR hard limits are met on a reference machine per OS: installed size at most 100 MB, memory at most 250 MB in the heaviest supported workload.
- Targets are met or the gap is documented: size 40 MB, idle RSS 120 MB, cold start 500 ms.
- The install command works on every machine in the clean-VM matrix, and uninstall works.
- Crash-safety test passes: no data loss when the app is killed during a save.
- Script sandbox tests and installer tamper tests pass.
- User documentation published and release notes written.
