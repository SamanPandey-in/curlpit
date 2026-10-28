# 07 — Phase 3a: saving requests, collections sidebar, shortcuts

> Written without a compiler: the new `at-store` crate has 8 unit tests that will tell us quickly whether TOML saving works; the sidebar/shortcuts are the likeliest places for a compile error (send me the first one).

**New**
- **At-rest format:** every request is one readable TOML file; collections are folders. Writes are atomic (temp file + rename), so a crash never leaves a half-written file.
- **Sidebar** (left): your collections, folders and requests. Click a request to open it; click a collection/folder to choose where the next *Save* goes. **+** creates a collection.
- **Name box** (top-left) + **Save** button. Saving an opened request overwrites it; renaming it moves the file.
- **Shortcuts:** Cmd+S (Ctrl+S) saves, Cmd+Enter (Ctrl+Enter) sends/cancels. They work while focus is inside the window's fields; if one doesn't fire right after clicking a button, click into any text box first.
- Unreadable files show in red with ⚠ (never silently hidden); files from a newer app version are refused with a clear message.

**Not yet:** rename/delete/duplicate/drag-move in the sidebar, creating folders from the UI (you can make them in Finder), tabs, environments.

## Setup
```
cd ~/inCoding/nextgen/api-tester
tar xzf ~/Downloads/api-tester-3a.tar.gz --strip-components=1
cargo fmt --all
cargo test                      # expect 41 passed (28 core + 5 http + 8 store)
API_TESTER_DATA_DIR=/tmp/at-test cargo run --release -p at-app
```
`API_TESTER_DATA_DIR` makes the app use a throw-away folder; the files appear under `/tmp/at-test/collections/`. Without it the OS location is used (shown in the "Saved to …" message).

## Checklist
| # | Do | Expect |
|---|---|---|
| S1 | Launch (empty data dir) | sidebar says "No collections yet…" |
| S2 | Name = `Get httpbin`, click **Save** | status "Saved to …/my-collection/get-httpbin.toml"; sidebar shows **My Collection** with **Get httpbin** (highlighted) |
| S3 | `cat /tmp/at-test/collections/my-collection/get-httpbin.toml` | readable TOML: `schema_version = 1`, `[request]`, url, params, headers, body |
| S4 | Change the URL, Save again | same file overwritten; `ls` shows no `-2` copy |
| S5 | Change the name to `Renamed`, Save | old file gone, `renamed.toml` exists, sidebar updated |
| S6 | Click **+**, click the new "Collection 1", change name, Save | new file appears inside `collection-1/` |
| S7 | Click a saved request | name, method, URL, params, headers, body all restored; clicking another and back keeps data correct |
| S8 | Press Cmd+S / Cmd+Enter (click in the URL box first) | saves / sends |
| S9 | `echo "= =" > /tmp/at-test/collections/my-collection/bad.toml`, relaunch | red `⚠ bad`; clicking it shows an error; app fine |
| S10 | Quit, relaunch | sidebar restored; open a request and send it |
| S11 | Mash Cmd+S ~20 times quickly, then `ls -a /tmp/at-test/collections/my-collection` | no `.tmp` files left over |
| S12 | Edit a saved file's `url` in a text editor, relaunch, open it | app shows your edit (files are hand-editable) |
| S13 | `mkdir /tmp/at-test/collections/my-collection/users`, move a request file into it, relaunch | folder `▸ users` appears with the request; opening it and Save keeps it in that folder |
| S14 | Memory with a few requests saved | still ~70 MB idle (no regression) |

Report pass/fail per row; for S3 paste me the file so I can check readability, and note anything awkward in the sidebar look.
