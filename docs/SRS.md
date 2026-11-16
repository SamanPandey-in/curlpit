# API Tester (GPUI) — Software Requirements Specification

Version 0.1 (draft) · 23 October 2026 · Derived from the retired Next.js project api-tester-kit

## 1. Introduction

### 1.1 Purpose
This document specifies the requirements for **API Tester**, a native desktop application for building, sending and testing API requests (a Postman alternative), written in Rust on GPUI, the UI framework behind the Zed editor. It consolidates every module found in the earlier Next.js implementation (api-tester-kit, docs 01 to 22), which is being retired because of its memory footprint, and adds the requirements a native single-binary product needs.

### 1.2 Scope
**In scope for 1.0:** HTTP/REST testing, GraphQL, WebSocket, gRPC, environments and variables, collections, history, scripting and assertions, collection runner, import/export, code generation, command palette and shortcuts, local-first storage, and one-command installation on macOS, Windows and Linux.

**Out of scope for 1.0:** cloud sync, accounts, team collaboration, a browser or mobile edition, and the Docker/Nginx VPS deployment used by the legacy app.

### 1.3 Definitions
| Term | Meaning |
|---|---|
| GPUI | Rust GPU-accelerated UI framework developed for the Zed editor |
| Collection | A named group of requests and nested folders, with optional collection variables |
| Environment | A named set of variables (for example Local, Staging, Production) |
| Variable | A value referenced as `{{name}}` in requests |
| Tab | An open request editor in the main workspace |
| Runner | Feature that executes the requests of a collection in sequence |

### 1.4 Priorities
**P0** = required for release 1.0. **P1** = shortly after 1.0. **P2** = later or optional. Requirement IDs are stable and are referenced by the implementation plan.

## 2. Overall description

### 2.1 Product perspective
API Tester is a standalone, local-first desktop application shipped as one native binary with no server component. The legacy `/api/proxy` route existed only to get around browser CORS limits. A native client sends requests directly, so the proxy layer is dropped and replaced by an optional outbound-proxy setting.

### 2.2 Users
- API and backend developers who build and debug services (primary).
- QA engineers who write assertions and run collections.
- Front-end and mobile developers who need quick calls, code snippets and mocks.

### 2.3 Operating environment
macOS (Apple Silicon and Intel), Windows 10/11 (x64), Linux (x86_64 and aarch64, X11 and Wayland). Exact minimum OS versions and GPU requirements are to be confirmed against GPUI's current platform support (section 9).

### 2.4 Constraints
- Rust and GPUI only: no webview, Node or other runtime dependency.
- Installed size at most 100 MB; resident memory at most 250 MB under the supported workload. Tighter targets are in section 5.
- Must feel instant: fast startup, 60 fps interaction, fast rendering of large responses.
- Installable with a single command on each OS.
- All data stored locally; fully usable offline.

### 2.5 Assumptions and dependencies
- GPUI is pre-1.0 and its API changes often. The UI layer is pinned to a known revision and kept separate from a UI-independent core crate.
- HTTP, TLS, WebSocket, gRPC, scripting and storage rely on third-party Rust crates selected in the implementation plan.

## 3. Functional requirements

### 3.1 Workspace, layout and tabs
| ID | Requirement | Pri |
|---|---|---|
| FR-WS-01 | Main window with collapsible sidebar (navigation, collections tree, open tabs), request editor and response viewer; panes are resizable and sizes persist | P0 |
| FR-WS-02 | Multiple request tabs: create, close, switch; closing a tab activates the nearest adjacent tab | P0 |
| FR-WS-03 | Pin tabs (visually distinguished) and show an unsaved (dirty) marker | P0 |
| FR-WS-04 | Navigation to Requests, Collections, Environments, History, Runner, GraphQL, WebSocket, gRPC and Settings | P0 |
| FR-WS-05 | Restore open tabs and the active tab on relaunch | P1 |
| FR-WS-06 | Reorder tabs by drag and drop | P1 |
| FR-WS-07 | Toast notifications, confirm dialogs and empty states for every list and viewer | P0 |

