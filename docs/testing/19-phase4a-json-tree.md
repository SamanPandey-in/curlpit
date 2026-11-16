# 19 — Phase 4a: JSON tree view and in-line Find

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests, release build). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Tree** response sub-tab, next to Body: the response as a collapsible JSON tree (FR-RES-03). Objects and arrays deeper than **two levels start folded**, so a big document opens readable instead of as 150,000 rows.
- **Click a row** to select it; clicking an object or array also folds/unfolds it (`▸` folded, `▾` open). **Expand all** / **Collapse all** in the bar above.
- **A colour per JSON type:** keys blue, strings green, numbers peach, booleans mauve, `null` and container summaries grey. Fixed colours, not theme colours, so a type reads the same in the light and the dark theme.
- **Copy value** (the selected row — on an object or array this copies that whole subtree as JSON), **Copy path** (`data.users[0].name`, bracketed and escaped for odd keys: `data["weird key"]`), **Copy all** (the whole document). Each one reports in the status line.
- Folded containers show a count instead of their contents: `{3 fields}`, `[12 items]`, `{}`, `[]`. Strings are shown quoted and clipped at 300 characters on the row — **Copy value still copies all of it**.
- **Find in the Body view now highlights every match inside its line**, not just the row it sits on, with the current match in a second colour. Closing Find (`✕`) clears the query and the matches (FR-RES-05's "clear").
- Non-JSON or malformed bodies show why there's no tree instead of an empty pane, and the Body view keeps working. Bodies over 16 MB aren't indexed and say so.

**Not yet:** editing values in the tree, searching/filtering inside the tree, keyboard folding, array indices shown on each row, a binary/hex view, a tree for bodies over 16 MB, highlighting `{{variables}}` in the tree.

## How it stays inside the memory budget
The index holds **byte offsets into the response body, never a parsed copy** — one 32-byte node per value, so the whole index is a few times the node count rather than ~10x the body like a `serde_json::Value` tree would be (D18, D36, NFR-PERF-05). Rendering is virtualized: only the rows on screen are built, and the visible-row list is cached so scrolling never re-walks the tree. The parser is iterative with an explicit stack, so a hostile deeply-nested body can't overflow the stack — past 1024 levels it reports an error instead.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 149 passed (52 core + 9 http + 18 store + 11 history + 13 interop + 36 codegen + 10 script)
cargo test -p at-app            # expect 12 passed
API_TESTER_DATA_DIR=/tmp/at-test13 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| Q1 | **Tree** on a fresh tab, nothing sent yet | "Send a request to see the response" |
| Q2 | GET `https://httpbin.org/get`, then **Tree** | a tree: `url`, `headers` (folded or open per depth), `origin`; keys blue, string values green |
| Q3 | Click `headers` (an object) | it folds; the row shows `{N fields}`; click again, it opens |
| Q4 | GET `https://httpbin.org/json` → **Tree** | `slideshow` open at depth 1; its nested `slides` array **folded** (depth 2 default) |
| Q5 | **Expand all** | everything opens, including the nested slides |
| Q6 | **Collapse all** | collapses to a single root row |
| Q7 | Click a deep string row, **Copy path**, paste somewhere | a path like `slideshow.slides[0].title` |
| Q8 | With the same row selected, **Copy value** | just that value, unescaped, no quotes |
| Q9 | Select the `slideshow` object, **Copy value** | that whole object as JSON |
| Q10 | **Copy all** | the entire document |
| Q11 | **Copy value** / **Copy path** with nothing selected | red "Click a row in the tree first." |
| Q12 | POST `https://httpbin.org/post` with JSON body `{"n":1,"ok":true,"nil":null,"s":"x"}`, **Tree**, find them under `json` | `1` peach, `true` mauve, `null` grey, `"x"` green and quoted |
| Q13 | GET `https://httpbin.org/html` → **Tree** | "This response can't be shown as a JSON tree: …" with an offset; **Body** still shows the HTML |
| Q14 | GET a large JSON (e.g. `https://httpbin.org/stream/500`) → **Tree** | opens promptly, scrolls smoothly; RSS in Activity Monitor stays well under 250 MB |
| Q15 | Switch Body ⇄ Tree ⇄ Headers repeatedly | no flicker, no reparse delay (the tree is built once per response) |
| Q16 | Send again in the same tab with Tree open | the tree replaces itself with the new response |
| Q17 | Two tabs, different responses, both on Tree | each keeps its own tree, fold state and selection |
| Q18 | **Body** → **Find** → type `e` | every `e` on screen highlighted yellow, the current one peach; `1 / N` counts up live |
| Q19 | `↓` / `↑` through matches | the peach match moves and the view scrolls to it; the yellow ones stay yellow |
| Q20 | Press `✕` on the Find bar, then **Find** again | the box is empty, no stale highlights or count |
| Q21 | Find a string that occurs inside a long wrapped line (turn wrap on) | the highlight lands on the right part of the right row, split across rows if it straddles one |
| Q22 | Find with a tab character nearby (e.g. a raw body with tabs) | highlight stays aligned with the text (tabs are expanded per span) |
| Q23 | A **spilled** body: fetch JSON over 10 MB and open **Tree** while it still says "Loading the … response body…" | the placeholder first reports it can't be shown as a tree, then the tree **appears by itself** once the body finishes loading (it used to stay stuck on that error) |
| Q24 | With Find open and a query that matches, send the same request again and get a response of the same size but different content | the highlights move to the new body's matches; nothing stays highlighted at a stale position |
| Q25 | Regression: J2/J7 (doc 18), K2 (doc 17), S1 (doc 16) | same as before |

Report pass/fail per row; for Q7/Q9/Q13 paste what you got, and for Q14 the peak RSS.

Q23 and Q24 cover the two stale-cache bugs fixed in D40. Neither was visible before this
phase — the tree and the in-line highlighting are the first things to read those cached
offsets — so they are worth running even though they look like edge cases.
