# 14 — Phase 3h: session restore ("hot exit") and reopen closed tab

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit tests). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Your tabs come back.** Quit and relaunch: the same tabs are open in the same order, with the same tab active. Each tab shows exactly what it showed, including unsaved edits, which still carry their `●` so you can Save or Discard later. This works like VS Code's hot exit: quitting never loses work, so there's no "save changes?" prompt.
- Saved silently to `<data-dir>/session.toml`: a check every 3 seconds (written only when something changed), plus whenever the window closes or the app quits. A crash or force-quit loses at most the last ~3 seconds of typing.
- If a tab's file was **moved or deleted** while the app was closed, the tab reopens with your last version, marked unsaved, and a note saying so. Nothing is lost.
- If the request file was **edited outside the app**, a tab that had no unsaved edits shows the new file contents; a tab that had edits keeps your edits and is marked unsaved against the new file.
- **Cmd+Shift+T** (Ctrl+Shift+T) reopens the most recently closed tab, unsaved edits included. That covers tabs closed with **Discard?** too. The last 20 closed tabs are remembered while the app runs.
- Responses aren't stored in the session (they're in History). Untouched blank tabs aren't worth reopening and are skipped.
- If `session.toml` is corrupt, the app starts fresh with a note. If it's from a newer app version, it's left untouched and this run doesn't write one.

**Not yet:** remembering the window size/position, sidebar width, or the selected sub-tab per tab; restoring closed tabs across restarts.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 95 passed (32 core + 5 http + 17 store + 11 history + 12 interop + 18 codegen)
cargo test -p at-app            # expect 4 passed
API_TESTER_DATA_DIR=/tmp/at-test8 cargo run --release -p at-app
```
"Quit" below means closing the window (or Ctrl+C in the terminal that runs it, for the crash case).

## Checklist
| # | Do | Expect |
|---|---|---|
| R1 | First launch (empty data dir) | one starter tab as before; after ~3 s `cat /tmp/at-test8/session.toml` shows it |
| R2 | Open 3 tabs: a saved request, a new unsaved one (type a URL), one with an edit to a saved request (●). Make the 2nd active. Quit, relaunch | same 3 tabs, same order, 2nd active; the edited one still shows `●` with your edit; the saved one has no `●` |
| R3 | In the restored `●` tab, Save | `●` gone; file updated |
| R4 | Restored unsaved new tab: close it | asks **Discard?** (it's still unsaved) |
| R5 | Type in a tab, wait 4 s, then kill the app (`Ctrl+C` in its terminal), relaunch | the typed text is there |
| R6 | Type, then quit within 1 s by closing the window | the text is there (saved on close) |
| R7 | Quit; delete a request file that was open (`rm …/get-httpbin.toml`); relaunch | tab reopens with its content, `●`, note "… was moved or deleted; this is your last version (not saved)"; Save writes it back |
| R8 | Quit; edit an open request's file in a text editor (change the URL); relaunch | tab without unsaved edits shows the new URL, no `●` |
| R9 | Same as R8 but the tab had unsaved edits | your edits are shown, `●`; saving overwrites the outside change |
| R10 | Auth/body/settings (doc 13) in an unsaved tab, quit, relaunch | all restored (Basic auth fields, form-data rows, timeout) |
| R11 | Close a tab with edits via **Discard?**, then Cmd+Shift+T | it comes back with the edits and `●` |
| R12 | Close 3 tabs, press Cmd+Shift+T three times | they come back newest-first |
| R13 | Cmd+Shift+T on a tab whose request is already open again | just switches to it (no duplicate) |
| R14 | Cmd+Shift+T with nothing closed | status "No closed tabs to reopen" |
| R15 | `echo "= =" > /tmp/at-test8/session.toml`, relaunch | starter tab plus red "Last session could not be read, starting fresh: …"; a few seconds later the file is valid again |
| R16 | `printf 'schema_version = 99\n' > /tmp/at-test8/session.toml`, relaunch, quit | red "Last session not restored: …newer…"; the file still says 99 afterwards |
| R17 | `cat session.toml` with an unsaved Basic-auth tab | the password is in plain text, the same as request files (keychain comes later) |
| R18 | Leave the app idle; run `stat -f %Sm /tmp/at-test8/session.toml` twice, a minute apart | same time both runs: the file isn't rewritten while nothing changes |
| R19 | History-opened tab and a + cURL tab, quit, relaunch | both reopen with their request (responses aren't restored; History still has them) |
| R20 | Regression: T5, T14, T19 (doc 09), A21/A22 (doc 13) | same as before |

Report pass/fail per row; for R2/R7 a screenshot helps.
