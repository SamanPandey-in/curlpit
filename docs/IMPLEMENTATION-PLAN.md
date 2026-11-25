# Implementation plan for GAPS-AND-ROADMAP.md

Status: draft, 23 November 2026. Source: `GAPS-AND-ROADMAP.md` (gap IDs G-01…G-22, sections §4–§6).
This plan turns that guide into waves of parallel agent work on top of `main` at `1913f4f`.

---

## 1. What the guide got right, and what it got wrong

The guide is a static review; it did not build the app. I checked its claims against the code.

| Claim | Verified | Note |
|---|---|---|
| No GraphQL/WebSocket/gRPC/SSE; `MainView` has 5 views | ✅ | `crates/app/src/sidebar.rs:66` |
| Auth has 6 variants, no OAuth/Digest/inheritance | ✅ | `crates/core/src/models.rs:105` |
| Collection scope exists but is unwired | ✅ | `crates/core/src/vars.rs` |
| Client built per request | ✅ | `crates/http/src/lib.rs:106` |
| `pm.sendRequest` throws | ✅ | `crates/script/src/prelude.js:328` |
| Runner capped at 100 iterations, no data files | ✅ | `crates/app/src/workspace/runner.rs:65` |
| No OpenAPI import | ✅ | `crates/interop/src` has HAR, native JSON, Postman only |
| CI skips the GUI crate, no release workflow | ✅ | `.github/workflows/ci.yml` |
| README is stale ("not yet compiled") | ✅ | |
| Extracting `execute()` is an **M** refactor | ❌ | `execute()` in `workspace/send.rs:60` already uses no GPUI types; moving it is **S** |
| Licence is an open question | ❌ | `Cargo.toml` already declares `MIT OR Apache-2.0` |

---

## 2. Triage: what is implementable

### 2.1 Build it: agents can implement and test these here
Covers G-01 to G-17, G-19 to G-22, and from §4–§6:
- the CLI, mock server, MCP server and GitHub Action
- docs generation and the linter
- local monitors
- workspaces and the Git panel
- the environment guard, undo, the response filter and click-to-extract
- the first-run sample workspace

### 2.2 Build it after you decide
| Item | Decision needed |
|---|---|
| AI v1 (explain failure, generate tests, natural-language builder) | Which providers: local only (Ollama), bring-your-own key, or both |
| Rename `api-tester` → `curlpit` (binary, env vars, data dir) | Yes or no. The data dir moves, so it needs a one-time migration |
| Release workflow run on GitHub | Permission to push branches and tags so the workflow can actually run |

### 2.3 Agents can write the config, but you have to act
These need accounts, certificates or other machines:
- code signing and notarisation
- Homebrew, winget and Scoop submissions
- benchmarks against Postman, Bruno and Insomnia
- real macOS and Linux verification (CI only proves those builds compile)

### 2.4 Not planned now (the guide's 2.0 "platform")
- self-hosted sync server, roles, SSO/SCIM and audit log
- live sessions and CRDT co-editing
- Flows canvas
- load testing
- notebooks
- WASM plugin system
- hosted docs and API network
- visualizer

These conflict with the local-first core or need a service to run; revisit after 1.2.

---

## 3. How the parallel work is run