### 3.2 Request builder
| ID | Requirement | Pri |
|---|---|---|
| FR-RB-01 | Methods: GET, POST, PUT, PATCH, DELETE, OPTIONS, HEAD, TRACE, CONNECT, each with a distinct color | P0 |
| FR-RB-02 | URL bar in monospace with `{{variable}}` highlighting (resolved vs unresolved) | P0 |
| FR-RB-03 | Send by button or Ctrl/Cmd+Enter; Cancel button replaces Send while a request is in flight | P0 |
| FR-RB-04 | Query parameters table (key, value, description, enable checkbox, add, delete) kept in sync with the URL query string in both directions | P0 |
| FR-RB-05 | Headers table with the same behavior; new requests default to `Content-Type: application/json` | P0 |
| FR-RB-06 | Body types: none, JSON, XML, HTML, text, x-www-form-urlencoded, multipart form-data (text and file fields) | P0 |
| FR-RB-07 | Body type binary/file upload | P1 |
| FR-RB-08 | Body editor: monospace, line numbers, JSON validation, variable highlighting | P0 |
| FR-RB-09 | JSON beautify/minify and syntax highlighting in the body editor | P1 |
| FR-RB-10 | Per-request settings: timeout, follow redirects, TLS verification | P0 |
| FR-RB-11 | Save request to a collection or folder (Ctrl/Cmd+S) | P0 |
| FR-RB-12 | Variable picker for inserting `{{variables}}` | P1 |
| FR-RB-13 | URL autocomplete from history | P1 |
| FR-RB-14 | Header name and value autocomplete; cookie jar; client certificates | P1 |

### 3.3 Authentication
| ID | Requirement | Pri |
|---|---|---|
| FR-AUTH-01 | Types: none, basic, bearer, API key (header or query), JWT bearer, custom headers | P0 |
| FR-AUTH-02 | Auth is applied at send time and reflected in code generation | P0 |
| FR-AUTH-03 | OAuth 2.0 (authorization code with PKCE, client credentials, token refresh) | P1 |
| FR-AUTH-04 | Digest auth; inherit auth from folder or collection | P1 |
| FR-AUTH-05 | AWS Signature v4 | P2 |

### 3.4 Variables and environments
| ID | Requirement | Pri |
|---|---|---|
| FR-ENV-01 | Create, rename, delete, activate and deactivate environments; each holds key/value variables with a secret flag | P0 |
| FR-ENV-02 | Global variables. Precedence, lowest to highest: global, collection, active environment, script-set runtime variables | P0 |
| FR-ENV-03 | `{{name}}` substitution in URL, params, headers, body and auth fields; unresolved variables stay literal and are flagged | P0 |
| FR-ENV-04 | Secret values are masked with a reveal toggle and excluded from exports by default | P0 |
| FR-ENV-05 | Full Environments page for create, edit and delete | P0 |
| FR-ENV-06 | Quick-edit popover in the request header to view and edit global or active-environment variables | P1 |
| FR-ENV-07 | Secrets stored in the OS keychain | P1 |
| FR-ENV-08 | Dynamic built-ins such as `{{$uuid}}`, `{{$timestamp}}`, `{{$randomInt}}` | P1 |

### 3.5 HTTP engine
| ID | Requirement | Pri |
|---|---|---|
| FR-ENG-01 | Native HTTP/1.1 and HTTP/2 client over TLS 1.2/1.3 with no CORS restrictions; localhost and LAN targets work | P0 |
| FR-ENG-02 | Pipeline: resolve variables, build URL with enabled params, build headers and auth, serialize body, send | P0 |
| FR-ENG-03 | Cancel an in-flight request immediately | P0 |
| FR-ENG-04 | Capture status, status text, headers, body, total time, size in bytes and timestamp | P0 |
| FR-ENG-05 | Stream large bodies and SSE; bodies above a threshold (proposed 10 MB) spill to disk instead of staying in memory | P0 |
| FR-ENG-06 | Clear, structured errors for DNS failure, connection refused, TLS error and timeout | P0 |
| FR-ENG-07 | Transparent gzip, brotli and zstd decompression | P1 |
| FR-ENG-08 | Timing breakdown (DNS, connect, TLS, first byte, download) and redirect chain | P1 |
| FR-ENG-09 | Outbound HTTP/SOCKS proxy settings, honoring system proxy variables; replaces the legacy direct/proxy/auto mode | P1 |

