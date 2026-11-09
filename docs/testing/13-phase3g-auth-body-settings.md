# 13 — Phase 3g: auth, body and settings editors

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- Request sub-tabs are now **Params · Headers · Body · Auth · Settings**. The Body and Auth labels show what's chosen, e.g. `Body (XML)`, `Auth (Basic)`.
- **Body** tab: pick **None**, **Raw**, **urlencoded** or **form-data**.
  - *Raw* has **JSON / XML / HTML / Text**. This sets the default `Content-Type` (`application/json`, `application/xml`, `text/html`, `text/plain`); a Content-Type header you add yourself always wins.
  - *urlencoded*: key/value rows with on/off checkboxes, sent as `a=1&b=2` with `application/x-www-form-urlencoded`.
  - *form-data*: rows with **Text / File** per row. File rows get **Choose…** (native file picker) or you can type a path; sent as multipart.
  - Switching mode keeps what you typed in the other modes until you save, so you can flip back.
- **Auth** tab: **None**, **Basic** (username, password), **Bearer token**, **API key** (name, value, **Header** or **Query param**). Passwords, tokens and key values are masked, with **Show / Hide**. `{{variables}}` work in every field. Requests imported with JWT or custom-header auth show **From file** and keep it unchanged until you pick another type.
- **Settings** tab: **Timeout (ms)** (0 = none; blank = default 30000; a half-typed value like `12s` keeps the previous one), **Follow redirects**, **Verify TLS certificates** (with a warning when off).
- All of this is saved in the request file, sent, recorded in history, exported to Postman, and shown in the Code tab.
- The grey "Also sent: … (editing arrives later)" line from 3e is gone; those parts are now editable.

**Not yet:** JWT/custom-header editors, OAuth 2.0, Digest/AWS auth, binary-file bodies, syntax highlighting that follows the raw kind (XML still highlights as JSON), collection-level auth/settings, proxy and client-certificate settings.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 94 passed (unchanged)
cargo test -p at-app            # expect 4 passed
API_TESTER_DATA_DIR=/tmp/at-test7 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| A1 | Launch | sub-tabs `Params (1) · Headers (0) · Body (JSON) · Auth · Settings` |
| A2 | Body: Raw JSON, POST to `https://httpbin.org/post` with `{"a":1}` | httpbin `json: {"a": 1}`, `Content-Type: application/json` |
| A3 | Switch Raw kind to **XML**, body `<a>1</a>`, Send | `data: "<a>1</a>"`, header `Content-Type: application/xml`; tab label `Body (XML)` |
| A4 | **Text** kind | `Content-Type: text/plain` |
| A5 | Add header `Content-Type: application/vnd.test+json`, Send | your header is sent, not the default |
| A6 | Body **None**, method POST, Send | no body; `data: ""` |
| A7 | **urlencoded**, rows `a=1`, `b=2` (b unticked), Send | `form: {"a": "1"}` |
| A8 | **form-data**: `note` Text `hi`; `file` → **File** → **Choose…** a small image, Send | `form: {"note": "hi"}`, `files: {"file": "data:image/…"}` |
| A9 | form-data File row: type a path that doesn't exist, Send | clear red error naming the file; no crash |
| A10 | Switch Raw → urlencoded → Raw | the raw text is still there |
| A11 | Auth **Basic** `user` / `passwd`, GET `https://httpbin.org/basic-auth/user/passwd` | 200 `authenticated: true`; password masked; **Show** reveals, **Hide** masks |
| A12 | Auth **Bearer** token `abc`, GET `https://httpbin.org/bearer` | 200 `token: "abc"` |
| A13 | Auth **API key** `X-API-Key` = `k1`, **Header**, GET `https://httpbin.org/headers` | `X-Api-Key: k1` in the echoed headers |
| A14 | Same with **Query param**, GET `https://httpbin.org/get` | `args: {"X-API-Key": "k1"}` |
| A15 | Token `{{token}}` with env var `token=t9` active | sent as `Bearer t9` |
| A16 | Switch Auth Basic → Bearer → Basic | username/password still filled in |
| A17 | Settings: **Timeout** `1000`, GET `https://httpbin.org/delay/3` | red timeout error after ~1 s |
| A18 | Timeout `0`, same URL | waits and returns 200 after ~3 s |
| A19 | **Follow redirects: off**, GET `https://httpbin.org/redirect/1` | `302` with a `Location` header; on → 200 |
| A20 | **Verify TLS: off**, GET `https://self-signed.badssl.com/` | 200 (amber warning shown); on → certificate error |
| A21 | Set Basic auth, a form-data body and timeout 5000; **Save**; `cat` the TOML | `[request.auth] type = "basic"`, `[request.body] type = "formdata"` with fields, `timeout_ms = 5000` |
| A22 | Close the tab and reopen the request from the sidebar | Body mode, rows, auth type and fields, settings all restored; no `●` |
| A23 | Change only the auth type | tab shows `●` (unsaved); changing back removes it |
| A24 | Open an imported request with Basic auth (doc 11 sample) | Auth tab shows **Basic** with `user` / `passwd` filled in |
| A25 | Code tab with Basic auth and urlencoded body | snippet has `Authorization: Basic …` and `a=1` |
| A26 | Export the collection (doc 11) and re-import it | auth, bodies, follow-redirect/TLS settings survive |
| A27 | Regression: T10 (doc 09), H7 (doc 10), I8 (doc 11), C5/C17 (doc 12) | same as before |

Report pass/fail per row; for A8/A20/A21 paste the output.
