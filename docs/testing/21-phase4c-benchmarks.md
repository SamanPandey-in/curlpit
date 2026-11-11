# 21 — Phase 4c: benchmarks and the budget gates

> Measured on your Mac over SSH. Unlike docs 19 and 20 this one needs no GUI clicking — it is here so the numbers are reproducible and so a later change that breaks a budget is caught by a command rather than by feel.

**New**
- A `benches/` crate (`at-benches`) with criterion benchmarks for the paths the NFR budgets name: body search, the viewer's line index, JSON pretty-printing and request building. This is the `benches/` directory the plan's section 2.1 asks for and the criterion coverage section 6 asks for.
- **It is deliberately *not* in `default-members`**, so `cargo test` and CI never compile criterion (D21's rule — CI stays fast). Confirmed: a plain `cargo test` build log mentions criterion zero times.
- **A budget-gate test** that measures the NFR-PERF-06 search budget with a wall clock and fails if it exceeds the 500 ms hard limit. It is `#[ignore]`d so CI's `cargo test` never runs it — a timing assertion on a shared runner would be flaky — and is meant to be run by hand when the viewer or the index changes.
- criterion is pinned to **0.5.1**, not the latest 0.8.2, because 0.8's `rust-version` is 1.86 and the workspace MSRV is 1.80 (D1).
- The sample bodies are generated in process from a fixed seed, so nothing large is committed and run-to-run numbers are comparable. The 10 MB body is 10,485,142 bytes / 439,411 rows every time.

## Running them
```
cd ~/inCoding/nextgen/api-tester
cargo test                                             # 149 passed; must not build criterion
cargo bench -p at-benches                               # the full set (slow; add --warm-up-time 1 --measurement-time 3 to skim)
cargo test -p at-benches -- --ignored --nocapture        # the NFR-PERF-06 budget gate
cargo test -p at-benches --release -- --ignored --nocapture   # same gate, closer to what ships
```

## Measured — MacBook Air M5, macOS 26.6 (25G72), rustc 1.99.1, criterion 0.5.1, 2026-11-09
Point estimates. Measured on a machine that was also running other cargo builds, so treat them as upper bounds rather than quiet-machine bests.

| What | Budget (SRS) | Measured | Verdict |
|---|---|---|---|
| Search 10 MB, first match near the start | ≤ 500 ms (target 200 ms) | **88 ns** | ✅ |
| Search 10 MB, first match at the very end (worst case) | ≤ 500 ms (target 200 ms) | **4.23 ms** | ✅ |
| Search 10 MB, no match at all | ≤ 500 ms | **4.25 ms** | ✅ |
| Search 10 MB, many matches, no limit | — | 5.20 ms | ✅ |
| Match offset → viewer row (`row_of`) | instant | 9.5 ns | ✅ |
| Line index, 10 MB, unwrapped | — | 8.11 ms | ✅ |
| Line index, 10 MB, wrapped at 80 cols | — | 8.08 ms | ✅ |
| Line index, 50 MB, unwrapped | — | 36.9 ms (±7% run to run) | ✅ |
| Line index, 50 MB, wrapped at 80 cols | — | 35.7 ms | ✅ |
| `count_chars`, 10 MB / 50 MB | — | 1.22 ms / 6.07 ms | ✅ |
| Pretty-print 1 MB / 10 MB JSON | — | 1.14 ms / 11.96 ms (~835 MiB/s) | ✅ |
| Request build, params + headers + basic auth | < 5 ms (NFR-PERF-03) | **1.37 µs** | ✅ |
| Request build, 25 headers | < 5 ms | **3.64 µs** | ✅ |
| Release binary size (`target/release/api-tester`) | ≤ 40 MB target, ≤ 100 MB installed | **14.4 MB** (15,092,240 bytes) | ✅ |

Budget gate, 10 MB body, worst case of 5 runs per case:

| Build | First match at the very end | No match | Limit |
|---|---|---|---|
| debug | 23.5 ms | 23.5 ms | 500 ms |
| release | 6.4 ms | 4.8 ms | 500 ms |

So NFR-PERF-06 passes with roughly **21x headroom unoptimised and 78x in release**.

## Caveats, so these numbers aren't over-read
- `cargo bench` uses `[profile.bench]`, i.e. plain `opt-level = 3` — **not** the shipping `[profile.release]` with fat LTO and one codegen unit. The budget-gate numbers under `--release` are the ones that reflect what users get.
- Synthetic ASCII JSON, and the search/index figures exclude rendering. The real test of the viewer is still the GUI checklist (doc 19, Q14).
- The 50 MB groups use 10 samples, so their spread is wider (±7% observed between runs).
- These are single-machine numbers. The CI budget gates the plan's section 6 wants (fail the build on regression) are **not wired up yet** — this crate is the measurement half; turning the gate into a CI job that compares against a stored baseline is still to do.

## Checklist
| # | Do | Expect |
|---|---|---|
| G1 | `cargo test` | 149 passed, and no criterion in the build output |
| G2 | `cargo bench -p at-benches -- --warm-up-time 1 --measurement-time 3` | all four bench targets run and print numbers in the same ballpark as the table |
| G3 | `cargo test -p at-benches -- --ignored --nocapture` | the gate prints each case and passes, worst case well under 500 ms |
| G4 | `cargo test -p at-benches --release -- --ignored --nocapture` | same, single-digit milliseconds |
| G5 | `ls -l target/release/api-tester` after `cargo build --release -p at-app` | about 14–15 MB |

No GUI involvement; just paste the output.