### 3.6 Response viewer
| ID | Requirement | Pri |
|---|---|---|
| FR-RES-01 | Status bar: color-coded status badge (2xx green, 3xx amber, 4xx and 5xx red), time, size, copy body, save to file | P0 |
| FR-RES-02 | Tabs: Body, Headers (sorted, monospace), Code | P0 |
| FR-RES-03 | JSON tree: collapsible, default depth 2, colors for keys, strings, numbers, booleans and null, copy value, copy JSON path (for example `data.users[0].name`), copy all | P0 |
| FR-RES-04 | Pretty/raw toggle for JSON, XML, HTML and text | P0 |
| FR-RES-05 | Search in body: live, case-insensitive, match count (current/total), next and previous, clear | P0 |
| FR-RES-06 | Virtualized rendering: only visible lines are laid out, so large bodies stay smooth | P0 |
| FR-RES-07 | Loading state with spinner and an empty state before the first send | P0 |
| FR-RES-08 | Image preview and hex view for binary responses | P1 |
| FR-RES-09 | Compare two responses side by side | P1 |
| FR-RES-10 | Filter JSON with a path or query expression | P1 |
| FR-RES-11 | Tests and Timeline tabs | P1 |

### 3.7 Collections
| ID | Requirement | Pri |
|---|---|---|
| FR-COL-01 | Create, rename, describe and delete collections | P0 |
| FR-COL-02 | Nested folders of unlimited depth; requests live at the root or in folders | P0 |
| FR-COL-03 | Rename, duplicate, delete and drag-move requests and folders | P0 |
| FR-COL-04 | Collection-level variables | P0 |
| FR-COL-05 | Open a request from the sidebar tree into a tab | P0 |
| FR-COL-06 | Collections stored as human-readable files on disk, one file per request, friendly to version control | P0 |
| FR-COL-07 | Folder and collection level auth, headers and scripts inherited by children | P1 |

### 3.8 History
| ID | Requirement | Pri |
|---|---|---|
| FR-HIS-01 | Record each sent request with its response summary, active environment name and timestamp | P0 |
| FR-HIS-02 | Default cap of 500 entries (configurable); oldest are pruned; stored response bodies are truncated and compressed | P0 |
| FR-HIS-03 | Debounced search by URL or method; delete one entry; clear all | P0 |
| FR-HIS-04 | Reopen a history entry in a new tab | P0 |
| FR-HIS-05 | Runner requests are not written to history unless a setting enables it | P0 |

### 3.9 Import and export
| ID | Requirement | Pri |
|---|---|---|
| FR-IE-01 | Import Postman Collection v2.1 including recursive folders, method, raw URL, headers, query and raw body | P0 |
| FR-IE-02 | Import cURL command (method, URL, headers, data, basic auth, `--data-raw`, `--compressed`) | P0 |
| FR-IE-03 | Import HAR files | P0 |
| FR-IE-04 | Import native JSON collections | P0 |
| FR-IE-05 | Import OpenAPI 3.x and Swagger 2.0: all methods, path/query/header parameters, example bodies, servers as a variable, tags as folders (the legacy importer covered only query parameters and five methods) | P1 |
| FR-IE-06 | Export a collection as JSON | P0 |
| FR-IE-07 | Export as YAML and as Markdown documentation | P1 |
| FR-IE-08 | Export to Postman v2.1; copy any request as cURL | P0 |
| FR-IE-09 | Import from file picker, drag and drop, or paste; malformed input shows an error and never crashes | P0 |
| FR-IE-10 | Preview what will be imported before confirming; export OpenAPI | P1 |

### 3.10 Code generation
| ID | Requirement | Pri |
|---|---|---|
| FR-CG-01 | Languages: cURL, JavaScript (fetch), TypeScript (fetch), Python (requests), Go (net/http), PHP (cURL) | P0 |
| FR-CG-02 | Output includes enabled params, headers, auth and body, with correct escaping | P0 |
| FR-CG-03 | Language selector and copy button with success feedback | P0 |
| FR-CG-04 | Option to resolve variables or keep `{{placeholders}}` | P0 |
| FR-CG-05 | More languages: Rust (reqwest), Java, C#, Ruby, Swift, Kotlin, HTTPie, wget | P1 |

