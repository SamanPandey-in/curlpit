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
## Round 3: sticky line numbers + text wrap

What changed: (1) the line-number column stays put while you scroll sideways; (2) a **Wrap: off / on** button appears at the right of the status line when text is wider than the window; (3) with wrap on, a long line continues on the next row(s) with a **blank** gutter, and line numbers never shift (the next real line keeps its true number); (4) wrapping re-flows when you resize the window.

Known limits (by design for now): wrap width is computed from an estimated character width (monospace assumed; emoji/CJK wide glyphs may overflow slightly); diagonal swipes can drop a bit of sideways movement; no visible scrollbar yet; resizing a huge body (50 MB) with wrap on may stutter.

```
cd ~/inCoding/nextgen/api-tester
tar xzf ~/Downloads/api-tester-wrap.tar.gz --strip-components=1
cargo fmt --all
cargo test -p at-core        # expect 28 passed (6 line-index tests; the old 4 were replaced)
cargo run --release -p at-app
```
| # | Do | Expect |
|---|---|---|
| R1 | `https://jsonplaceholder.typicode.com/photos`, narrow the window, swipe sideways | text moves, **line numbers stay fixed** at the left |
| R2 | Widen the window until everything fits | **Wrap** button disappears; no sideways scroll |
| R3 | Narrow again, click **Wrap: off** | button reads "Wrap: on"; long lines continue on following rows; those rows have an **empty** gutter |
| R4 | In wrap mode, look at the numbers around a wrapped line | numbers go 5, (blank), 6 — never 5, 6, 7 for the same line |
| R5 | Resize the window slowly in wrap mode | text re-wraps to the new width, no jumping/flicker; right edge not clipped |
| R6 | Toggle wrap off again | back to one row per line; sideways scroll works; position resets to the left |
| R7 | `https://example.com` and `https://httpbin.org/html` | wrap button appears only when text really overflows |
| R8 | `https://httpbin.org/bytes/200000` (garbage bytes) | no freeze with wrap on or off |
| R9 | Scroll the 5 MB photos response vertically in wrap mode | still smooth; memory still roughly 140–160 MB |

Report: pass/fail per row; for any wrap alignment issue, tell me if text is clipped at the right (my width estimate is too big) or leaves a gap (too small).
