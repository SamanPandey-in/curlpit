# 05 — Phase 2a: first real window (send a request, read the response)

**What exists:** a single window with a method button (click to cycle GET→POST→PUT→PATCH→DELETE→HEAD→OPTIONS), URL box, Send/Cancel, a JSON body editor (shown for POST/PUT/PATCH/DELETE), a coloured status line, and the virtualized response viewer with automatic JSON pretty-printing.
**Not yet:** headers/params tables, tabs, saving, environments, Enter-to-send, selecting/copying response text, search. Those are the next slices.

> Written without a compiler against the gpui 0.2 / gpui-component 0.5 APIs. If the build fails, send the first error block.

## Setup
```
cd ~/inCoding/nextgen/api-tester
tar xzf ~/Downloads/api-tester-phase2a.tar.gz --strip-components=1
cargo test -p at-core            # new: JSON pretty-printer tests (expect 22 passed)
cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| 1 | Click **Send** with the default URL (httpbin.org/get) | `200 OK · NNN ms · NNN B` in green; pretty JSON with line numbers |
| 2 | Click the method button once → POST. Change URL to `https://httpbin.org/post`, send | body editor appears (160 px); response `json` field shows your body |
| 3 | URL `httpbin.org/get` (no https://) | still works (https:// added) |
| 4 | URL `https://httpbin.org/status/404`, then `/status/500`, then `/redirect/1` | red for 404/500; 200 after the redirect |
| 5 | URL `https://httpbin.org/delay/10`, Send, then click **Cancel** after 1–2 s | button changes to Cancel; "Cancelled" appears immediately; UI never freezes |
| 6 | URL `https://does-not-exist.invalid` | red line: "DNS lookup failed…" |
| 7 | URL `http://127.0.0.1:9` | red line: "connection refused…" |
| 8 | Empty URL, or `ftp://x` | clear error text, no crash |
| 9 | Large response: `https://jsonplaceholder.typicode.com/photos` (~5000 items) | smooth scrolling; note time shown and memory (Activity Monitor → "api-tester") |
| 10 | Local server: in a terminal `python3 -m http.server 8000`, URL `http://127.0.0.1:8000/` | HTML listing shown as text, total time a few ms |
| 11 | While a request is running, try typing in the URL box and scrolling the old response | UI stays responsive |
| 12 | Resize the window, switch macOS light/dark | layout holds; colours readable in both |

## Report back
Pass/fail per row (describe any ❌), the memory shown for the app after #9, and anything that looks off visually (spacing, colours, font). Also: `ls -l target/release/api-tester` (the real binary size) and `ls -l ../spikes/target/release/spike-gpui-hello` if you still haven't sent that.


---
## Re-test after the width fix (viewer horizontal scroll)
Changes: the response viewer now scrolls sideways (two-finger swipe left/right, or Shift+scroll), sized to the longest line; very long lines (minified HTML etc.) are split into 4,000-byte rows so they stay fast; line numbers count those rows.

```
cd ~/inCoding/nextgen/api-tester
tar xzf ~/Downloads/api-tester-widthfix.tar.gz --strip-components=1
cargo fmt --all
cargo test -p at-core        # expect 26 passed (4 new for the line index)
cargo run --release -p at-app
```
| # | Do | Expect |
|---|---|---|
| W1 | `https://jsonplaceholder.typicode.com/photos`, make the window narrow | long lines are cut at the edge but you can swipe sideways to read the rest |
| W2 | Widen the window | no phantom scroll area; content fills, sideways scroll disappears when everything fits |
| W3 | `https://example.com` (HTML) | text shown, scrolls sideways if lines are long |
| W4 | A response with one huge line, e.g. `https://httpbin.org/bytes/200000` (binary garbage) | no freeze; shown as many ~4,000-byte rows |
| W5 | Vertical scroll still smooth on the 5 MB response; memory still ~140 MB | yes |
Note: no visible sideways scrollbar yet (swipe only). I'll add scrollbars with the next slice.
