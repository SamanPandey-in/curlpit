//! Budget gate for NFR-PERF-06: search a 10 MB body for the first match.
//! Hard limit 500 ms (asserted), target 200 ms or less (reported, not asserted).
//!
//! This is a plain wall-clock `#[test]`, not a criterion benchmark: it is the gate a human runs to
//! re-verify the budget on real hardware before recording a number in `docs/BASELINE.md`. It is
//! `#[ignore]`d so CI's `cargo test` never runs it: a timing assert is flaky on a shared runner.
//!
//! How to run it:
//!
//!     cargo test -p at-benches -- --ignored --nocapture

mod support;

use at_core::lines::{build_rows, count_chars, find_all, MAX_ROW_CHARS};
use std::hint::black_box;
use std::time::Instant;
use support::{pretty_body, MB, NEEDLE_ABSENT, NEEDLE_EARLY, NEEDLE_LATE};

/// SRS NFR-PERF-06 hard limit: the gate fails above this.
const HARD_LIMIT_MS: f64 = 500.0;
/// SRS NFR-PERF-06 target: reported so drift towards the hard limit is visible.
const TARGET_MS: f64 = 200.0;
/// Best of N: the gate asks what the machine can do, not what a noisy sample averages.
const RUNS: usize = 5;

#[test]
#[ignore = "wall-clock budget gate; run with: cargo test -p at-benches -- --ignored --nocapture"]
fn nfr_perf_06_first_match_in_10mb_is_under_the_hard_limit() {
    let body = pretty_body(10 * MB);
    let rows = build_rows(&body, MAX_ROW_CHARS);
    println!(
        "NFR-PERF-06 gate: body {} bytes ({} chars), {} rows; hard limit {HARD_LIMIT_MS} ms, target {TARGET_MS} ms",
        body.len(),
        count_chars(&body),
        rows.len(),
    );

    let cases: [(&str, &str); 3] = [
        ("first match near the start", NEEDLE_EARLY),
        ("first match at the very end (worst case)", NEEDLE_LATE),
        ("no match at all", NEEDLE_ABSENT),
    ];

    let mut worst_label = "";
    let mut worst_ms = 0.0f64;
    for (label, needle) in cases {
        let mut best_ms = f64::MAX;
        for _ in 0..RUNS {
            let started = Instant::now();
            let hits = find_all(&body, needle, 1);
            let row = hits.first().map(|&off| rows.row_of(off));
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            black_box((hits.len(), row));
            if elapsed_ms < best_ms {
                best_ms = elapsed_ms;
            }
        }
        let verdict = if best_ms <= TARGET_MS {
            "within target"
        } else if best_ms < HARD_LIMIT_MS {
            "over target, under hard limit"
        } else {
            "OVER HARD LIMIT"
        };
        println!("  {label}: {best_ms:.3} ms (best of {RUNS}) - {verdict}");
        if best_ms > worst_ms {
            worst_ms = best_ms;
            worst_label = label;
        }
    }

    println!("  worst case measured: {worst_ms:.3} ms ({worst_label})");
    assert!(
        worst_ms < HARD_LIMIT_MS,
        "NFR-PERF-06 exceeded: {worst_label} took {worst_ms:.3} ms, hard limit is {HARD_LIMIT_MS} ms"
    );
}
