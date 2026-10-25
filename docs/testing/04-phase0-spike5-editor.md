# 04 — Spike 5: body editor (gpui-component)

This is the biggest UI risk in the plan, so I want hard feedback. gpui-component is a third-party component library for GPUI; this spike uses its code editor (line numbers, JSON highlighting).

> Written against the 0.5 API from documentation excerpts, without a compiler. Compile errors are likely and useful: send me the first error block.

## Run
```
cd spikes
cargo run --release -p spike-editor-gpui          # 3 KB document
cargo run --release -p spike-editor-gpui -- 1024  # 1 MB document
ls -l target/release/spike-editor-gpui            # compare with spike-gpui-hello: this is the cost of the library
```

## Checklist (do it in the 3 KB run, then the 1 MB run)
| # | Try | Expect | Result (works / janky / missing) |
|---|---|---|---|
| 1 | JSON appears with colours and line numbers | yes | |
| 2 | Type fast, hold a key (key repeat) | no lag | |
| 3 | Type `{` and `"` | auto-close pairs (nice to have) | |
| 4 | Press Enter between `{` and `}` | auto-indent | |
| 5 | Select all, copy, paste somewhere else, paste back | works | |
| 6 | Cmd+Z, Cmd+Shift+Z | undo/redo | |
| 7 | Click in the middle, Shift+arrows select | works | |
| 8 | Scroll the 1 MB document | smooth | |
| 9 | Type in the middle of the 1 MB document | no lag | |
| 10 | Find (Cmd+F) | works / missing | |
| 11 | The text `{{base_url}}` in the first line: any special highlight? | no (expected; we'd add it ourselves) | |

Also report: first-render time, the 8 RSS readings, binary size, and the **look** (fonts, cursor, selection colours) — rough is fine, I just want to know whether it's a reasonable base.

## What I'll decide from this
- Everything works → use it for the body, script and GraphQL editors.
- Works but typing lags on 1 MB or key features are missing → I use it for small fields only and build a minimal rope-based editor for big bodies.
- Won't build / is far heavier → I build the editor myself from the start.