### 3.11 Scripting and tests
| ID | Requirement | Pri |
|---|---|---|
| FR-SCR-01 | Pre-request script and test (post-response) script per request | P0 |
| FR-SCR-02 | Sandboxed JavaScript with `console.log/warn/error`, `expect()`, mutable `vars`, `request`, and `response` (status, statusText, headers, parsed body, time) | P0 |
| FR-SCR-03 | Assertions: toBe, toBeDefined, toBeUndefined, toBeNull, toBeTruthy, toBeFalsy, toEqual, toContain, toBeLessThan, toBeGreaterThan, toHaveProperty | P0 |
| FR-SCR-04 | Result object: success flag, logs, errors, variables set, passed and failed counts, failure messages | P0 |
| FR-SCR-05 | Scripts cannot access the file system, network or processes; execution has a time limit (proposed 5 s) and a memory limit | P0 |
| FR-SCR-06 | Run Tests button, plus automatic test run after each send | P0 |
| FR-SCR-07 | Postman-compatible `pm.*` shim; more matchers (toMatch, toBeGreaterThanOrEqual, and similar) | P1 |
| FR-SCR-08 | Option to persist script-set variables into the active environment | P1 |

### 3.12 Collection runner
| ID | Requirement | Pri |
|---|---|---|
| FR-RUN-01 | Select a collection (or folder) and run its requests sequentially with the active environment | P0 |
| FR-RUN-02 | Per-request states: pending, running, success, error; row shows index, status code, method, name, time and error | P0 |
| FR-RUN-03 | Live stats: total, success, failed, pending | P0 |
| FR-RUN-04 | Stop aborts the in-flight request and leaves the remaining requests pending | P0 |
| FR-RUN-05 | Pre-request and test scripts run for each request; failed assertions mark the request failed (the legacy runner only called the send function) | P0 |
| FR-RUN-06 | Iterations, delay between requests, data file (CSV or JSON), stop on first failure | P1 |
| FR-RUN-07 | Export run report as JSON or HTML | P1 |
| FR-RUN-08 | Headless CLI runner for CI that reads the same collection files | P2 |

### 3.13 GraphQL
| ID | Requirement | Pri |
|---|---|---|
| FR-GQL-01 | Endpoint URL, query editor, variables JSON editor, headers manager with enable toggles, Run button | P0 |
| FR-GQL-02 | Formatted JSON response; format (prettify) and copy buttons; character count | P0 |
| FR-GQL-03 | `{{variable}}` substitution in URL, query, variables and headers | P0 |
| FR-GQL-04 | Recent queries list (last 50) for quick replay | P0 |
| FR-GQL-05 | Schema introspection, docs explorer and autocomplete | P1 |
| FR-GQL-06 | Subscriptions over WebSocket; save queries into collections | P1 |

### 3.14 WebSocket
| ID | Requirement | Pri |
|---|---|---|
| FR-WS-10 | Connect and disconnect with visible state: disconnected, connecting, connected, error | P0 |
| FR-WS-11 | Send text or JSON messages (Ctrl/Cmd+Enter) | P0 |
| FR-WS-12 | Message log with timestamps and color-coded direction; clear log; copy any message | P0 |
| FR-WS-13 | Optional auto-reconnect after 3 seconds, except after a clean close | P0 |
| FR-WS-14 | `{{variable}}` substitution in URL and messages | P0 |
| FR-WS-15 | Custom headers and subprotocols, binary frames, save log, ping interval | P1 |

### 3.15 gRPC
| ID | Requirement | Pri |
|---|---|---|
| FR-GRPC-01 | Native gRPC over HTTP/2 with TLS (the legacy client supported only JSON over gRPC-Web proxies) | P0 |
| FR-GRPC-02 | Service and method selection; load services from a .proto file or server reflection | P0 |
| FR-GRPC-03 | JSON request editor mapped to protobuf; JSON response viewer; metadata editor; `{{variable}}` support | P0 |
| FR-GRPC-04 | Call types: unary (P0), server-streaming, client-streaming and bidirectional (P1) | P0 |
| FR-GRPC-05 | Deadline and cancellation for calls | P1 |

