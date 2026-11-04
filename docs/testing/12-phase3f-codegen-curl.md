# 12 — Phase 3f: code generation and cURL import

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Code** tab in the response bar (next to Body / Headers): the current request as a ready-to-run snippet in **cURL**, **Python** (`requests`), **JavaScript** (`fetch`, Node 18+ / browsers), **Go** (`net/http`) or **Rust** (`reqwest`, blocking). It updates live as you edit the request; no Send needed.
- Snippets are built by the same code that sends requests, so they match exactly: params encoded into the URL, auth as headers, default Content-Type, form-data as multipart with files.
- **Variables: values / {{kept}}** toggle. *values* (default) substitutes the active environment and globals, so the snippet runs as-is. *{{kept}}* leaves `{{name}}` placeholders, safe to paste into docs or chat. If *values* would put a **secret** into the snippet, an amber warning says so.
- **Copy** copies the snippet.
- **+ cURL** (tab bar): copy a cURL command anywhere (browser dev tools "Copy as cURL (bash)", API docs, or our own snippet), then click **+ cURL**. It opens as a new unsaved request (`●`) named e.g. `POST api.example.com/v1/items`. It understands bash quoting (`'…'`, `"…"`, `$'…'`, `\` line breaks), `-X -H -d --data-raw --data-binary --data-urlencode --json -F -u -A -b -e -G -I -k -m --url --oauth2-bearer`, combined short flags (`-sSL`, `-XPOST`) and `--flag=value`. Anything else is listed as a note, never silently dropped.
- Core: `build_template()` builds a request without substituting variables (used by *{{kept}}*).

**Not yet:** more languages (PHP, C#, Java, HTTPie, PowerShell), syntax highlighting in the Code view, Windows `cmd`-style cURL (`^` escapes), loading `-d @file` data.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 94 passed (32 core + 5 http + 16 store + 11 history + 12 interop + 18 codegen)
cargo test -p at-app            # expect 3 passed
API_TESTER_DATA_DIR=/tmp/at-test6 cargo run --release -p at-app
```
For the run-the-snippet rows you need `python3 -m pip install requests` (Python), Node 18+ (JS), Go, or a scratch `cargo new` project with reqwest; skip the languages you don't have.

## Checklist
| # | Do | Expect |
|---|---|---|
| C1 | Default request (httpbin GET with `hello=world`), click **Code** | cURL selected: `curl 'https://httpbin.org/get?hello=world'` |
| C2 | Click Python / JavaScript / Go / Rust | each shows a complete program for the same request; tabs in Go shown as spaces |
| C3 | Change the URL or add a header | the snippet updates immediately |
| C4 | Method POST, body `{"a": 1}`, cURL | `-X POST`, `-H 'Content-Type: application/json'`, `--data-raw '{"a": 1}'` |
| C5 | Copy the cURL snippet, paste into Terminal, run | same response as pressing Send |
| C6 | Run the Python snippet (`python3 x.py`) | prints `200` and the body |
| C7 | Run the JS snippet (`node x.mjs`) | prints `200` and the body |
| C8 | Run the Go snippet (`go run x.go`) | prints `200 OK` and the body |
| C9 | Body containing a single quote, e.g. `{"name": "O'Brien"}`, run the cURL snippet | works; the quote is escaped as `'\''` |
| C10 | Env var `host=httpbin.org`, URL `https://{{host}}/get`, Variables: values | snippet shows `https://httpbin.org/get` |
| C11 | Toggle **Variables: {{kept}}** | snippet shows `https://{{host}}/get` |
| C12 | URL `{{base}}/get` with `base=https://httpbin.org`, {{kept}} | snippet shows `{{base}}/get` (no error, no added `https://`) |
| C13 | Mark a variable Secret and use it in a header, Variables: values | amber "This snippet contains secret values…"; switching to {{kept}} removes the warning |
| C14 | Empty URL, Code tab | amber "Can't generate code yet: URL is empty" |
| C15 | Open an imported request with Basic auth (doc 11), Code | `-H 'Authorization: Basic …'` |
| C16 | **Copy** | "Copied ✓"; pasted text equals the snippet; switching language resets the button |
| C17 | Copy this, click **+ cURL**: `curl -sSL -X PUT 'https://httpbin.org/put' -H 'X-Test: 1' --data-raw '{"x":1}' -H 'Content-Type: application/json'` | new tab `● PUT httpbin.org/put`; method PUT, header X-Test, Content-Type, JSON body; Send → 200 echoing `{"x":1}` |
| C18 | In Chrome/Safari dev tools, right-click a request → Copy as cURL (bash), then **+ cURL** | opens with all headers; Send works (cookies included as a `Cookie` header) |
| C19 | **+ cURL** with `curl -u user:passwd https://httpbin.org/basic-auth/user/passwd` | **Auth (Basic)** tab filled in; Send → 200 authenticated |
| C20 | **+ cURL** with `curl -G https://httpbin.org/get --data-urlencode 'q=a b'` | GET; Params `q = a b`; Send → args `q: "a b"` |
| C21 | **+ cURL** with `curl --proxy http://x:1 --weird https://httpbin.org/get` | opens; status lists notes for `--proxy` and `--weird` |
| C22 | **+ cURL** with non-cURL text on the clipboard (e.g. "hello") | red "Clipboard: this doesn't look like a cURL command…"; no new tab |
| C23 | **+ cURL** with `curl 'https://x` (broken quote) | red "unbalanced quotes" |
| C24 | Round trip: build a POST with form-data (or use **Upload**-style imported request), copy its cURL, **+ cURL** it | identical method, URL, headers and form fields |
| C25 | Close a **+ cURL** tab without saving | asks **Discard?** first (it's unsaved) |
| C26 | Regression: T10 (doc 09), H7 (doc 10), I8/I9 (doc 11), E4 (doc 08) | same as before |

Report pass/fail per row; for anything failing, the snippet text (or a screenshot) plus what you did.
