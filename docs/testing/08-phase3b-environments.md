# 08 — Phase 3b: environments and variables

> Written without a compiler. Likeliest first-build trouble spots: `InputState::masked` / `set_masked` (secret masking) and `ClipboardItem::new_string` in the app crate. Paste me the first error if there is one.

**New**
- **Variables…** button (top bar) opens the environments page: **Globals** plus any number of named environments. Each variable has an enable checkbox, key, value, **Secret** (masks the value; **Show/Hide** reveals it) and ✕.
- **Env: …** button (top bar) cycles the active environment (none → first → … → none). The choice is remembered across launches.
- `{{name}}` is substituted in URL, params, headers, body and auth when you send. Precedence: active environment overrides globals. Unresolved names stay literal and are listed in amber next to the status.
- **Copy** button in the response bar copies the pretty-printed body.
- Fixed: the spare empty row in Params/Headers is no longer written into saved files.

**Not yet:** `{{var}}` highlighting inside the URL box, collection-level variables, a variable picker, exporting environments, request tabs, history.

## Setup
```
cd ~/inCoding/nextgen/api-tester
tar xzf ~/Downloads/api-tester-3b.tar.gz --strip-components=1
cargo fmt --all
cargo test                      # expect 50 passed (31 core + 5 http + 14 store)
API_TESTER_DATA_DIR=/tmp/at-test2 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| E1 | Launch; top bar | shows `Env: none` and `Variables…` |
| E2 | Click **Variables…** | page with Globals selected, one empty row, hint text at the bottom |
| E3 | Globals: add `host` = `httpbin.org`, click **Save** | status "Global variables saved"; `cat /tmp/at-test2/globals.toml` shows it |
| E4 | **← Back to requests**, set URL to `https://{{host}}/get`, Send | 200; the response `url` field says `https://httpbin.org/get` |
| E5 | Variables… → **+ New** | environment "Environment 1" appears and is selected; file `environments/environment-1.toml` exists |
| E6 | Rename it `Local`, add `host` = `example.com`, Save | file renamed to `local.toml`, content readable |
| E7 | Click **Make active**; Back; Send the same URL | request goes to example.com (the env overrides the global); top bar says `Env: Local` |
| E8 | Click `Env: Local` in the top bar | cycles to `Env: none`; Send goes to httpbin.org again |
| E9 | Quit and relaunch | active environment restored (set it active first); variables still there |
| E10 | URL `https://{{nope}}/get`, Send | amber `Unresolved variables: nope`; request fails with a clear error, no crash |
| E11 | URL `{{base}}/get` with env var `base` = `https://httpbin.org` | works (no double `https://`) |
| E12 | Add a variable, mark **Secret** | value shows as dots; **Show** reveals, **Hide** masks again |
| E13 | `cat environments/local.toml` after saving a secret | `secret = true`; the value is plain text (known, keychain comes later) |
| E14 | Edit a value, then click another scope (Globals) without pressing Save | edit is auto-saved; switching back shows it |
| E15 | Clear an environment's name, press Save | red "Give the environment a name"; nothing written |
| E16 | Create two environments with the same name | red "already exists" message |
| E17 | **Delete** | button changes to "Click again to delete"; second click removes the file and returns to Globals; if it was active, top bar shows `Env: none` |
| E18 | Untick a variable's checkbox, Save, Send | that variable is treated as unresolved |
| E19 | Cmd+S on the environments page | saves the variables (not the request) |
| E20 | Send any request, click **Copy**, paste into a text editor | pretty-printed body; button shows "Copied ✓" |
| E21 | Save a request that has only the default spare empty row in Headers; open the TOML | no `[[request.headers]]` with an empty key |
| E22 | Memory with environments page open | still ~70 MB idle |

Report pass/fail per row; for E6 and E13 paste the TOML files.