### 3.16 Request diff
| ID | Requirement | Pri |
|---|---|---|
| FR-DIFF-01 | Compare any two open requests: method, URL, headers (key/value) and body (line by line) | P1 |
| FR-DIFF-02 | Color-coded additions, removals and modifications with counts; copy diff; shortcut Ctrl/Cmd+D | P1 |

### 3.17 Mock server
| ID | Requirement | Pri |
|---|---|---|
| FR-MOCK-01 | Generate Express (JavaScript) and Hono (TypeScript) mock server code from a collection; copy or save to file | P1 |
| FR-MOCK-02 | Built-in local mock server run from the app using saved response examples | P2 |

### 3.18 Command palette and shortcuts
| ID | Requirement | Pri |
|---|---|---|
| FR-KEY-01 | Command palette on Ctrl/Cmd+K and Ctrl/Cmd+Shift+P with fuzzy search by label or category; arrow keys, Enter, Escape | P0 |
| FR-KEY-02 | Commands: New Request, navigation to every view, switch theme, switch environment, send, import, export | P0 |
| FR-KEY-03 | Global shortcuts: Send (Ctrl/Cmd+Enter), Save (Ctrl/Cmd+S), Diff (Ctrl/Cmd+D), Escape to dismiss | P0 |
| FR-KEY-04 | Cmd on macOS and Ctrl on Windows and Linux | P0 |
| FR-KEY-05 | Shortcuts help dialog; new tab, close tab, next tab, focus URL bar | P1 |
| FR-KEY-06 | Customizable keymap | P2 |

### 3.19 Settings and theming
| ID | Requirement | Pri |
|---|---|---|
| FR-SET-01 | Theme: light, dark or follow system | P0 |
| FR-SET-02 | Settings: default timeout, TLS verification, history cap, outbound proxy, data directory location | P0 |
| FR-SET-03 | No telemetry by default | P0 |
| FR-SET-04 | Font size and editor font; update-check opt-out | P1 |
| FR-SET-05 | High-contrast theme | P2 |

### 3.20 Persistence
| ID | Requirement | Pri |
|---|---|---|
| FR-PER-01 | Local-first and offline: tabs, requests, collections, environments, global variables, history and UI preferences are saved to disk | P0 |
| FR-PER-02 | Runtime-only state (in-flight responses, loading flags, cancel handles, palette open state) is not persisted | P0 |
| FR-PER-03 | Atomic writes and debounced autosave so a crash never corrupts data | P0 |
| FR-PER-04 | Schema version in every file, with automatic migration and a backup copy before migrating | P0 |
| FR-PER-05 | Data stored in each OS standard location (macOS Application Support, Windows AppData, Linux XDG data directory) | P0 |
| FR-PER-06 | Import of the JSON export from the legacy api-tester-kit | P1 |
| FR-PER-07 | Open an arbitrary folder as a workspace | P1 |

### 3.21 Distribution, installation and updates
| ID | Requirement | Pri |
|---|---|---|
| FR-DIST-01 | One-line install on macOS and Linux (`curl ... \| sh`) and on Windows (`irm ... \| iex`) | P0 |
| FR-DIST-02 | The installer detects OS and architecture, downloads the matching release asset, verifies its SHA-256 checksum, installs per user without admin rights, and adds a launcher or menu entry and PATH shim | P0 |
| FR-DIST-03 | Release assets: macOS arm64 and x86_64, Windows x64, Linux x86_64 and aarch64 | P0 |
| FR-DIST-04 | Single-command uninstall that optionally keeps user data | P0 |
| FR-DIST-05 | CI builds, tests and publishes all assets and checksums on every tagged release | P0 |
| FR-DIST-06 | In-app update check and self-update, with opt-out | P1 |
| FR-DIST-07 | macOS code signing and notarization; Windows Authenticode signing | P1 |
| FR-DIST-08 | Package managers: Homebrew, winget, Scoop, AUR, .deb, .rpm, AppImage, and cargo install | P1 |
| FR-DIST-09 | Windows arm64 build and macOS universal binary | P2 |

## 4. External interface requirements

### 4.1 User interface
Native GPUI window with a Zed-like feel and a clean, dense layout inspired by Linear and Raycast (the legacy design references). Three-pane layout: sidebar, request editor, response viewer. Platform conventions apply: macOS menu bar and Cmd shortcuts, native window controls on Windows and Linux. Light and dark themes share one set of design tokens.

