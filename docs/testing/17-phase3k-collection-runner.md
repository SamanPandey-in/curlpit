# 17 — Phase 3k: collection runner

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Run…** at the bottom of the Collections sidebar opens the **Collection runner** page. It runs the collection or folder you last clicked in the sidebar (or the collection holding the active tab's request).
- Every request runs **in sidebar order** (folders first, then by name, depth-first), one after another, through the same pipeline as Send: pre-request script → send → test script.
- **Variables chain:** what one request's scripts set with `pm.environment` / `pm.globals` is used by the next request, and is saved to the active environment and globals as it goes (same diff rules as doc 16). `pm.variables` (local) values last for one request.
- Options: **Iterations** (1–100), **Delay (ms)** between requests (0–60000), **Stop on first failure**.
- **Run / Stop.** Stop cancels the in-flight request and ends the run.
- **Live report:** a row per request with ✓/✗, iteration (`#2`, when more than one), method, name, status, time, size and `tests 3/4`. Failed tests and script errors are listed under the row. The summary line reads `12 / 30 requests · 1 failed · tests 40 passed, 2 failed · 3.2 s · done/stopped`.
- A request **fails** when there's no response (network/build error, unreadable file), a script errors, or a test fails. An HTTP error status on its own isn't a failure; add `pm.test(...)` for that, as in Postman.
- **Click a row** to open that request in a tab.
- **Copy summary** copies a plain-text report.
- The run keeps going while you look at other pages; the sidebar button shows `Run… ●` meanwhile.
- Runs use the **saved files** (unsaved tab edits aren't included) and **don't fill History**.

**Not yet:** data files (CSV/JSON iterations), choosing/reordering requests, a custom run order (Postman's own order isn't kept because our folders sort by name), saving run results, running in parallel, `postman.setNextRequest`.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 108 passed (unchanged)
cargo test -p at-app            # expect 8 passed
API_TESTER_DATA_DIR=/tmp/at-test11 cargo run --release -p at-app
```
Prepare: import `examples/postman/httpbin.postman_collection.json` and `httpbin.postman_environment.json` (doc 11), and make **httpbin** active. Add a test to a couple of requests, e.g. on **Get with query** → Scripts → Tests: Insert **Status is 200**.

## Checklist
| # | Do | Expect |
|---|---|---|
| K1 | Click **httpbin sample** in the sidebar, **Run…** | runner page: "Runs every request in "httpbin sample", in sidebar order." |
| K2 | **Run** | rows appear one by one: Auth/Basic auth, Auth/Bearer (inherited), Auth/OAuth2…, Basics/Get with query, Basics/Post JSON, Form urlencoded, Path variable; summary ends `· done` |
| K3 | Look at **Get with query** | `✓ … 200 OK · … ms · …  tests 1/1` |
| K4 | Change that test to expect 201, Save, Run | that row `✗`, `tests 0/1`, the failure message under it; summary `1 failed` |
| K5 | **Stop on first failure: on**, Run | stops after the failing row; summary ends `· stopped` |
| K6 | Iterations `3`, Run | 3 × 7 rows with `#1/#2/#3`; total `21 / 21` |
| K7 | Delay `1000`, Run | about a second between rows |
| K8 | Run `https://httpbin.org/delay/5`-style request (add one), press **Stop** while it runs | run ends promptly, `· stopped`, the in-flight row shows "request cancelled" |
| K9 | Chaining: request A Tests `pm.environment.set("n", String(Number(pm.environment.get("n") \|\| 0) + 1))`; request B URL `https://httpbin.org/get?n={{n}}`; Run with 2 iterations | B's responses show `n=1` then `n=2`; Variables… → httpbin shows `n = 2` afterwards |
| K10 | Click a row | opens that request in a tab (Request page) |
| K11 | **Copy summary**, paste | text report with ok/FAIL per line and failed test messages |
| K12 | Start a run, click **← Back to requests**, work in tabs, click **Run…** again | run continued; button showed `Run… ●` while running; page shows the progress |
| K13 | Click a folder (e.g. **Auth**), **Run…** | runs only the 3 requests in Auth |
| K14 | Put a broken file in the collection (`echo "= =" > …/bad.toml`), Run | a red row "unreadable…"/parse error; other rows run |
| K15 | Edit a request in a tab without saving, Run | the run uses the saved version |
| K16 | After a run, open History | runner requests aren't listed |
| K17 | Empty folder, Run… → Run | amber "… has no requests to run" |
| K18 | No collection selected (fresh blank tab, nothing clicked) → Run… | "Click a collection or folder in the sidebar, then Run…" |
| K19 | Iterations `abc` / `0` / `500` | treated as 1 / 1 / 100 |
| K20 | Regression: S1/S8 (doc 16), T10 (doc 09), R2 (doc 14) | same as before |

Report pass/fail per row; for K9 paste the two response `args`.
