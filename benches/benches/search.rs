//! NFR-PERF-06: search a 10 MB body for the first match.
//! Hard limit 500 ms, target 200 ms or less (SRS section 5).
//!
//! Four shapes of search, because the cost is entirely about where (and whether) the needle is:
//! near the start (early exit), at the very end (full scan), absent (full scan, no exit), and a
//! needle that hits in every record (full scan plus one push per hit). `RowIndex::row_of` is
//! measured too: it is what turns a match offset into the viewer row to scroll to.

mod support;

use at_core::lines::{build_rows, find_all, MAX_ROW_CHARS};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use support::{pretty_body, MB, NEEDLE_ABSENT, NEEDLE_EARLY, NEEDLE_LATE, NEEDLE_MANY};

fn first_offset(hay: &[u8], needle: &str) -> usize {
    match find_all(hay, needle, 1).first() {
        Some(&off) => off,
        None => panic!("needle `{needle}` must be present in the generated body"),
    }
}

fn search_10mb(c: &mut Criterion) {
    let body = pretty_body(10 * MB);

    let mut g = c.benchmark_group("search_10mb");
    g.throughput(Throughput::Bytes(body.len() as u64));
    g.bench_function("first_match_near_start", |b| {
        b.iter(|| find_all(black_box(&body), black_box(NEEDLE_EARLY), 1))
    });
    g.bench_function("first_match_at_end", |b| {
        b.iter(|| find_all(black_box(&body), black_box(NEEDLE_LATE), 1))
    });
    g.bench_function("no_match", |b| {
        b.iter(|| find_all(black_box(&body), black_box(NEEDLE_ABSENT), 1))
    });
    g.bench_function("many_matches_unlimited", |b| {
        b.iter(|| find_all(black_box(&body), black_box(NEEDLE_MANY), usize::MAX))
    });
    g.finish();
}

fn row_of_10mb(c: &mut Criterion) {
    let body = pretty_body(10 * MB);
    let rows = build_rows(&body, MAX_ROW_CHARS);
    let early = first_offset(&body, NEEDLE_EARLY);
    let late = first_offset(&body, NEEDLE_LATE);

    let mut g = c.benchmark_group("row_of_10mb");
    g.bench_function("row_of_match_near_start", |b| {
        b.iter(|| rows.row_of(black_box(early)))
    });
    g.bench_function("row_of_match_at_end", |b| {
        b.iter(|| rows.row_of(black_box(late)))
    });
    g.finish();
}

criterion_group!(benches, search_10mb, row_of_10mb);
criterion_main!(benches);
