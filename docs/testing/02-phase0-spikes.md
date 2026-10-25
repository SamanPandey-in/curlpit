# 02 — Phase 0 spikes (GPUI go/no-go measurements)

Three throw-away programs in `spikes/` (their own workspace, so they don't affect the main build). **Run all in release mode** — debug numbers are meaningless for budgets.

> I wrote these without a compiler. Any compile error is expected feedback: send me the first error block.

## macOS prerequisite: NO full Xcode needed
GPUI normally compiles its Metal shaders at build time, which needs Xcode's `metal` tool. Its `runtime_shaders` feature (already enabled in `spikes/gpui-hello/Cargo.toml`) instead compiles them when the app starts, using the Metal compiler built into macOS. You only need the small **Command Line Tools** you already have (`xcode-select -p` should print a path; if not, `xcode-select --install`).

Trade-off to watch: shader compilation at launch adds to startup time, so the "first render" number below is the one that tells us if this is acceptable.

## Spike 1 — Empty GPUI window  (first build takes several minutes)
```
cd spikes
cargo run --release -p spike-gpui-hello
```
- A dark window titled with "Hello, API Tester!" should open. Leave it alone for 5 s, then close it.
- The terminal prints `first render: … after main()` and `idle RSS after 4s: … MB`.
- Size: `ls -l target/release/spike-gpui-hello` (in `spikes/target`).
- Feel check (subjective, please be honest): drag-resize the window — smooth? Any flicker or lag?

| Metric | Budget | Report |
|---|---|---|
| first render after main() | ≤ 500 ms | |
| idle RSS | ≤ 120 MB | |
| binary size | ≤ 40 MB | |
| resize feel | smooth | |

## Spike 2 — Big-response data structure (UI-independent)  (≈2 min)
```
cargo run --release -p spike-viewer-index          # 10 MB
cargo run --release -p spike-viewer-index -- 50    # 50 MB
```
Paste me the full output. Budgets: first match ≤ 200 ms, count/worst-case search ≤ 500 ms (10 MB), fetching 60 visible lines is effectively instant, RSS ≤ ~2.5× the data size.

## Spike 3 — Script engine (QuickJS)  (≈2 min)
```
cargo run --release -p spike-script-quickjs
```
Expected: `infinite loop` and `memory bomb` and `deep recursion` all report "stopped/error" within about a second or two; `forbidden globals` prints only `undefined` values (`undefined,undefined,undefined,...`). **If any of those hang or crash, that's a ❌ and the most important finding.** Paste the full output, plus `ls -l target/release/spike-script-quickjs`.

## Also still open from checkpoint 01 (please run when convenient)
- Error messages (DNS / refused / bad URL), Ctrl+C cancel, 20 MB spill + memory — items B5–B7 in `01-phase1-core-engine.md`.
- `cargo fmt --all` then commit (formatting differences only, no bug).
