//! NFR-PERF-05 and D18: building the line-offset row index.
//!
//! `build_rows` runs once per response (and again on every wrap-width change), so it sits directly
//! in front of "first paint under 500 ms". Both the unwrapped case (rows split only at
//! `MAX_ROW_CHARS`, D22) and a wrapped case (80 columns) are measured, at 10 MB and 50 MB.
//! `count_chars` is the other half of D18's index cost.

mod support;

use at_core::lines::{build_rows, count_chars, MAX_ROW_CHARS};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use support::{pretty_body, MB};

/// A realistic wrap width for a narrow window (D22: window width / character width).
const WRAP_COLS: usize = 80;

fn index_10mb(c: &mut Criterion) {
    let body = pretty_body(10 * MB);

    let mut g = c.benchmark_group("index_build_10mb");
    g.throughput(Throughput::Bytes(body.len() as u64));
    g.bench_function("build_rows_unwrapped", |b| {
        b.iter(|| build_rows(black_box(&body), MAX_ROW_CHARS))
    });
    g.bench_function("build_rows_wrapped_80", |b| {
        b.iter(|| build_rows(black_box(&body), WRAP_COLS))
    });
    g.bench_function("count_chars", |b| b.iter(|| count_chars(black_box(&body))));
    g.finish();
}

fn index_50mb(c: &mut Criterion) {
    let body = pretty_body(50 * MB);

    let mut g = c.benchmark_group("index_build_50mb");
    // The 50 MB cases get criterion's minimum sample size so a full `cargo bench` stays tolerable;
    // each iteration allocates an index of roughly 1.4x the body size (D18).
    g.sample_size(10);
    g.throughput(Throughput::Bytes(body.len() as u64));
    g.bench_function("build_rows_unwrapped", |b| {
        b.iter(|| build_rows(black_box(&body), MAX_ROW_CHARS))
    });
    g.bench_function("build_rows_wrapped_80", |b| {
        b.iter(|| build_rows(black_box(&body), WRAP_COLS))
    });
    g.bench_function("count_chars", |b| b.iter(|| count_chars(black_box(&body))));
    g.finish();
}

criterion_group!(benches, index_10mb, index_50mb);
criterion_main!(benches);
