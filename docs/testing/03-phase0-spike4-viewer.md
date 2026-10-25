# 03 — Spike 4: the real virtualized viewer + remaining Phase 0 numbers

Goal: prove GPUI can scroll a 10 MB (≈487,000-line) response smoothly within the RAM budget, and close the two open items from the empty window.

## A. Remaining numbers from spike 1 (1 min)
```
cd spikes
ls -l target/release/spike-gpui-hello
```
Report the size. Also run it again and drag-resize the window: smooth, or any flicker/lag?

## B. Big viewer
```
cargo run --release -p spike-viewer-gpui          # 10 MB
```
A dark window with line numbers and JSON appears. The terminal says "SCROLL HARD NOW" and prints RSS every 5 s for 40 s.

During those 40 s:
1. Trackpad-flick the content quickly from top to bottom and back, several times.
2. Drag the scrollbar (if one shows) to the middle and the end.
3. Resize the window while scrolling.

Report:
| Item | Budget | Result |
|---|---|---|
| first render time printed | ≤ 500 ms (includes 10 MB generation) | |
| RSS readings (all 8) | ≤ 250 MB, should be flat | |
| Scroll smoothness | 60 fps, no stutter | smooth / stutters / janky (describe) |
| Text readable, line numbers aligned | yes | |

Then repeat with `-- 50` (50 MB) and tell me whether it still feels the same.

Also try: is text selectable / can you press Cmd+C? (Expected: **no** — selection and copy are things we'll build; I just want to confirm the baseline.)
