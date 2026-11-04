# 16 — Phase 3j: pre-request and test scripts

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Scripts** request sub-tab (shows `Scripts ●` when a request has any) with two JavaScript editors (highlighted, line numbers):
  - **Pre-request**, run before sending. It can read and set variables and change headers, URL and the raw body; changes apply to this send.
  - **Tests**, run after the response arrives. `pm.test(...)` results appear in the response's **Tests (3/4)** tab.
  - **Insert:** buttons add common snippets (status is 200, fast response, JSON field, save to env; set variable, add header, log).
- **Tests** response tab: ✓ / ✗ / ○ (skipped) per test with the failure message, script errors in red (with the line), notes in amber, a **CONSOLE** section for `console.log/info/warn/error`, and timings.
- **The `pm` API** (a Postman-compatible subset):
  - `pm.environment`, `pm.globals`, `pm.variables`: `.get .set .unset .has .clear .toObject .replaceIn`. `pm.variables.get` looks through all scopes; `.set` sets a local variable for this send only; `pm.environment.name`.
  - `pm.request`: `.url`, `.method`, `.headers` (`.get .has .add .upsert .remove .toObject`), `.body.raw`.
  - `pm.response`: `.code .status .responseTime .responseSize .headers .text() .json()`, `pm.response.to.have.status(…)/.header(…)/.body(…)/.jsonBody(…)`, `pm.response.to.be.ok/.success/.clientError/.serverError/.notFound/…`.
  - `pm.test(name, fn)`, `pm.test.skip(name)`, and `pm.expect(x)` (Chai style): `.to.equal/.eql/.deep.equal`, `.not`, `.be.true/.false/.null/.undefined/.ok/.empty/.exist/.NaN`, `.above/.below/.least/.most/.within`, `.a("string")`, `.include`, `.property(name[, value])`, `.lengthOf`, `.match(/re/)`, `.oneOf`, `.keys`, `.status`, `.header`.
  - Legacy: `tests["name"] = bool`, `responseBody`, `responseCode.code`, `responseTime`, `postman.setEnvironmentVariable/getEnvironmentVariable/setGlobalVariable/…`.
  - `console.log/info/warn/error` (last 500 lines).
- **Variables set by scripts are saved:** into the active environment (secret flags and disabled variables kept) and globals. Two tabs changing variables at the same time don't undo each other. Without an active environment, `pm.environment.set` changes aren't kept and a note says so.
- **Sandboxed:** no file, network or process access (`require`, `fetch`, `process` don't exist); each script is limited to 5 s, 64 MB and a bounded call stack, and results collected before a time-out are still shown. Scripts run off the UI thread.
- **Saved and exported:** scripts live in the request file (`pre_request_script`, `test_script`), the session, history, and Postman export. **Postman import now brings scripts in** (previously skipped): collection- and folder-level scripts are copied into each request below them, outer ones first, which matches Postman's run order.

**Not yet:** `pm.sendRequest`, `require(...)` libraries (lodash, moment, CryptoJS), `pm.collectionVariables` persistence, `pm.cookies`, async tests, a collection runner, `atob/btoa`, script autocompletion.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 109 passed (33 core + 5 http + 18 store + 11 history + 13 interop + 18 codegen + 10 script)
cargo test -p at-app            # expect 6 passed
API_TESTER_DATA_DIR=/tmp/at-test10 cargo run --release -p at-app
```
Create an environment **Dev** and make it active (doc 08) before S8.

## Checklist
| # | Do | Expect |
|---|---|---|
| S1 | GET `https://httpbin.org/json`, Scripts → Tests → Insert **Status is 200**, Send | response tab **Tests (1/1)**; ✓ "Status is 200"; sub-tab shows `Scripts ●` |
| S2 | Add `pm.test("title", () => pm.expect(pm.response.json().slideshow.title).to.equal("Nope"));`, Send | **Tests (1/2)**; ✗ title with `expected "Sample Slide Show" to equal "Nope"` |
| S3 | Change the URL to `https://httpbin.org/status/404`, Send | ✗ Status is 200: `expected response to have status 200 but got 404` |
| S4 | Tests: `console.log("hello", {a: 1}); console.warn("careful")` | CONSOLE shows `hello {"a":1}` and `careful` in amber |
| S5 | Tests: `pm.test("broken", () => { throw new Error("boom") }); nosuch();` | ✗ broken: boom; red "Test script: ReferenceError: nosuch is not defined (…line…)" |
| S6 | Tests with a syntax error `pm.test("x", () => {` | red "SyntaxError…"; the request itself still succeeded |
| S7 | Tests: `while (true) {}` | after ~5 s, red "Script stopped: it ran longer than 5 s"; the app stayed responsive meanwhile |
| S8 | (Dev active) Tests: `pm.environment.set("lastId", pm.response.json().slideshow.author)`, Send to `/json` | Variables… → Dev shows `lastId = Yours Truly`; `cat …/environments/dev.toml` has it |
| S9 | Pre-request: `pm.variables.set("q", "from-script")`, URL `https://httpbin.org/get?x={{q}}` | response `args.x` = `from-script`; `q` is not saved anywhere (local) |
| S10 | Pre-request: `pm.request.headers.upsert({key: "X-Trace", value: "abc"})`, GET `https://httpbin.org/headers` | echoed headers contain `X-Trace: abc`; the Headers tab of the request is unchanged |
| S11 | Pre-request: `pm.environment.set("token", "t-" + Date.now())`, Auth Bearer `{{token}}`, GET `https://httpbin.org/bearer` | 200 and the token starts with `t-` (pre-request variables apply to the same send) |
| S12 | Make no environment active, Tests: `pm.environment.set("a", "1")` | amber "pm.environment changes were not kept: no environment is active" |
| S13 | Globals: Tests `pm.globals.set("g", "1")` | Variables… → Globals shows `g = 1` |
| S14 | Mark Dev's `token` Secret, then a script changes it | still Secret afterwards (masked) |
| S15 | Two tabs with `delay/2` and tests setting different env vars, Send both | both variables end up in Dev |
| S16 | Legacy: `tests["is 200"] = responseCode.code === 200;` | ✓ is 200 |
| S17 | `pm.test("no files", () => pm.expect(typeof require).to.equal("undefined"))` | ✓ |
| S18 | `pm.sendRequest(...)` | red "pm.sendRequest isn't supported yet" |
| S19 | Save the request, `cat` the file | `pre_request_script = …` / `test_script = …` near the top; reopen restores both editors |
| S20 | Quit/relaunch with an unsaved script edit (doc 14) | restored with `●` |
| S21 | Import a Postman collection that has tests (e.g. export one from Postman, or add an `event` to `examples/postman/httpbin.postman_collection.json`) | requests have the scripts; collection-level ones are prepended; the import notes say "scripts on N collection/folder level(s) were copied…" |
| S22 | Export a collection with scripts and re-import it | scripts identical |
| S23 | Code tab (doc 12) | snippets unchanged (scripts aren't part of generated code) |
| S24 | A request with no scripts | no Tests results; Tests tab says how to add some; Send is as fast as before |
| S25 | Regression: T10 (doc 09), H7 (doc 10), A11 (doc 13), R2 (doc 14), B9 (doc 15) | same as before |

Report pass/fail per row; for S5/S7 paste the Tests tab text.
