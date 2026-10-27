# 06 — Phase 2b: request Params / Headers / Body tabs, response Body / Headers tabs, Enter to send

> Written without a compiler against gpui-component 0.5 (Checkbox, Input events). Send me the first compile error if any.

**New:**
- Request area has tabs **Params (n) · Headers (n) · Body**. Params and Headers are editable tables: checkbox to enable/disable a row, key and value boxes, ✕ to delete a row, **+ Add** for a new row. The (n) counts enabled rows with a key.
- Body tab: the JSON editor (a note shows when the method — GET, HEAD, OPTIONS — doesn't send a body).
- Response has tabs **Body · Headers (n)**; headers are sorted and shown key / value.
- **Enter in the URL box sends** (it will not cancel a running request).

**Not yet (next):** Cmd/Ctrl+Enter global shortcut, URL ⇄ Params two-way sync (for now params are *added to* the URL at send time), body types other than JSON, auth tab, saving.

## Setup
```
cd ~/inCoding/nextgen/api-tester
tar xzf ~/Downloads/api-tester-2b.tar.gz --strip-components=1
cargo fmt --all
cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| T1 | Launch | Params tab shows one row `hello = world`; tab label "Params (1)" |
| T2 | Send `https://httpbin.org/get` | response JSON `args` contains `"hello": "world"` |
| T3 | Untick the checkbox, send | `args` empty; label "Params (0)" |
| T4 | Tick it, edit value to `a b&c`, send | `args` shows `a b&c` (properly encoded on the wire) |
| T5 | **+ Add** a row `x` = `1`, send | `args` has both |
| T6 | ✕ the first row | row disappears, others remain intact (right keys/values stay with their rows) |
| T7 | Headers tab: add `X-Test` = `hello`, send to `https://httpbin.org/headers` | response shows `"X-Test": "hello"` |
| T8 | Headers tab: add `Authorization` = `Bearer abc`, send | server sees it |
| T9 | Method → POST, URL `https://httpbin.org/post`, Body tab edit JSON, send | `json` field matches; `Content-Type: application/json` |
| T10 | Method → GET, open Body tab | note "GET requests don't send a body." |
| T11 | Type in the URL box and press **Enter** | request sent; pressing Enter again while loading does nothing |
| T12 | Response **Headers** tab after any request | sorted list, e.g. content-type, date, server; count in the label matches |
| T13 | Switch Body ⇄ Headers repeatedly | scroll position / wrap button behave (wrap button only on Body) |
| T14 | Add 15 rows in Params | panel scrolls vertically; typing in rows still works |
| T15 | Memory after the 5 MB photos response | still ~140 MB; no growth after switching tabs a few times |

Report pass/fail per row, and anything that looks off in the tables (alignment, checkbox look, tab styling).
