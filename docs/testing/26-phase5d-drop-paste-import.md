# 26 — Phase 5d: import by drag-and-drop and paste

> Branch `p5`. Compiled and tested over SSH (fmt, clippy `-D warnings`, whole suite green, at-app 25). Both new routes are GUI, so **they haven't been exercised yet** — this checklist is the first real test. Short, because it reuses the import machinery that doc 23 already covered.

**New (FR-IE-09)**
- **Drag and drop**: drop a `.json`/`.har` (Postman collection, HAR, native JSON, or a Postman environment/globals file) anywhere on the window and it imports, exactly as the Import… picker does. Drop several and each imports in turn.
- **Paste**: a **Paste** button next to the collections' Import…, and a command-palette entry **"Import from clipboard"**, both import whatever collection/environment/globals text is on the clipboard.
- All three routes — picker, drop, paste — share one code path, so format detection, the warnings about what couldn't be brought over, and the "malformed input shows an error, never crashes" guarantee are identical for each.

## Known limits, stated plainly
- **Paste is a button/command, not Ctrl+V** — deliberately, so pasting into the URL or a header field still works normally. (Same convention as "Paste cURL".)
- **Drop imports the file's bytes**, so dropping a non-collection JSON gives the normal "not a recognised import file" error.
- No preview-before-import yet (P1).

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test            # whole suite green
API_TESTER_DATA_DIR=/tmp/at-test18 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| D1 | Drag a real Postman `.json` export from Finder/Explorer onto the window | it imports; the collection appears in the sidebar with its folders/requests; status says how many |
| D2 | Drag a `.har` onto the window | an "Imported from HAR" collection appears (same as doc 23 I2) |
| D3 | Select several collection files and drop them at once | each imports; the sidebar shows all of them |
| D4 | Drop a Postman **environment** file | it imports as an environment (not a collection) |
| D5 | Drop a plain `.txt` or a random `.json` (e.g. a `package.json`) | a clear "Could not import …: not a recognised import file" error; no crash; nothing added |
| D6 | Copy the full text of a Postman collection to the clipboard, click **Paste** (next to Import…) | it imports |
| D7 | With collection text still on the clipboard, open the palette (`Cmd/Ctrl+K`) → "Import from clipboard" | it imports the same way |
| D8 | Clear the clipboard (or copy a single space), click Paste | a clear "The clipboard is empty" message; no crash |
| D9 | Copy some non-collection text (a sentence), click Paste | a clear "not a recognised import file" error; no crash |
| D10 | Click in the URL bar and press Ctrl/Cmd+V with text on the clipboard | the text pastes into the field as normal (Paste didn't hijack Ctrl+V) |
| D11 | Regression: the Import… picker still works | unchanged (doc 23) |
| D12 | Anything that looks or feels wrong | say what and where |

Report pass/fail per row; D1 (drop a real file) and D10 (Ctrl+V still works in fields) are the two I most want confirmed.
