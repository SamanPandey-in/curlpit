# 01 — Test the core engine (Phase 1 checkpoint)

**What exists:** models, `{{variable}}` resolution, request building (URL/params/headers/auth/body), the HTTP engine (timeouts, cancel, errors, disk spill), and a tiny CLI to try it.

**Time needed:** ~15 minutes.

## A. Automated tests (2 min)
```
cargo test --workspace
```
**Expect:** every test passes (about 25 tests: core unit tests + 5 engine integration tests on a local fake server). No internet is needed for these.
**Report back:** pass/fail, and if fail, the first failing test name + message.

Also run: `cargo clippy --workspace --all-targets` and `cargo fmt --all -- --check`. Warnings are fine to report; I'll clean them.

## B. Manual checks with the demo CLI (10 min)
These need internet and use https://httpbin.org (any echo service works).

1. **Basic GET with variables**
   ```
   cargo run -p core-demo -- examples/get.json --env host=httpbin.org --env token=abc123
   ```
   Expect: `< 200 OK`; body JSON shows `"hello": "world"` in `args` and `"Authorization": "Bearer abc123"` in headers.
2. **Unresolved variable is flagged, not hidden**
   ```
   cargo run -p core-demo -- examples/get.json --env host=httpbin.org
   ```
   Expect a line `! unresolved variables: ["token"]` and header `Bearer {{token}}` sent literally.
3. **Variable precedence** — `--var` is global, `--env` is environment; environment must win:
   ```
   cargo run -p core-demo -- examples/get.json --var host=wrong.invalid --env host=httpbin.org --env token=t
   ```
   Expect success (it used `httpbin.org`).
4. **POST JSON**
   ```
   cargo run -p core-demo -- examples/post-json.json --env host=httpbin.org --env user=Ada
   ```
   Expect body echo with `"json": {"name": "Ada"}` and `Content-Type: application/json`.
5. **Error quality** (each should print a clear one-line `ERROR:` and exit, never a crash/panic):
   - DNS: `--env host=does-not-exist.invalid` → "DNS lookup failed"
   - Refused: edit a copy of get.json to use `http://127.0.0.1:9/` → "connection refused"
   - Bad URL: set url to `ftp://x` → "build error: URL must start with http:// or https://"
6. **Cancel**: use a slow endpoint (`https://{{host}}/delay/10` with host=httpbin.org), press **Ctrl+C** after 1–2 s. Expect `ERROR: request cancelled` within ~1 s (not 10 s).
7. **Large response / disk spill**: request `https://httpbin.org/stream-bytes/20000000` (20 MB). Expect `[body spilled to ... (20000000 bytes) ]` and RAM staying low (watch Activity Monitor / Task Manager / `htop`; the process should stay well under 100 MB).

## C. Speed check (optional)
Build release and time a local call:
```
cargo build --release -p core-demo
```
Then run against a local server (e.g. `python3 -m http.server 8000`, url `http://127.0.0.1:8000/`). Report the `in ...` time; target is a few ms.
Also report the size of `target/release/core-demo` (this is a baseline, not the final app).

## What to send me
Results of A, and a ✅/❌ per item in B (with the output for any ❌).
