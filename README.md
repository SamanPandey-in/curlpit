# Curlpit

A native, fast API client for people who live in Git. Built in Rust on [GPUI](https://www.gpui.rs/) (the UI framework behind the Zed editor), it stores collections as plain TOML files, one per request, and has no account, no cloud sync and no telemetry.

<!-- Screenshot or GIF goes here once one exists: ![Curlpit](docs/user/img/curlpit.png) -->

> **Status: early, pre-release.** Curlpit is usable day to day for REST work, but there are no installers yet and some things a Postman user expects are still missing. See [What it does not do yet](#what-it-does-not-do-yet). The binary is still called `api-tester` and the window title reads "API Tester".

## Why

- **Native and fast.** It's a single binary with no Electron and no embedded browser. It opens in a fraction of a second and stays around 70 MB idle (see [Performance](#performance)).
- **Git-friendly.** A collection is a folder, and each request is a small, readable TOML file. Diffs are clean, merges are sane, and you can edit requests in your text editor.
- **No account.** Everything lives in a local data directory. Nothing is sent anywhere except the requests you send.
- **Postman-compatible where it counts.** It imports Postman v2.0/v2.1 collections, environments and globals, runs the common `pm.*` script API, and exports back to Postman.

## Features (what works today)

**Requests**
- Methods GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, TRACE and CONNECT; Params, Headers, Body, Auth, Scripts and Settings tabs.
- Bodies: raw JSON / XML / HTML / Text, `x-www-form-urlencoded`, and multipart form-data with file fields. GraphQL bodies (from imports) are sent as JSON and shown read-only.
- Auth: Basic, Bearer token and API key (header or query) can be edited. JWT and custom-header auth from imported files are kept and sent. OAuth 2.0 and Digest are stored but can't be sent yet.
- Per-request timeout, follow-redirects and TLS-verification settings.
- Several request tabs at once, each sending independently. Cancel an in-flight request; reopen a closed tab.
- Session restore ("hot exit"): open tabs, unsaved edits included, come back after a restart.

**Responses**
- Status, time and size; body and headers. JSON is pretty-printed, and there is a collapsible **JSON tree** with copy-path and copy-value.
- Big bodies (over 10 MB) spill to disk and stay smooth to scroll; 50 MB was measured at about 130 MB of RAM.
- Find in response, soft wrap, copy the body, and save the body to a file.

**Organising**
- Collections and folders on disk (see [collections on disk](docs/user/collections-on-disk.md)).
- Environments and globals with `{{variables}}` in the URL, params, headers, body and auth. Secret values are masked in the editor.
- History of every request sent from a tab, searchable, stored in SQLite. Collection runs are not recorded.
- A cookie jar shared by all tabs and the runner, with a Cookies page to inspect and delete cookies.

**Scripts and runs**
- Pre-request and test scripts in a sandboxed JavaScript engine (QuickJS), with a Postman-compatible `pm` subset: `pm.test`, `pm.expect`, `pm.response`, `pm.environment` and so on (see the [scripting reference](docs/user/scripting.md)).
- A collection runner with iterations, a delay between requests, stop on first failure, and a live report.

**Import, export, code**
- Import: Postman collections (v2.0, v2.1), environments and globals, HAR files, Curlpit's own JSON, and cURL commands from the clipboard. You can import by file picker, drag and drop, or paste.
- Export: Postman v2.1 collections and environments (secrets left out unless you opt in), and Curlpit JSON.
- Code generation: cURL, Python, JavaScript, TypeScript, Go, PHP and Rust, generated from the exact request that Send would send. There is also Copy as cURL.

**Workspace**
- A command palette (`Ctrl+K` / `Cmd+K`) and keyboard shortcuts (see [shortcuts](docs/user/shortcuts.md)).
- Light and dark themes that follow the system, based on Zed's One theme.
- A Settings page: default timeout, TLS verification, history size, and an outbound proxy.
- A first-run sample: on a fresh install, a "Getting started" collection and an `httpbin` environment are created, so the first Send works right away.

## Install

Prebuilt binaries and installers for macOS, Windows and Linux are coming with the first release. The release pipeline exists, but nothing has been published yet. For now, build from source.

1. Install Rust stable with [rustup](https://rustup.rs). The repo's `rust-toolchain.toml` selects the stable toolchain for you.
2. Install the platform prerequisites:
   - **macOS:** the Xcode Command Line Tools (`xcode-select --install`). The full Xcode app is not needed.
   - **Windows:** the MSVC toolchain ("Desktop development with C++" in the Visual Studio Build Tools).
   - **Linux (Debian/Ubuntu):** a C toolchain plus the GUI libraries CI uses:
     ```sh
     sudo apt-get install -y build-essential pkg-config \
       libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
       libx11-xcb-dev libxcb1-dev libfontconfig1-dev libfreetype-dev
     ```
3. Build and run:
   ```sh
   git clone <this repo> curlpit && cd curlpit
   cargo build -p at-app --release
   ./target/release/api-tester        # Windows: target\release\api-tester.exe
   ```

The first build compiles GPUI and takes several minutes. The release binary is about 14 MB.

## 60-second quickstart

1. Start the app. On a fresh install, the sidebar shows **Getting started**, and the title bar shows the **httpbin** environment as active.
2. Click **1. GET with query params**, then press **Send** (or `Ctrl+Enter` / `Cmd+Enter`). httpbin echoes your query back in `args`.
3. Open **4. Test script** and send it. The response's **Tests** tab shows two passing checks.
4. Make your own: press `Ctrl+T` / `Cmd+T` for a new tab, type a URL (`{{baseUrl}}/uuid` works), press Enter, then `Ctrl+S` / `Cmd+S` to save it.
5. Coming from Postman? Drop an exported collection onto the window, or use **Import…** at the bottom of the sidebar.

There is more detail in [docs/user/quickstart.md](docs/user/quickstart.md).

## Performance

Measured on a MacBook Air M5 (macOS 26.6), from [docs/BASELINE.md](docs/BASELINE.md):

| What | Budget | Measured |
|---|---|---|
| Release binary, whole app | ≤ 40 MB | **14.4 MB** |
| First render after launch (Phase 0 empty-window spike) | ≤ 500 ms | **347 ms** (84 ms on a warm second launch) |
| Idle memory (Phase 0 empty-window spike) | ≤ 120 MB | **70.1 MB** |
| 10 MB response in the virtualized viewer: first render / RAM while scrolling | ≤ 500 ms / ≤ 250 MB | **108 ms / 84.2 MB**, flat |
| 50 MB response in the viewer: RAM | ≤ 250 MB | **130.5 MB** |
| Search a 10 MB body, worst case (release build) | ≤ 500 ms | **6.4 ms** |
| Pretty-print 10 MB of JSON | — | **11.96 ms** |
| Build a request (params, headers, basic auth) | < 5 ms | **1.37 µs** |
| Script engine start (QuickJS) | small | **0.58 ms** |

The launch time and idle-memory rows come from the Phase 0 GPUI spike, not the finished app. The viewer rows come from the viewer spike. The search and pretty-print figures use synthetic JSON and leave out rendering. `docs/BASELINE.md` has the full table, the caveats, and how to reproduce each number.

## Where your data lives

Everything is in one data directory:

| OS | Default location |
|---|---|
| macOS | `~/Library/Application Support/dev.api-tester.API-Tester/` |
| Windows | `%APPDATA%\api-tester\API Tester\data\` |
| Linux | `$XDG_DATA_HOME/apitester/` (usually `~/.local/share/apitester/`) |

Set **`API_TESTER_DATA_DIR`** to use another folder instead, for a throw-away test profile, a portable install, or a collection folder kept in a Git repo:

```sh
API_TESTER_DATA_DIR=/tmp/curlpit-test ./target/release/api-tester
```

The Settings page shows which directory is in use. The layout inside it is described in [collections on disk](docs/user/collections-on-disk.md).

## What it does not do yet

These gaps are honest and tracked in [GAPS-AND-ROADMAP.md](GAPS-AND-ROADMAP.md):

- No installers or auto-update. You build from source.
- No OAuth 2.0 or Digest sending, and no auth inheritance from folders or collections. Imports flatten inherited auth and scripts into each request.
- Collection and folder settings (`collection.toml`, `folder.toml`) are stored but not applied when sending.
- No WebSocket, gRPC, SSE, or GraphQL editor.
- No `pm.sendRequest`, no `require()` libraries, and no `pm.cookies` in scripts.
- In the app, you can't rename folders, delete or move requests, or create folders from the sidebar. Use your file manager or editor instead. The sidebar re-reads the disk after any save or import, and on start.
- No Insomnia, OpenAPI or Bruno import.
- No command-line runner yet.

## Project layout

A Cargo workspace. Library crates are UI-free and covered by plain `cargo test`; the GUI crate builds separately.

| Crate | Package | What it does |
|---|---|---|
| `crates/core` | `at-core` | Data models, `{{variable}}` resolution, request building, JSON tree index, line index and search |
| `crates/http` | `at-http` | HTTP engine (reqwest + rustls): cancel, timeouts, disk spill for big bodies, cookie jar, client cache |
| `crates/store` | `at-store` | TOML storage for collections, environments, globals, settings and session; first-run sample |
| `crates/history` | `at-history` | Request history in SQLite |
| `crates/interop` | `at-interop` | Import and export: Postman, HAR, native JSON |
| `crates/codegen` | `at-codegen` | Code snippets and cURL import |
| `crates/script` | `at-script` | Sandboxed QuickJS and the `pm` API (`prelude.js`) |
| `crates/runner` | `at-runner` | The send pipeline (script → build → send → tests → history) and the collection run loop, without a UI |
| `crates/app` | `at-app` | The GPUI desktop app (binary `api-tester`) |
| `crates/demo` | `core-demo` | A small CLI harness for sending a request JSON file during development |
| `benches` | `at-benches` | criterion benchmarks and budget gates |
| `spikes/` | — | Phase 0 throw-away prototypes (a separate workspace) |

## Documentation

- **User guides:** [quickstart](docs/user/quickstart.md), [migrating from Postman](docs/user/migrating-from-postman.md), [scripting reference](docs/user/scripting.md), [keyboard shortcuts](docs/user/shortcuts.md), [collections on disk](docs/user/collections-on-disk.md)
- **Contributing:** [CONTRIBUTING.md](CONTRIBUTING.md)
- **Design:** [requirements (SRS)](docs/SRS.md), [plan](docs/PLAN.md), [decision log](docs/DECISIONS.md), [performance baseline](docs/BASELINE.md), [gaps and roadmap](GAPS-AND-ROADMAP.md)
- **Feature behaviour and manual test passes:** [docs/testing/](docs/testing/)

## License

MIT OR Apache-2.0, at your option (as declared in `Cargo.toml`).
