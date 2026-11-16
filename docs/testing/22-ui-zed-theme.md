# 22 — UI: one theme, based on Zed's One

> Branch `ui`, built on top of `p4`. Compiled and tested on your Mac over SSH, but **nobody has looked at it** — `screencapture` over SSH fails ("could not create image from display"), so this whole pass was made blind, from Zed's published colour values rather than from seeing the result. That makes this checklist the first real review.

**What was wrong**
Colour came from two unrelated places: `gpui-component`'s stock theme via `cx.theme()`, and about 20 hard-coded Catppuccin literals (`c(0x…)`) sprinkled through `ui.rs`, the Find highlight, the JSON tree and several render modules. Nothing shared a palette, so nothing matched. Worse, `gpui-component`'s `Input` paints itself from a *highlight theme* that was left at the library's `default_light()` — a light text field on a dark window, which is probably the single ugliest thing you were looking at.

**New**
- **One palette: Zed's "One"**, both One Dark and One Light, taken verbatim from `zed-industries/zed`'s `assets/themes/one/one.json` — not approximated by eye. It lives in `crates/app/src/theme.rs` and is the only file in the crate that contains a hex colour (61 of them); everything else asks for a colour by *job* (`status_error()`, `json_string()`, `find_match_bg()`). There are zero `c(0x…)` literals left anywhere else.
- **All 103 of `gpui-component`'s theme fields are set**, as a struct literal with no `..Default::default()`, so a future library upgrade that adds a field breaks the build instead of quietly painting something a stock colour. That includes the `Input` highlight theme, which now follows the palette.
- **Light and dark follow the system**, live (FR-SET-01): flipping macOS appearance repaints without a restart. The Find highlight used to be fixed yellow with near-black text — the one thing hard-wired to dark — and now inverts properly.
- **Quieter chrome.** Selected sub-tabs used `primary()`, an accent fill, so six sub-tabs read as six calls to action; selection is now a quiet surface change with full-strength text, the way Zed does it. The tab bar, status row, sidebar and viewer gutter got the same treatment, on a 4 px spacing scale. Accent is now spent in one place per view — Send, and Save on the variables page.

## Known limits, stated plainly
- **Light mode is unverified by eye.** The tests prove no inverted or colliding colour pairs, but relative weight — whether One Light's amber reads well as a Find fill, for instance — is a judgement nobody has made yet.
- **Base font size is still the library's 16 px.** For a tool this dense that is chunky, and 14 px would read far more like Zed, but it shifts every row height and was not safe to change blind. Same for the window background colour, which is still the OS default before first paint.
- The chrome changed; **the layout did not**. Deliberately — see the recommendations at the end.

## Setup
```
cd ~/inCoding/nextgen/api-tester-ui     # the `ui` branch lives in its own worktree
cargo test                              # expect 149 passed
cargo test -p at-app                    # expect 20 passed (13 + 7 theme tests)
API_TESTER_DATA_DIR=/tmp/at-test14 cargo run --release -p at-app
```

## Checklist
| # | Do | Expect |
|---|---|---|
| U1 | Launch on a **dark** system appearance | the window reads as one piece: dark panes, hairline borders, no light-grey boxes anywhere |
| U2 | Look at the URL field, the name field and any Params/Headers row | text fields are **dark**, not light-on-dark (this was the worst of it) |
| U3 | Switch macOS to **Light** while the app is open (System Settings → Appearance) | the whole window repaints to One Light immediately, no restart, nothing left dark |
| U4 | In light mode, repeat U2 | fields and text read correctly; nothing is white-on-white |
| U5 | Request sub-tabs (Params / Headers / Body / Auth / Scripts / Settings) | the selected one is a quiet filled chip, the others plain; only one thing looks "selected" |
| U6 | Response sub-tabs (Body / Tree / Headers / Tests / Code) | same treatment, consistent with U5 |
| U7 | The **Send** button | the one strongly coloured control in the view; nothing else competes with it |
| U8 | The request tab bar with 3–4 tabs open | the active tab is the pane colour continuing upward; inactive tabs sit back on the bar |
| U9 | Send a request; look at the status row | status code coloured by class (2xx green, 3xx amber, 4xx/5xx red), the row itself quiet chrome |
| U10 | Response **Tree** on a JSON body | strings, numbers, booleans and keys are each distinct, and they match Zed's syntax colours rather than the old palette |
| U11 | **Find** in the response, type a query | matches highlighted, the current one distinct from the rest, text readable on **both** fills |
| U12 | Repeat U11 in light mode | still readable — report if the current-match fill looks muddy, this is the case I could not judge |
| U13 | The sidebar: hover a row, then select one | hover and selected are visibly different from each other and from the background |
| U14 | The viewer's line-number gutter | numbers recede (comment colour); the rule beside them is a hairline, not a hard line |
| U15 | Variables page, Cookies page, Runner page | all three use the same surfaces and spacing as the main view; no page looks like a different app |
| U16 | Anything that still looks wrong | say what and where — this is the point of the pass |
| U17 | Regression: send a request, save it, run the collection runner | behaviour unchanged; this was a visual pass only |

Report pass/fail per row, and for U16 a screenshot is worth far more than a description.

## Recommendations deliberately left out of this pass
Each one moves or resizes a control rather than recolouring it, so none was safe to do blind:
1. **Base font size 16 px → 14 px**, with matching row heights — the single biggest remaining "doesn't look like Zed" item.
2. **Set a window background colour** so the OS default never shows through at the edges or before first paint.
3. **The top row carries eight controls** (name, method, URL, Send, Save, Copy cURL, Env, Variables…, Cookies). Zed would move Env/Variables/Cookies into a menu or the status bar and give the URL field room.
4. **The request panel is a fixed 210 px tall**; a draggable splitter between request and response is the Zed answer.
5. **The sidebar is a fixed 260 px** with no drag handle.
6. **The two sub-tab rows are different idioms** — one floats above its panel, the other is wedged into the status row. One pattern would be better.
7. **`✕` and `+` glyphs should be icon buttons** (`gpui_component` ships `IconName`); swapping them changes hit areas.
8. If hover feedback on lists reads as *too* subtle, the accent token is deliberately quiet and is a one-line change in `theme.rs`.
