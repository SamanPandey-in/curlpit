//! JSON pretty-printing throughput (FR-RES-04).
//!
//! This is the hot path when a response arrives: the body comes off the wire minified and the
//! viewer shows it pretty-printed, so `pretty_json` runs before first paint and counts against
//! NFR-PERF-04 (1 MB) and NFR-PERF-05 (10 MB). The input is one long minified line, which is what
//! a server actually sends.

mod support;

use at_core::pretty::pretty_json;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use support::{minified_body, MB};

/// The viewer's indent width.
const INDENT: usize = 2;

fn pretty_1mb(c: &mut Criterion) {
    let body = minified_body(MB);

    let mut g = c.benchmark_group("pretty_json_1mb");
    g.throughput(Throughput::Bytes(body.len() as u64));
    g.bench_function("indent_2", |b| {
        b.iter(|| pretty_json(black_box(&body), black_box(INDENT)))
    });
    g.finish();
}

fn pretty_10mb(c: &mut Criterion) {
    let body = minified_body(10 * MB);

    let mut g = c.benchmark_group("pretty_json_10mb");
    // Each iteration allocates ~1.5x the body; 20 samples keeps the wall clock sane.
    g.sample_size(20);
    g.throughput(Throughput::Bytes(body.len() as u64));
    g.bench_function("indent_2", |b| {
        b.iter(|| pretty_json(black_box(&body), black_box(INDENT)))
    });
    g.finish();
}

criterion_group!(benches, pretty_1mb, pretty_10mb);
criterion_main!(benches);
