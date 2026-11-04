# 15 — Phase 3i: big responses, Save…, Find

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Big responses are shown.** Bodies over 10 MB were already streamed to a temp file (D7), and the app used to only say where. Now they load into the viewer on a background thread (UI stays responsive; a "Loading the 48.0 MB response body…" placeholder shows meanwhile), up to **200 MB**. Bigger ones show where the file is and can be saved. Big bodies are shown as received (not pretty-printed), so a minified 50 MB JSON stays one long line, split into rows of 4,000 characters as before.
- **Save…** in the response bar writes the body to a file (save dialog in `~/Downloads`, name `response.json/.xml/.html/.txt` from the Content-Type). For big bodies it copies the original bytes; for normal ones it saves what you see (JSON pretty-printed).
- **Find** (button, or **Cmd+F** / Ctrl+F when focus isn't inside a text box): a search bar above the response.
  - Case-insensitive; shows `3 / 120` (counts up to 10,000, then `10000+`).
  - **Enter** = next match, **Cmd+Enter** = previous, or the **↑ ↓** buttons. Wraps around at the ends.
  - The row with the current match is highlighted and scrolled to the middle of the view; without Wrap, the text also scrolls sideways so the match is visible.
  - Each tab keeps its own search, and the results update when a new response arrives; **✕** closes the bar.
- A slow background load never replaces a newer response in that tab (each response gets a generation number).

**Not yet:** highlighting each match within the line (the whole row is highlighted), regex/whole-word search, search in the Headers or Code views, pretty-printing big JSON in the background, a dedicated binary/hex view.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 96 passed (33 core + 5 http + 17 store + 11 history + 12 interop + 18 codegen)
cargo test -p at-app            # expect 4 passed
API_TESTER_DATA_DIR=/tmp/at-test9 cargo run --release -p at-app
```
Big test bodies, served locally (separate terminal):
```
mkdir -p /tmp/big && cd /tmp/big
python3 -c "import json; json.dump([{'id': i, 'name': f'user {i}', 'email': f'u{i}@example.com', 'tags': ['a','b','c']} for i in range(500000)], open('big.json','w'), indent=1)"
python3 -c "open('huge.txt','w').write(('x' * 99 + '\n') * 2_500_000)"   # ~250 MB
python3 -c "import json; json.dump([{'id': i, 'name': f'user {i}'} for i in range(400)], open('min.json','w'))"   # one long line
ls -lh                           # big.json is ~50 MB
python3 -m http.server 8765
```

## Checklist
| # | Do | Expect |
|---|---|---|
| B1 | GET `http://localhost:8765/big.json` | status `200 OK · … · ~50 MB`; briefly "Loading the … response body…", then the JSON appears; scrolling is smooth; the UI never freezes |
| B2 | While B1 loads, click other tabs and type | responsive |
| B3 | Activity Monitor: memory with B1 shown | roughly 130–250 MB (baseline measured 130 MB at 50 MB) |
| B4 | Send B1, then immediately send `https://httpbin.org/get` in the same tab | ends showing the httpbin response, not the big file |
| B5 | GET `http://localhost:8765/huge.txt` (~250 MB) | message "…too large to show here…saved to: …"; **Save…** copies it (check the size with `ls -l`) |
| B6 | B1 shown → **Save…** → save as `big-copy.json`; `cmp /tmp/big/big.json ~/Downloads/big-copy.json` | identical (original bytes) |
| B7 | Normal JSON response (`https://httpbin.org/json`) → **Save…** | file has the pretty-printed JSON; name suggested `response.json` |
| B8 | `https://httpbin.org/html` → **Save…** | suggested name `response.html` |
| B9 | On `https://httpbin.org/json` click **Find**, type `title` | count like `1 / 4`; that row highlighted and centered |
| B10 | Press Enter repeatedly | moves through matches and wraps to `1 / n` after the last |
| B11 | Cmd+Enter in the find box, or **↑** | goes backwards (wraps from 1 to n) |
| B12 | Type `zzzz` | "No matches"; nothing highlighted |
| B13 | Search is case-insensitive: `TITLE` | same matches as `title` |
| B14 | Find on B1 (`user 499999`) | jumps near the end instantly; row highlighted |
| B15 | Find `@example.com` on B1 | count shows `1 / 10000+` (capped), Enter still steps through |
| B16 | Wrap off: GET `http://localhost:8765/min.json` (one long line, see setup), find `user 300` | the text scrolls sideways so the match is visible; line numbers stay put |
| B17 | Wrap on, find | the wrapped row containing the match is highlighted |
| B18 | Find open in tab A with a query; switch to tab B and back | A still has its query and position; B has its own (empty) |
| B19 | With Find open, Send again | matches recompute for the new response |
| B20 | **✕** on the find bar | bar closes, highlight gone; Cmd+F opens it again with the last query |
| B21 | Cmd+F while typing in the body editor | the editor's own find opens (ours doesn't steal it); Cmd+F after clicking the response area opens ours |
| B22 | Regression: C1 (doc 12), A2 (doc 13), R2 (doc 14), H7 (doc 10) | same as before |

Report pass/fail per row; for B1/B3 note the load time and memory.