### 4.2 Software and system interfaces
Windowing and GPU API per platform (Metal, Vulkan or DirectX through GPUI), clipboard, native file dialogs, OS keychain, system proxy settings, and the OS shell for launcher registration.

### 4.3 Communication interfaces
HTTP/1.1, HTTP/2, WebSocket (ws and wss), gRPC over HTTP/2, TLS 1.2 and 1.3.

### 4.4 File formats
Native collection files; Postman Collection v2.1; OpenAPI 3.x and Swagger 2.0; HAR 1.2; cURL command lines; export as JSON, YAML and Markdown.

### 4.5 Command-line interface
The binary may expose `api-tester run <collection>` for headless runs (FR-RUN-08, P2).

## 5. Non-functional requirements
Targets below are proposed and are validated by benchmarks in Phase 0 of the plan. Hard limits come from the product constraints; targets are the engineering goals.

| ID | Requirement | Hard limit | Target |
|---|---|---|---|
| NFR-RES-01 | Installed size on disk | 100 MB | 40 MB or less |
| NFR-RES-02 | Resident memory (RSS on macOS and Linux, private working set on Windows) | 250 MB under the heaviest supported workload | 120 MB or less idle with 5 tabs open |
| NFR-RES-03 | Data growth on disk is bounded by the history cap and body truncation | n/a | History database under 200 MB at the default cap |
| NFR-PERF-01 | Cold start to an interactive window | 1 s | 500 ms or less |
| NFR-PERF-02 | Frame rate while scrolling, typing and resizing; input latency | 30 fps min | 60 fps; input latency under 50 ms |
| NFR-PERF-03 | App-added overhead between Send and bytes on the wire (excluding scripts) | 20 ms | 5 ms or less |
| NFR-PERF-04 | Render a 1 MB JSON response | 300 ms | 100 ms or less |
| NFR-PERF-05 | Open and scroll a 10 MB JSON response; handle 50 MB without exceeding the memory limit (disk spill and partial view) | RSS stays under 250 MB | First paint under 500 ms at 60 fps |
| NFR-PERF-06 | Search a 10 MB body for the first match | 500 ms | 200 ms or less |
| NFR-PERF-07 | Sidebar with 5,000 requests opens and filters | 300 ms | 100 ms or less |
| NFR-PERF-08 | Runner executes 1,000 requests with flat memory | RSS stays under 250 MB | No growth after warm-up |

### 5.1 Reliability
No data loss on crash or power failure (atomic writes, autosave). Network and script errors are shown as messages and never panic the app. Crash reports are written to a local log only.

### 5.2 Security and privacy
- Scripts are sandboxed (FR-SCR-05).
- TLS verification is on by default; disabling it is explicit and per request or global.
- Secrets are masked in the UI, never written to logs, and excluded from exports by default.
- No telemetry and no network calls except user requests and the optional update check.
- The installer is served over HTTPS and verifies release checksums; dependencies are audited in CI.

### 5.3 Portability
Feature parity on macOS, Windows and Linux; one native binary with no runtime dependency.

### 5.4 Usability and accessibility
Keyboard-first: every action is reachable from the command palette. Visible focus indicators and full keyboard navigation. Screen-reader support is limited by what GPUI currently exposes (section 9).

### 5.5 Maintainability
Cargo workspace with a UI-independent core crate. Unit tests for the engine, variable resolution, import/export, code generation and scripting (the legacy project had tests in these areas), integration tests for the request flow, and CI on all three operating systems.

## 6. Data requirements

### 6.1 Entities
| Entity | Key fields |
|---|---|
| KeyValue | id, key, value, description, enabled |
| Request | id, name, method, url, params[], headers[], body, auth, preRequestScript, testScript, collectionId, folderId |
| Body | type (none, json, xml, html, text, urlencoded, form-data, binary), raw text, urlencoded[], form-data[] (text or file) |
| Auth | type (none, basic, bearer, jwt, apikey, custom, oauth2, digest) with a type-specific block |
| Response | status, statusText, headers, body (or disk reference), time, size, timestamp |
| Collection | id, name, description, variables, folders[], requests[], createdAt, updatedAt |
| Folder | id, name, requests[], folders[] (recursive) |
| Environment | id, name, variables, secret flags, createdAt, updatedAt |
| HistoryEntry | id, request snapshot, response summary, environment name, timestamp |
| Tab | id, requestId, name, dirty, pinned |
| Settings | theme, sidebar width, timeout, TLS verify, proxy, history cap, data directory |