- **One agent per work package**, each in its own git worktree on `feat/<pkg-id>`. Agents never commit to `main`.
- **File ownership.** Each package lists the files it owns. Shared seams (model enums, view enums, crate wiring) are cut once in Wave 0, so parallel agents mostly add new files and one-line hookups.
- **Agents must not edit `DECISIONS.md`.** Parallel numbering would collide, so each agent puts proposed decisions in its final report and the integrator appends them as D46+.
- **Integrator (main session).** Merges branches one at a time, resolves the small enum and registration conflicts, then runs the gates:
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets -D warnings`, for the default members and for `-p at-app`
  - `cargo test --workspace` and `cargo test -p at-app`
  - `cargo build -p at-app`
- **UI QA runs one at a time.** The OCR/mouse harness drives the real screen, so two agents cannot use it at once. Agents verify logic with unit tests and the `at-app` build. The integrator runs the harness on each wave's UI features after merging and fixes what it finds, as in the last test pass.
- **Commits** use Conventional Commits, one or more per package, with no AI trailer.
- **Size.** 5–7 agents per wave, inside the configured guideline of fewer than 10.

Each agent brief contains:
- the goal and gap IDs
- the files it owns and the seams it may touch
- the required tests: TDD for pure crates, at minimum a build for UI
- what "done" means
- a final report: changes, proposed decisions, open issues and untested items

---

## 4. Waves

Effort uses the guide's scale: S is days, M is 1–2 weeks, L is 3+ weeks of one developer. With parallel agents, a wave's wall time is roughly its largest package plus integration.

### Wave 0: Foundation (sequential, 1 agent or the integrator)
Everything later depends on this, so it lands first and alone.

| ID | Work | Gaps | Files |
|---|---|---|---|
| W0.1 | Create `crates/runner` (`at-runner`). Move these out of the app crate: `execute()`, `history_record`, the script glue from `app/src/script.rs`, and the non-UI run loop from `app/src/runner.rs`. The app depends on `at-runner`. No behaviour change, and the existing tests still pass | G-12 prerequisite | new crate, `app/src/workspace/send.rs`, `app/src/runner.rs`, `app/src/script.rs` |
| W0.2 | Client cache: `Arc<ClientCache>` keyed on TLS, redirect, proxy and cert settings, with the timeout set per request. Add a benchmark for "second request to the same host" | G-10 | `crates/http`, `benches` |
| W0.3 | Model seams in `at-core`, all `serde(default)` and backward compatible, plus round-trip tests for old files. Variants not built yet make `build()` return a clear "not supported yet" error, never a panic. Additions: `Request.uid`; `Auth::{Inherit, OAuth2{..}, Digest{..}}`; `Body::GraphQl{query, variables}`; `CollectionMeta` and `FolderMeta` (variables, headers, auth, scripts); `Example` | G-02, G-03, G-08, G-01 seams | `crates/core/src/models.rs`, `crates/store` |
| W0.4 | CI job that builds and tests `-p at-app` on all three OSes | G-19 (part) | `.github/workflows/ci.yml` |

Exit criteria:
- all gates are green
- the app behaves exactly as before (a short harness smoke run)
- old session, collection and settings files still load

### Wave 1: Release 0.9, "Installable and honest" (5 agents in parallel)

| Agent | Package | Gaps | Owns |
|---|---|---|---|
| A1 | Release pipeline: a `cargo-dist` (or hand-written) workflow for Windows x64, macOS arm64 and x64, and Linux x64 and aarch64; checksums; `install.sh` and `install.ps1`; a nightly budget job (binary size, cold start, RSS → JSON artefact) that fails on a regression over 10% | G-18, G-19 | `.github/workflows/*`, `dist` config, `scripts/` |
| A2 | README rewrite (what, why, screenshot, install, 60-second quickstart, measured performance); `docs/user/` (quickstart, migrating from Postman, scripting reference, shortcuts); `CONTRIBUTING.md`; first-run sample workspace seeded into an empty data dir | G-22, §6.1 | `README.md`, `docs/user/`, sample seeding in `crates/store` plus a small app hook |
| A3 | Dynamic variables in `at-core` (`$uuid`, `$timestamp`, `$isoTimestamp`, `$randomInt`, `$randomEmail`…), pure and TDD; `{{var}}` highlighting in the URL field; `{{` autocomplete showing value and scope; "create" on an unresolved variable | G-13, G-21 (D21) | `crates/core/src/vars.rs`, new `app/src/var_complete.rs`, URL field hookup |
| A4 | Environment split `env.toml` / `env.local.toml` with an automatic `.gitignore` entry; import trust prompt ("this collection contains N scripts"); TLS options (custom CA, PEM and PKCS#12 client certs, TLS floor) in Settings and per request | G-14, G-16, G-17 | `crates/store` (env files), `crates/interop` (script count), `crates/http` (TLS), the settings page |
| A5 | Headless CLI `crates/cli`, the `curlpit` binary: `curlpit run <dir> --env <name> --iterations N --data file.csv\|json --reporter junit\|json\|html --out <file> --bail`. Runs on `at-runner`; adds data-file iteration with `pm.iterationData`; removes the 100-iteration cap in the core loop, keeping the result list memory-flat; JUnit, JSON and HTML reporters; exit code reflects failures | G-12, G-05 (headless half) | new `crates/cli`, `crates/runner` (data and report modules) |

Exit criteria:
- the CLI runs `examples/` against the local echo server and writes valid JUnit
- the release workflow passes `actionlint` and a dry run
- the README quickstart works on a clean data dir

### Wave 2: Release 1.0, "A real Postman replacement for REST" (7 agents in parallel)

| Agent | Package | Gaps | Owns |
|---|---|---|---|
| B1 | OpenAPI 3.x and Swagger 2 import (tags → folders, `servers` → `{{baseUrl}}`, examples → bodies, security schemes → auth); Insomnia v4 and Bruno `.bru` importers; a compatibility report model ("212 requests, 3 scripts need review"); a preview-before-import screen | G-04, §6.1 | `crates/interop` (new modules), new `app/src/workspace/render/import_preview.rs` |
| B2 | OAuth 2.0 (authorization code with PKCE via a loopback redirect on a random port; client credentials, password and device code; automatic refresh; a token store showing expiry; header or query injection); Digest (401 challenge round-trip in `at-http`); AWS SigV4 (stretch); the auth UI for these | G-02 | `crates/http/src/auth/*`, new `crates/core` auth resolution, `app` auth editor |
| B3 | Collection and folder files (`collection.toml` / `folder.toml`). Resolution walks request → folder → collection for variables, headers, auth (`Auth::Inherit`) and scripts. A collection settings page and an "inherited from …" chip. The Postman importer keeps folder-level data instead of flattening it | G-03 | `crates/store`, `crates/core` (resolve), `crates/interop/postman*`, new app page |
| B4 | Scripting: `pm.sendRequest` (callback and promise) via a host callback into `at-runner`, with a byte budget; `crypto` (HMAC and SHA), `uuid`, `btoa`/`atob`, `Buffer`; `pm.cookies`, `pm.info`, `pm.iterationData`; `pm.execution.setNextRequest` (runner flow control); async tests; a console panel in the app | G-11 | `crates/script`, `crates/runner`, new `app` console panel |
| B5 | GraphQL body mode with variables, plus a schema explorer driven by introspection. SSE: a streaming response API in `at-http`, an incremental viewer and a Stop button | G-01 (part), G-06 (SSE) | `crates/http` (streaming), new `app/src/graphql.rs`, viewer hookup |
| B6 | Keychain secrets: `keyring`, `secret://` references in TOML, migration of existing plain secrets with a backup, and a scanner warning on token-like literals in saved requests | G-15 | `crates/store/src/secrets.rs`, environment and auth save paths |
| B7 | Runner UI: data-file picker, uncapped iterations with a virtualised result list, report export, running unsaved tab edits, drag-to-reorder (order stored in `folder.toml`, so it follows B3) | G-05 (UI half) | `app/src/runner.rs`, `app/src/workspace/runner.rs` |

Ordering inside the wave:
- B3 merges before B7's reorder step and before B2's inheritance tests.
- B4's `sendRequest` needs W0.1.
- Everything else is independent.

Exit criteria:
- a public Postman test collection imports and passes
- the OAuth code flow works against a local mock identity provider
- old data still loads
- a UI harness pass covers B1, B2, B3, B5 and B7

### Wave 3: Release 1.1, "Better than Postman for developers" (7 agents in parallel)

| Agent | Package | Gaps | Owns |
|---|---|---|---|
| C1 | WebSocket: new `crates/protocols` (`tokio-tungstenite`), a connection view, a message log on the virtualised viewer, saved messages and auto-reconnect | G-01 | `crates/protocols/ws`, new app view |
| C2 | gRPC: `tonic` plus `prost-reflect`, from server reflection or `.proto` files; unary first, then server streaming | G-01 | `crates/protocols/grpc`, new app view |
| C3 | Saved examples ("save response as example" writes `examples/*.toml`). Local mock server (`axum`): method and path matching, latency and error injection, a Mocks view and `curlpit mock` | G-08, G-09 | `crates/store` (examples), new `crates/mock`, CLI subcommand, Mocks view |
| C4 | Diff and compare (`similar`): request vs request, response vs a previous response from History, response vs a saved example, and environment vs environment | G-07, §6.2 | new `crates/diff`, new app Diff view |
| C5 | Response power tools: a JSONPath filter; click-to-extract a tree node into a variable or an assertion; infer a JSON schema and validate later responses against it; a Timeline tab with the redirect chain and the request as sent (timing split via a `hyper` connector if feasible, otherwise total time plus TTFB) | FR-RES-10, G-06 | `crates/http` (timings, redirects), `app/src/tree.rs`, new Timeline tab |
| C6 | Agent surface: `curlpit mcp` (collections, environments without secrets, run, history), CLI `--json` output, an agent skill file, and a composite GitHub Action using the CLI | §5.3, §6.4 | `crates/cli`, new `crates/mcp`, `action.yml` |
| C7 | Safety and UX debts: an environment guard (a production tag triggers a banner and a confirm before mutating methods); undo for deletes and edits; a virtualised sidebar for 5,000 requests; tree context menus (duplicate, copy as cURL, run folder, reveal in Explorer); a live history-cap setter | §5.4, G-21 | `app/src/sidebar.rs`, `workspace/collections.rs`, `workspace/environments.rs` |

Exit criteria:
- the mock server serves saved examples to the CLI runner in CI
- an MCP client can list and run a request
- the sidebar stays smooth with 5,000 requests, measured with the harness
- a UI harness pass over each new view

**AI v1 slots in here** as C8 once the provider decision is made:
- explain a failure, generate tests (shown as a diff before applying) and a natural-language builder
- redaction on by default, a "this will be sent" preview, and a per-workspace toggle

### Wave 4: Release 1.2, "Team ready" (5 agents in parallel)

| Agent | Package | Owns |
|---|---|---|
| D1 | Workspaces: `workspace.toml` and a switcher in the title bar (FR-PER-07) | `crates/store`, title bar |
| D2 | Git panel (`gix`): modified badges in the tree, commit, pull and push, a per-request conflict view using the C4 diff, a "Create PR" link, and per-request version history | new `crates/vcs`, app Git panel |
| D3 | Docs generation: `curlpit docs build` writes a static site from collections, examples and codegen snippets | new `crates/docs`, CLI |
| D4 | Linter and governance: rules for missing tests, hard-coded hosts, literal secrets and missing examples; available in the CLI and as a Problems panel in the app | new `crates/lint`, CLI, app panel |
| D5 | Local monitors: `curlpit monitor add --every 15m`, a SQLite results store, a Monitors view (uptime, p50/p95), alerts by webhook, Slack, SMTP and desktop notification, and flaky-request detection | new `crates/monitor`, CLI, app view |

Exit criteria:
- a two-clone Git round-trip shows the conflict view
- docs build for `examples/`
- the linter runs in CI on `examples/`

---

## 5. Dependencies

```
W0.1 at-runner ──► A5 CLI ──► C3 mock CLI, C6 MCP/Action, D3 docs, D4 lint, D5 monitors
               └─► B4 sendRequest / setNextRequest
W0.3 model seams ──► B2 auth, B3 inheritance, B5 GraphQL, C3 examples
B3 folder files ──► B7 run order, D1 workspaces
C3 examples ──► C4 diff vs example, D3 docs
C4 diff ──► D2 Git conflict view
```

## 6. Risks

| Risk | Mitigation |
|---|---|
| Parallel edits to the app crate's shared enums and render dispatch | Seams cut in Wave 0; the integrator merges serially; each new view goes in its own file |
| Agents can't fully verify UI (one screen) | Unit tests plus the build per agent; a serial harness QA pass by the integrator after each wave |
| GPUI 0.2 / gpui-component 0.5 churn (D14/R1) | Pin the versions for the whole plan; no upgrades mid-wave |
| macOS and Linux unverified | The W0.4 CI matrix builds all three; real runs need you or a VM |
| Data-format changes break user data | Every model change is `serde(default)`, has round-trip tests for old files, and migrations back up first |
| Scope creep in L-sized items (gRPC, Git panel, OAuth) | Each has a "stretch" tail that can slip to the next wave without blocking the exit criteria |
