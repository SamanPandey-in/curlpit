# 24 — Phase 5b: command palette and global shortcuts

> Branch `p5` (now rebased onto `main`, so it carries the One theme). Compiled and tested on your Mac over SSH — clippy, fmt, 25 `at-app` tests (incl. 5 new palette-ranking tests) and the whole suite — but **the overlay and its keys have not been clicked yet**, so this checklist is the first real test. The palette is pure GUI, so none of it could be driven over SSH.

**New**
- **Command palette (FR-KEY-01, 02).** `Cmd+K` (macOS) / `Ctrl+K` (Windows, Linux), and also `Cmd/Ctrl+Shift+P`, open a dimmed overlay with a search box and a list. Type to fuzzy-filter by command **or** its category; **Up/Down** move the highlight, **Enter** runs it, **Esc** or a click outside closes it, and a click on a row runs it.
- **The command set:** Send, Save, New / Close / Reopen tab, Find in response; go to Requests / Environments / Runner / Cookies; show Collections / History; Import; Export collection as Postman; **Export collection as JSON** (new — see below); Theme: Light / Dark / Follow system; and one **Environment: …** row per environment (plus "none"), rebuilt each time you open it so the list is current.
- **Global shortcuts (FR-KEY-03, 04)** use Cmd on macOS and Ctrl elsewhere. Send (`Cmd/Ctrl+Enter`) and Save (`Cmd/Ctrl+S`) are unchanged; Escape now also dismisses the palette.
- **Native JSON export** (left to the GUI in 5a) is now reachable as the palette command "Export collection as JSON", so a collection can be round-tripped out and back in entirely through the app.

## Known limits, stated plainly
- **Esc is the one routing risk.** If the search box's own Escape handling ever swallows the key, the palette won't close on Esc — use the click-outside or press `Cmd/Ctrl+K` again. Tell me if this happens (U8 below).
- **Diff (`Cmd/Ctrl+D`) is not bound.** The request-diff feature doesn't exist yet (it's P1); the shortcut lands with it, not before.
- **No drag-and-drop or paste for collections yet** (FR-IE-09), and native-JSON export has a palette command but no toolbar button.
- **Theme chosen from the palette is not persisted** — it applies for the session. Saving a theme preference is the Settings-screen slice (FR-SET).
- The list doesn't scroll; with a query it's short, but with no query and many environments the tail can clip. Type to narrow.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test -p at-app            # expect 25 passed
cargo test                      # expect all green
API_TESTER_DATA_DIR=/tmp/at-test16 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| U1 | Press `Cmd/Ctrl+K` | a dimmed overlay opens with a focused search box and a list of commands |
| U2 | Press `Cmd/Ctrl+Shift+P` | opens the same palette |
| U3 | Type `send` | "Send request" is at or near the top |
| U4 | Type `theme dark` | "Theme: Dark" is the top match (category + label both searched) |
| U5 | Type `nt` | "New tab" appears (initials match) |
| U6 | Press Down a few times, then Up | the highlight moves and wraps around at the ends |
| U7 | Highlight a command and press Enter | it runs and the palette closes (try "Go to Runner" — the Runner page shows) |
| U8 | Open the palette, press **Esc** | it closes. If it does **not**, note it — then close by clicking outside or `Cmd/Ctrl+K` |
| U9 | Open the palette, click a row | that command runs |
| U10 | Open the palette, click the dimmed area outside the panel | it closes without running anything |
| U11 | Palette → type `light` → Enter; then `dark`; then `follow` | the theme switches each time (light / dark / follows the system) |
| U12 | Create two environments, open the palette, type `env` | an "Environment: …" row per environment, plus "Environment: none"; run one |
| U13 | After U12, send a request using a `{{var}}` from that env | the chosen environment is active (same as selecting it on the Variables page) |
| U14 | Palette → "Import file…" → pick a file | the normal import dialog opens (same as the sidebar Import) |
| U15 | Select a collection, palette → "Export collection as JSON" | a save dialog for a `.json` file; the saved file re-imports via Import (round-trip) |
| U16 | Palette → "Export collection as Postman…" | the Postman export, unchanged from before |
| U17 | Close the palette, then use the app normally: type in the URL bar, press Up/Down in a text field | arrow keys behave normally; the palette's Up/Down only act while it is open |
| U18 | Regression: `Cmd/Ctrl+Enter` to send, `Cmd/Ctrl+S` to save, `Cmd/Ctrl+F` to find | all unchanged |
| U19 | Anything that looks or feels wrong | say what and where |

Report pass/fail per row; U8 (Esc) and U17 (arrow keys outside the palette) are the two I most want confirmed.