### 6.2 Proposed storage layout
Settings and session in small files, collections and environments as readable files, history in an embedded database:

```
<data-dir>/
  settings.toml
  session.json            open tabs, active tab, pane sizes
  collections/<name>/     collection file, folders, one file per request
  environments/<name>.toml
  history.db              capped, bodies truncated and compressed
```

## 7. Traceability to the legacy project
| Legacy module | SRS section | Disposition |
|---|---|---|
| 02 Types and utils | 6 | Carried over as Rust data models |
| 03 Stores (request, environment, collection, history, UI) | 3.1, 3.4, 3.7, 3.8, 3.19, 3.20 | Re-implemented as GPUI entities backed by files |
| 04 UI components (sidebar, palette, dialogs, toasts, empty states, error boundary) | 3.1, 3.18, 5.1 | Carried over; error boundary becomes error handling |
| 05 API engine and proxy | 3.5 | Proxy removed; native client |
| 06 Request builder | 3.2, 3.3 | Improved: form bodies, OAuth, per-request settings |
| 07 Response viewer | 3.6 | Improved: virtualization, disk spill |
| 08 Code generation | 3.10 | Carried over, more languages |
| 09 Import and export | 3.9 | Improved: OpenAPI, Postman export |
| 10 Scripting | 3.11 | Carried over; engine replaced by an embedded one |
| 11 Collection runner | 3.12 | Improved: runs scripts, iterations, reports |
| 12 Persistence (IndexedDB) | 3.20 | Replaced by files and an embedded database |
| 13 Keyboard shortcuts | 3.18 | Carried over |
| 15 GraphQL | 3.13 | Carried over; adds introspection |
| 16 WebSocket | 3.14 | Carried over |
| 17 gRPC | 3.15 | Improved: native HTTP/2 and protobuf |
| 18 Testing | 5.5 | Carried over as Rust tests |
| 19 Request diff | 3.16 | Carried over |
| 20 Mock server | 3.17 | Carried over; built-in mock is P2 |
| 21 Environment quick-edit | 3.4 | Carried over |
| 22 Docker deployment | 3.21 | Replaced by native one-command installers |
| Settings page (theme, proxy) | 3.19 | Carried over |

## 8. Gaps found in the legacy project and the decision for each
| Legacy gap | Decision |
|---|---|
| Form-data and URL-encoded bodies were marked coming soon | Required in 1.0 (FR-RB-06) |
| OAuth2, digest and AWS auth were listed as types but not implemented | Scheduled as P1 and P2 (FR-AUTH-03 to 05) |
| OpenAPI import handled only query parameters and five methods | Improved in FR-IE-05 |
| Ctrl+S was a placeholder | Real save in FR-RB-11 |
| gRPC worked only as JSON over a gRPC-Web proxy | Native gRPC in FR-GRPC-01 |
| GraphQL had no introspection and its state was not persisted | FR-GQL-05, and saved queries in FR-GQL-06 |
| History entries embedded full response bodies | Truncated and compressed (FR-HIS-02) |
| Per the legacy docs, the runner only sent requests | Scripts and assertions run in the runner (FR-RUN-05) |
| Ctrl+D for diff appeared in the diff doc but not in the shortcut list | Listed in FR-KEY-03 |

## 9. Open questions and assumptions
1. **GPUI platform support:** confirm minimum macOS and Windows versions, Linux windowing requirements, GPU fallback behavior, and current API stability before Phase 1.
2. **Script engine:** JavaScript is required for compatibility with existing scripts. The engine is chosen in Phase 0 by size, speed and memory benchmarks.
3. **Accessibility:** confirm what GPUI exposes for screen readers and plan around the gap.
4. **Collection file format:** TOML versus JSON for request files, decided in Phase 0.
5. **Installer hosting:** repository, release host and install domain are not yet decided, so install commands use placeholders in the plan.
6. **License** of the application.
7. **Mock server:** whether code generation (P1) is still wanted once the built-in mock (P2) exists.
