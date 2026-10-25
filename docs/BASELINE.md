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
