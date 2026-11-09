# 11 — Phase 3e: import / export (Postman)

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Import…** (bottom of the Collections sidebar, and on the Variables page): pick a Postman file. The kind is detected automatically:
  - **Collection** (v2.0 / v2.1) → a new collection on disk. Folders keep their real names (`User Management`, not a slug). Importing the same file twice makes a second collection; nothing is ever merged or overwritten.
  - **Environment** → a new environment (a clashing name gets ` 2`). Secret-type values stay marked as secret.
  - **Globals** → merged into your globals (same key replaces, new keys are added).
- What converts: method, URL, query params (moved into Params, so they aren't sent twice), headers, descriptions, enabled/disabled flags, bodies (raw JSON/XML/HTML/text, urlencoded, form-data including file fields), auth (none, basic, bearer, API key; folder/collection auth is inherited down into each request), follow-redirects and SSL-verify settings. GraphQL bodies become plain JSON (`{"query", "variables"}`).
- What doesn't (yet), always listed as **notes** in the status line: pre-request/test scripts, collection variables, OAuth2/Digest/AWS and other auth types, binary file bodies, and path variables (`:id` is kept as text). Postman v1 files get a clear "export it as v2.1" message.
- **Export…** (Collections sidebar): exports the collection you're working in, meaning the one holding the active tab's request or the collection/folder you last clicked, as Postman v2.1 JSON via a save dialog that starts in `~/Downloads`.
- **Export…** (Variables page): exports Globals or the selected environment as a Postman environment file. **Include secrets: off** (default) leaves secret values empty, and the status line says how many were left out; turn it **on** to include them.
- Sample files to test with: `examples/postman/httpbin.postman_collection.json` and `examples/postman/httpbin.postman_environment.json`.
- Fixed from 3d: the history file now lives at `<data-dir>/history.db` as documented (it was inside `collections/`).
- **Fixed (older bug, made worse by imports):** opening a request and saving it used to silently drop its auth, settings and urlencoded/form-data body, because the editor has no fields for them yet. Each tab now keeps those parts from the loaded request; they're saved and sent. A grey line next to the request tabs says what's being kept ("Also sent: Basic auth …"). An auth editor and form-body editor are still to come.

**Not yet:** cURL import (comes with code generation), Insomnia/OpenAPI import, exporting one folder or one request, importing into an existing collection, converting Postman scripts.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 75 passed (31 core + 5 http + 16 store + 11 history + 12 interop)
cargo test -p at-app            # expect 3 passed
API_TESTER_DATA_DIR=/tmp/at-test5 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| I1 | Sidebar (Collections) | **Import…** and **Export…** buttons at the bottom |
| I2 | **Import…** → `examples/postman/httpbin.postman_collection.json` | status "Imported "httpbin sample": 7 request(s), 2 folder(s). 4 note(s): …" listing the scripts (since 3j: "scripts on 1 collection/folder level(s) were copied into each request", so every request gets the collection's `console.log('hi')` pre-request script), the collection variable, oauth2, path variables; sidebar shows **httpbin sample** with folders **Auth** and **Basics** |
| I3 | `ls -R /tmp/at-test5/collections/httpbin-sample` | folders `Auth/`, `Basics/` (real names) and one `.toml` per request |
| I4 | Open **Get with query** | URL `{{base}}/get` (no `?…`); Params: `limit=10` (on), `tag=dog` (off); Headers: Accept on, X-Disabled off |
| I5 | `cat` the **Bearer (inherited)** file | `[request.auth]` `type = "bearer"`, `token = "{{token}}"` (inherited from the collection) |
| I6 | `cat` **Post JSON** | no auth (`noauth` overrides the collection); JSON body as in the file |
| I7 | **Import…** → `httpbin.postman_environment.json` | "Imported environment "httpbin" with 2 variable(s)"; Variables… shows it; `token` is marked Secret (masked) |
| I8 | Make **httpbin** active, open **Get with query**, Send | 200 from httpbin.org; the response's `args` contains only `limit=10` |
| I9 | Open **Basic auth** | **Auth (Basic)** tab with `user` / `passwd` (since 3g; before that a grey "Also sent: Basic auth" line); Send → 200 `"authenticated": true` |
| I9b | Open **Bearer (inherited)**, Send (env `httpbin` active) | 200, `"token": "sample-token-123"` |
| I9c | Open **Form urlencoded** | **Body (urlencoded)** with rows `a=1` (on) and `b=2` (off); Send → httpbin echoes `"form": {"a": "1"}` |
| I9d | In **Basic auth**, change the URL slightly, **Save**, then `cat` its file | `[request.auth]` with `type = "basic"` is still there (it used to be dropped on save) |
| I10 | Import the same environment again | a second one named **httpbin 2** |
| I11 | Import the same collection again | a second collection (`httpbin-sample-2/`); the first is untouched |
| I12 | Click **httpbin sample** in the sidebar, **Export…** | save dialog in Downloads suggesting `httpbin-sample.postman_collection.json`; after saving: "Exported collection "httpbin sample" to …" |
| I13 | Import that exported file | a third collection with the same requests, folders, params, headers, bodies |
| I14 | (If you have Postman) import the exported file into Postman | opens cleanly; requests look the same |
| I15 | Variables… → **httpbin** → **Export…** (Include secrets: off) | file saved; note "1 secret value(s) left empty…"; `cat` shows `"token"` with `"value": ""` |
| I16 | Turn **Include secrets: on**, Export again | `cat` shows `sample-token-123` |
| I17 | Variables… → **Globals**, add `g1=a`, **Export…** | `globals.postman_globals.json` with `"_postman_variable_scope": "globals"` |
| I18 | Change `g1` to `b` and add `g2`, Save, then **Import…** the file from I17 | "Imported global variables: 2 in total, 1 replaced"; `g1` is back to `a`, `g2` kept |
| I19 | Edit a variable on the Variables page without saving, then Import an environment | your edit is saved first, then the import happens; the page shows Globals |
| I20 | **Import…** a random non-JSON file (e.g. a `.png`) | red "Could not import …: not valid JSON …"; nothing created |
| I21 | **Import…** a JSON file that isn't Postman (`echo '{"a":1}' > /tmp/x.json`) | red "not a Postman collection, environment or globals file" |
| I22 | Cancel the open/save dialogs | nothing happens, no message |
| I23 | With a fresh blank tab (no collection clicked), **Export…** | red "Click a collection in the sidebar…" |
| I24 | `ls /tmp/at-test5` after sending a request | `history.db` is directly in the data folder (not under `collections/`) |
| I25 | Regression: T5/T10 (doc 09), H3/H7 (doc 10), E4 (doc 08) | same as before |

Report pass/fail per row. For I3/I5 paste the output; for anything failing, a screenshot plus what you clicked.
