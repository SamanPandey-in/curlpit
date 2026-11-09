# 18 — Phase 3l: cookie jar

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit and engine tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Cookies are kept and sent back automatically**, like a browser. A response's `Set-Cookie` is stored, and later requests to a matching domain/path get the `Cookie` header. This covers tabs, the collection runner, and redirects (e.g. a login that sets a cookie and redirects).
- Proper cookie rules (RFC 6265, via the `cookie_store` engine reqwest itself uses): host-only vs `Domain=` cookies (sub-domains), `Path=`, `Secure` (https only), `Max-Age` / `Expires`, `Max-Age=0` deletes, same name replaces.
- **A `Cookie` header you add yourself wins:** the jar adds nothing to that request.
- **Cookies** button (top bar, next to Variables…) opens the cookie list, grouped by domain: name, value, path, expiry (`in 3 h`, `session`), `Secure` / `HttpOnly`, **✕** per cookie, **Delete domain**, **Delete all** (two clicks).
- **Saved across restarts** in `<data-dir>/cookies.json`, session cookies too (so a login survives a restart, like in other API tools); expired ones are dropped on load. The file is written only when cookies change. If it can't be read, the app starts with an empty jar and says so.

**Not yet:** turning the jar off per request or globally, adding/editing cookies by hand, `pm.cookies` in scripts, per-environment jars, cookie domain allow-lists.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 112 passed (33 core + 9 http + 18 store + 11 history + 13 interop + 18 codegen + 10 script)
cargo test -p at-app            # expect 9 passed
API_TESTER_DATA_DIR=/tmp/at-test12 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| J1 | **Cookies** (top bar) on a fresh data dir | "No cookies yet…" |
| J2 | GET `https://httpbin.org/cookies/set?flavour=choc` (follow redirects on) | 200; the response is httpbin's `/cookies` page showing `{"cookies": {"flavour": "choc"}}` (set, then sent on the redirect) |
| J3 | **Cookies** | domain `httpbin.org` with `flavour = choc`, path `/`, `session` |
| J4 | GET `https://httpbin.org/cookies` | `{"cookies": {"flavour": "choc"}}` |
| J5 | GET `https://httpbin.org/cookies/set?a=1&b=2`, then **Cookies** | `a`, `b`, `flavour` listed |
| J6 | **✕** on `a`, GET `/cookies` | only `b` and `flavour` |
| J7 | Header `Cookie: manual=1`, GET `/cookies` | `{"cookies": {"manual": "1"}}` (your header wins; jar not added) |
| J8 | GET `https://httpbin.org/cookies/delete?b` | `b` gone from the list |
| J9 | GET `https://httpbin.org/response-headers?Set-Cookie=persist%3D1%3B%20Max-Age%3D3600` | `persist` listed with `in 59 min` / `in 1 h` |
| J10 | Quit, relaunch, **Cookies** | cookies still there; GET `/cookies` sends them |
| J11 | `cat /tmp/at-test12/cookies.json` | readable JSON with the cookies |
| J12 | **Delete domain** on httpbin.org | all httpbin cookies gone |
| J13 | **Delete all** → "Click again to delete all" → click | list empty; file updated |
| J14 | Runner (doc 17): request A GET `/cookies/set?run=1`, request B GET `/cookies`; Run | B's response shows `run=1` |
| J15 | `https://` cookie with `Secure` (`/response-headers?Set-Cookie=s%3D1%3B%20Secure`), then GET `http://httpbin.org/cookies` | `s` not sent over plain http |
| J16 | Quit; `echo junk > /tmp/at-test12/cookies.json`; relaunch; **Cookies** | red "Saved cookies could not be read, starting empty…"; app works |
| J17 | Two different hosts (httpbin.org and e.g. `https://postman-echo.com/cookies/set?x=1`) | listed under separate domains; each only sent to its own host |
| J18 | Regression: K2 (doc 17), S1 (doc 16), T10 (doc 09) | same as before |

Report pass/fail per row; for J2/J7/J14 paste the response body.
