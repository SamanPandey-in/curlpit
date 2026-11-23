# 25 — Phase 5c: the Settings screen

> Branch `p5`. Compiled and tested on your Mac over SSH — fmt, clippy `-D warnings`, 25 `at-app` tests, 20 `at-store` (incl. a settings round-trip and an old-format-loads-with-defaults test), whole suite green. The page itself is GUI, so **it hasn't been clicked yet**; this checklist is the first real test.

**New**
- **A Settings page (FR-SET-02, 03).** Open it from the **Settings** button (next to Cookies) or the command palette's "Go to Settings". It edits the app-wide defaults:
  - **Default timeout (ms)** and **TLS verification** — the defaults a *new* request starts with.
  - **History cap (entries)** — how many history rows are kept.
  - **Outbound proxy** — a proxy URL applied to every request, or blank for a direct connection (the app had no proxy support before).
  - **Data directory** — shown read-only (where your collections, history and settings live).
  - A line stating the app sends **no telemetry**.
  Press **Save** to persist.
- **How each one applies** (they differ, on purpose): proxy takes effect on the **next request**; default timeout and TLS seed **new request tabs** (existing tabs and each request's own Settings tab are unchanged); the history cap applies **next launch**.
- **Bug fixed along the way:** activating an environment used to rewrite the settings file from scratch. It now edits only the active-environment line, so your other settings survive (checked in S7 below).

## Known limits, stated plainly
- **Default timeout / TLS are defaults, not overrides.** Changing them does nothing to tabs already open or to a request whose own Settings tab you set — only new requests pick them up.
- **History cap applies next launch**, not instantly.
- **Proxy is best tested if you have a local proxy** (e.g. a dev proxy on `http://127.0.0.1:8080`). Without one, just confirm a bad proxy URL fails the request cleanly (S6).
- **Data directory is display-only** — moving it isn't supported here.
- Not here: font/editor settings and update-check opt-out (P1), high-contrast theme (P2).

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test -p at-app            # expect 25 passed
cargo test -p at-store          # expect 20 passed
cargo test                      # whole suite green
API_TESTER_DATA_DIR=/tmp/at-test17 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| S1 | Click **Settings** (next to Cookies) | the Settings page opens showing the current values; the data directory path is shown |
| S2 | Command palette (`Cmd/Ctrl+K`) → "Go to Settings" | opens the same page |
| S3 | Set Default timeout to `5000`, Save, then open a **new tab** and look at its Settings tab | the new request's timeout is 5000 |
| S4 | Toggle **TLS verification** to Off, Save, open a new tab | the new request's Settings show TLS verification off |
| S5 | Set History cap to `5`, Save, **restart** the app, send 6+ requests, open History | only the newest 5 are kept |
| S6 | Set Outbound proxy to a bad value like `http://nope:1`, Save, send any request | the request fails with a clear error, not a crash (if you have a real proxy, point at it and confirm traffic goes through) |
| S7 | **Regression (the fix):** set Default timeout to `12345`, Save; then activate an environment on the Variables page; reopen Settings | the timeout is still `12345` (activating an environment didn't reset it) |
| S8 | Enter letters in Default timeout, Save | a clear error; nothing is saved |
| S9 | Change a value, Save, fully **quit and relaunch**, open Settings | the saved value is still there |
| S10 | Confirm the "no telemetry" line is present | it is |
| S11 | Regression: everything else still works (send, save, tabs, import/export, palette) | unchanged |
| S12 | Anything that looks or feels wrong | say what and where |

Report pass/fail per row; S3/S4 (defaults reach new tabs) and S7 (the clobber fix) are the ones I most want confirmed.
