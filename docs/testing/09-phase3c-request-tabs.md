# 09 — Phase 3c: request tabs

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, tests). The GUI itself has not been clicked through yet; that's this checklist.

**New**
- **Tab bar** above the URL row: one tab per open request, plus **+** for a new blank tab. Click a tab to switch, **✕** to close.
- **Each tab is independent:** its own name, method, URL, params, headers, body, response, status line, wrap, scroll and unresolved-variable warning. A request can be in flight in one tab while you edit or send in another; the result always lands in the tab that sent it.
- **Unsaved marker:** a tab shows `●` when its editor differs from what was last opened or saved. A tab with a request in flight shows `…`.
- **Closing an unsaved tab** needs a second click: **✕** turns into **Discard?**. Saving or switching to another tab disarms it. Closing a tab with a request in flight cancels that request. Closing the last tab leaves one fresh blank tab.
- **Opening from the sidebar:** if the request is already open, its tab is focused (no duplicates). An untouched tab (never edited, saved or sent) is reused; otherwise a new tab opens. Opening a request while on the Variables page autosaves the variables and goes back to requests.
- **New tabs** start blank (empty URL and body) and save into the same folder the current tab was using.
- **Shortcuts:** Cmd+T new tab, Cmd+W close tab (same Discard? rule). Ctrl+T / Ctrl+W on Windows/Linux. Like Cmd+S, they fire while focus is in the window; click into a text box if one doesn't.

**Not yet:** reopening the same tabs after a restart, a warning on quitting with unsaved tabs, dragging tabs to reorder, Ctrl+Tab switching, middle-click close, history.

## Setup
```
cd ~/inCoding/nextgen/api-tester
git status                      # only crates/app/src/main.rs, docs/DECISIONS.md and this doc changed
cargo test                      # expect 50 passed (31 core + 5 http + 14 store), unchanged
cargo test -p at-app            # expect 2 passed (tab titles, which tab is active after closing)
API_TESTER_DATA_DIR=/tmp/at-test3 cargo run --release -p at-app
```
Use a fresh data dir (`/tmp/at-test3`) so earlier test files don't interfere. Create one collection with two saved requests first (T5 does this).

## Checklist
| # | Do | Expect |
|---|---|---|
| T1 | Launch | one tab "New request" (primary), a **+** after it; URL `https://httpbin.org/get` as before; no `●` |
| T2 | Click **+** | second tab "New request" selected; URL and body empty; one empty params row and one empty headers row |
| T3 | In tab 2 type a URL; switch to tab 1 and back | each tab keeps its own URL, method, params, headers, body, and its Params/Headers/Body sub-tab |
| T4 | Type in tab 2's URL | tab 2 shows `● New request`; tab 1 does not |
| T5 | Tab 2: name `Second`, URL `https://httpbin.org/uuid`, Save. Tab 1: name `First`, Save | each tab drops its `●`; titles read `First` / `Second`; both files in `my-collection/` |
| T6 | Click `First` in the sidebar | switches to the existing `First` tab; no third tab |
| T7 | Click **+**, then click `Second` in the sidebar | `Second` gets focused (it's already open); the blank tab stays |
| T8 | Close the other tabs, open a blank tab with **+**, click `First` in the sidebar | `First` opens inside that blank tab (no extra tab) |
| T9 | With `First` open and edited (●), click `Second` in the sidebar | `Second` opens/focuses in its own tab; `First` keeps its unsaved edit |
| T10 | Send in tab `First`; switch to `Second` before it finishes (use `https://httpbin.org/delay/3`) | `First` shows `… First` while running; `Second`'s status line is its own; when done, the response appears in `First` only |
| T11 | Send `delay/3` in two tabs at once | both complete; each shows its own response, time and size |
| T12 | Send in a tab, then press **Cancel** | "Cancelled" in that tab only |
| T13 | Send `delay/5` in a tab and close the tab while it runs | tab closes, request is cancelled, no crash, other tabs unaffected |
| T14 | Edit a tab (●), click its **✕** | label becomes **Discard?**; status says "Unsaved changes…"; tab still open |
| T15 | Click **Discard?** | tab closes; the saved file on disk is unchanged |
| T16 | Edit (●), click **✕** (Discard?), then press Cmd+S | saves; `●` gone; button is back to **✕** |
| T17 | Arm Discard? on a tab, then click a different tab | the first tab's button returns to **✕** |
| T18 | Close a saved tab with no edits | closes on the first click |
| T19 | Close every tab | one fresh blank tab remains; the app never shows zero tabs |
| T20 | Close the active middle tab of three | the tab to its right becomes active; close the last one → the one to its left |
| T21 | Cmd+T, then Cmd+W (click into a text box first) | new blank tab; Cmd+W closes it (asks Discard? if you typed) |
| T22 | Rename a request in its tab (name field), Save | file renamed as before (S5); tab title updates; no duplicate tab when clicking it in the sidebar |
| T23 | Click a collection in the sidebar, then Save | the active tab's request is saved into that collection as a new file (same as S6) |
| T24 | Set an environment active, use `{{host}}` in two tabs, send both | both resolve; an unresolved name shows the amber warning only in that tab |
| T25 | Open Variables…, edit a value, click a request in the sidebar | variables autosaved; you're back on requests with that request's tab focused |
| T26 | Response viewer in tab A scrolled sideways / wrap on; switch to tab B and back | A keeps its wrap and scroll; B has its own |
| T27 | Copy button in one tab, then switch | "Copied ✓" belongs to that tab only |
| T28 | Name a request with ~40 characters | tab title is cut to 24 characters ending with `…` |
| T29 | Open ~15 tabs | tab bar scrolls sideways (trackpad / Shift+wheel); app stays responsive |
| T30 | Memory with 5 tabs open, each with a small response | still close to ~70 MB idle |
| T32 | Type in tab A, switch to tab B, then click A's ✕ from B | A's ✕ becomes **Discard?** *and* a message naming A appears in the status row you are looking at (it used to change silently) |
| T31 | Regression: rerun E4, E7, E10, E20 from doc 08 and S7, S11 from doc 07 | same results as before |

Report pass/fail per row. For anything that fails, a screenshot plus what you clicked helps most.
