# 10 — Phase 3d: history

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **History** in the sidebar: a **Collections | History** switch at the top. Every request you send is recorded, including errors and cancels. Newest first.
- Each row shows the method, the status (or red `ERR`), how long ago, and the URL. Times are relative ("just now", "5 min ago", "3 h ago", "2 d ago").
- **Search** box: filters by URL or request name as you type (case-insensitive; `%` and `_` are plain characters).
- **Open an entry:** click a row. It opens in a tab (reusing an untouched tab, otherwise a new one) with the request as it was sent *and* the stored response: status line, body and headers. An amber note says "From history, sent … ago". The tab isn't linked to any file; **Save** adds it to a collection as a new request.
- **Clear:** two clicks (**Clear** → **Really clear?**).
- **Secrets stay out:** the request is stored as you typed it, so `{{token}}` stays `{{token}}` in the history file; substituted values are never written.
- **Limits:** the newest 1,000 entries are kept (older ones are deleted automatically). Up to 1 MB of each response body is kept; longer bodies are cut, and opening them shows a note. Bodies large enough to spill to disk (over 10 MB) aren't kept at all.
- **File:** `<data-dir>/history.db` (SQLite, WAL mode). Writes happen on the network thread, so the UI never waits for the disk. If the file can't be opened (corrupt, or from a newer app version), history turns off with a red message; the rest of the app works normally.

**Not yet:** deleting a single entry, date headers (Today/Yesterday), an "open in the same tab" option, showing which environment was active, re-sending straight from the list, importing/exporting history.

## Setup
```
cd ~/inCoding/nextgen/api-tester
git status                      # Cargo.toml/lock, crates/history, crates/app, docs changed
cargo test                      # expect 61 passed (31 core + 5 http + 14 store + 11 history)
cargo test -p at-app            # expect 2 passed
API_TESTER_DATA_DIR=/tmp/at-test4 cargo run --release -p at-app
```
The first build compiles SQLite (C code via the Command Line Tools), so it takes a little longer once.

## Checklist
| # | Do | Expect |
|---|---|---|
| H1 | Launch with the fresh data dir, click **History** | "No requests yet. Everything you send appears here."; **Collections** button no longer primary; **+** hidden |
| H2 | Click **Collections** | the collections list as before, **+** back |
| H3 | Send `https://httpbin.org/get`, then open History | one row: `GET 200 · just now` and the URL |
| H4 | With History open, send three more (a POST, a `https://httpbin.org/status/404`, a `https://httpbin.org/status/500`) | rows appear immediately at the top, newest first; 404/500 in red, 200 in green |
| H5 | Send to `https://nonexistent.invalid` | row with red `ERR` |
| H6 | Send `https://httpbin.org/delay/5`, press **Cancel** | row with `ERR`; opening it shows "request cancelled" |
| H7 | Click the 200 row | opens in a tab: method, URL, params, headers, body restored; response body pretty-printed; status line `200 OK · … ms · …`; amber "From history, sent …" |
| H8 | In that tab, click **Headers (n)** in the response bar | the stored response headers |
| H9 | Click the `ERR` row from H5 | status line shows the error in red; body area says "No response: …" |
| H10 | Click a history row while the active tab has edits (●) | opens in a **new** tab; the edited tab is untouched |
| H11 | Click a row while on a fresh blank tab (**+**) | it's loaded into that blank tab (no extra tab) |
| H12 | Opened-from-history tab: **Save** | saved as a new request in the current collection; sidebar Collections shows it; history entry still there |
| H13 | Type `status` in the search box | only the 404/500 rows; clear the box → all rows back |
| H14 | Search `zzz` | "No matches." |
| H15 | Set an env var `token` = `secret123`, add header `Authorization: Bearer {{token}}`, send, then run `sqlite3 /tmp/at-test4/history.db "select request_json from entries order by id desc limit 1"` | shows `{{token}}`, **not** `secret123` |
| H16 | Open that entry from History | header still reads `Bearer {{token}}` (resolves again on the next Send) |
| H17 | Quit and relaunch, open History | all entries still there; relative times updated |
| H18 | **Clear** → **Really clear?** | list empty; relaunch: still empty |
| H19 | Click **Clear** once, then switch to Collections and back | button is back to **Clear** (nothing deleted) |
| H20 | Send `https://httpbin.org/bytes/2000000` (2 MB), open the entry | body shown, ending in "[… history kept only the first 1.0 MB of 1.9 MB]" |
| H21 | Send in two tabs at once (`delay/3` each), History open | both rows appear when they finish; the app never stutters |
| H22 | Quit; `echo garbage > /tmp/at-test4/history.db`; relaunch; open History | red "History is off: …"; sending and saving requests still work; no crash. Afterwards `rm /tmp/at-test4/history.db*` |
| H23 | Send a request with an empty URL | nothing is recorded (the request never left: "URL is empty") |
| H24 | Send ~30 requests quickly, scroll the History list | smooth; newest at top |
| H25 | Memory after ~50 history entries | still close to ~70 MB idle |
| H26 | Regression: T5, T10, T14 from doc 09, E4 and E10 from doc 08, S7 from doc 07 | same as before |

Report pass/fail per row. For H15 paste the sqlite output; for anything failing, a screenshot plus what you clicked.
