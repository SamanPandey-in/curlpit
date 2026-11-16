# Phase 0 baseline (MacBook Air M5, macOS 26.6, rustc 1.99.1) — measured 2026-10-25

| Metric | Budget | Result | Verdict |
|---|---|---|---|
| GPUI empty window: first render after main() (with runtime shaders) | ≤ 500 ms | **347 ms** | ✅ |
| GPUI empty window: idle RSS | ≤ 120 MB | **70.1 MB** | ✅ |
| GPUI empty window: binary size | ≤ 40 MB | _still to report_ | ⏳ |
| GPUI resize feel | smooth | smooth (confirmed) | ✅ |
| GPUI empty window: 2nd launch first render | ≤ 500 ms | 84 ms (OS shader cache warm) | ✅ |
| GPUI virtualized viewer, 10 MB / 487k lines: first render | ≤ 500 ms | 108 ms | ✅ |
| GPUI virtualized viewer, 10 MB: RSS while scrolling | ≤ 250 MB | 84.2 MB, flat | ✅ |
| GPUI virtualized viewer, 50 MB / 2.4M lines: RSS | ≤ 250 MB | 130.5 MB (first 2 readings) | ✅ |
| Viewer text smoothness | 60 fps | smooth text confirmed | ✅ |
| 10 MB JSON: data + index in RAM | ≤ 250 MB | **14.8 MB** (1.9 MB index) | ✅ |
| 50 MB JSON: data + index in RAM | ≤ 250 MB | **60.8 MB** | ✅ |
| Fetch 60 visible lines | instant | 0.1–1.3 µs | ✅ |
| First match in 10 MB | ≤ 200 ms | 0.2 ms | ✅ |
| Count / worst-case search, 10 MB | ≤ 500 ms | 9.6 ms / 6.8 ms | ✅ |
| QuickJS init | small | 0.58 ms | ✅ |
| QuickJS 5M-iteration loop | — | 92 ms | ✅ |
| Sandbox: infinite loop / memory bomb / recursion | contained | stopped (1.0 s / 61 ms / 0.1 ms) | ✅ |
| Sandbox: require/process/fetch/XHR/std/os/Deno | undefined | all undefined | ✅ |

Caveats: the search/index numbers are for synthetic ASCII JSON and exclude rendering; the real test of the viewer is the GPUI spike below.

## Phase 4 measurements (MacBook Air M5, macOS 26.6 build 25G72, rustc 1.99.1, criterion 0.5.1) — measured 2026-11-09

Reproduce with `cargo bench -p at-benches` and `cargo test -p at-benches [--release] -- --ignored --nocapture`; details and caveats in `docs/testing/21-phase4c-benchmarks.md`.

| Metric | Budget | Result | Verdict |
|---|---|---|---|
| Release binary, whole app (`target/release/api-tester`) | ≤ 40 MB target, ≤ 100 MB installed | **14.4 MB** (15,092,240 bytes) | ✅ |
| Search 10 MB, first match near the start | ≤ 500 ms (target 200 ms) | 88 ns | ✅ |
| Search 10 MB, first match at the very end (worst case) | ≤ 500 ms (target 200 ms) | 4.23 ms | ✅ |
| Search 10 MB, no match at all | ≤ 500 ms | 4.25 ms | ✅ |
| Search 10 MB, many matches, unlimited | — | 5.20 ms | ✅ |
| Match offset → viewer row (`row_of`) | instant | 9.5 ns | ✅ |
| NFR-PERF-06 wall-clock gate, 10 MB, worst case | ≤ 500 ms | 23.5 ms debug / **6.4 ms release** | ✅ |
| Line index, 10 MB (unwrapped / wrapped at 80) | — | 8.11 ms / 8.08 ms | ✅ |
| Line index, 50 MB (unwrapped / wrapped at 80) | — | 36.9 ms / 35.7 ms (±7% run to run) | ✅ |
| `count_chars`, 10 MB / 50 MB | — | 1.22 ms / 6.07 ms | ✅ |
| Pretty-print JSON, 1 MB / 10 MB | — | 1.14 ms / 11.96 ms (~835 MiB/s) | ✅ |
| Request build (params, headers, basic auth) | < 5 ms (NFR-PERF-03) | 1.37 µs | ✅ |
| Request build, 25 headers | < 5 ms | 3.64 µs | ✅ |
| JSON tree index, 2 MB body | within the 250 MB ceiling | 156,003 nodes / 4.99 MB of index (32 B per node) | ✅ |

Caveats: `cargo bench` runs under `[profile.bench]` (plain `opt-level = 3`), **not** the shipping `[profile.release]` with fat LTO — the release gate row is the one that reflects a shipped build. Bodies are synthetic ASCII JSON from a fixed seed and the search/index figures exclude rendering, so the viewer's real behaviour is still the GUI checklist (doc 19, Q14). The machine was running other cargo builds during the run, so read these as upper bounds. The Phase 0 row above for the *empty-window spike's* binary size was never reported separately and is superseded in practice by the whole-app number here.
