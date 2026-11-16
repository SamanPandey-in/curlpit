//! Deterministic synthetic bodies for the api-tester benchmarks.
//!
//! Nothing is read from disk and nothing is downloaded: every body is generated in process from a
//! fixed seed, so a 10 MB body is byte-identical from run to run (numbers stay comparable) and no
//! multi-megabyte fixture has to be committed.
#![allow(dead_code)]

pub const MB: usize = 1024 * 1024;

/// Planted in the FIRST record only: the "first match near the start" case (NFR-PERF-06).
pub const NEEDLE_EARLY: &str = "zzz_start_marker_zzz";
/// Planted in the LAST record only: the worst case, where the whole body is scanned.
pub const NEEDLE_LATE: &str = "zzz_end_marker_zzz";
/// Never generated: the "no match at all" case (also a full scan, plus no early exit).
pub const NEEDLE_ABSENT: &str = "zzz_absent_marker_zzz";
/// A field name that occurs in every record: the many-match case.
pub const NEEDLE_MANY: &str = "email";

/// xorshift64* with a fixed seed. Not cryptographic, just repeatable.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> usize {
        (self.next_u64() % n) as usize
    }
}

/// A pretty-printed synthetic JSON body of roughly `target_bytes` bytes: many short lines, which
/// is what the row index and the viewer actually deal with.
pub fn pretty_body(target_bytes: usize) -> Vec<u8> {
    body(target_bytes, true)
}

/// The same data minified onto one very long line: what a server actually sends, and therefore the
/// input to `pretty_json`.
pub fn minified_body(target_bytes: usize) -> Vec<u8> {
    body(target_bytes, false)
}

fn body(target_bytes: usize, pretty: bool) -> Vec<u8> {
    let mut rng = Rng::new(0x2026_1109_abcd_0001);
    let mut out: Vec<u8> = Vec::with_capacity(target_bytes + 2048);
    out.extend_from_slice(b"{\"generated\":true,\"records\":[");
    // The first record carries the early needle, the last one the late needle, so "first match at
    // the very end" really does have to scan every byte.
    let mut id: u64 = 0;
    push_record(&mut out, &mut rng, id, NEEDLE_EARLY, pretty);
    id += 1;
    // Leave room for the closing record so the late needle lands inside the last kilobyte.
    while out.len() + 1024 < target_bytes {
        out.push(b',');
        push_record(&mut out, &mut rng, id, "plain", pretty);
        id += 1;
    }
    out.push(b',');
    push_record(&mut out, &mut rng, id, NEEDLE_LATE, pretty);
    out.extend_from_slice(b"]}");
    if pretty {
        out.push(b'\n');
    }
    out
}

fn nl(out: &mut Vec<u8>, pretty: bool, pad: usize) {
    if pretty {
        out.push(b'\n');
        out.resize(out.len() + pad, b' ');
    }
}

fn push_num(out: &mut Vec<u8>, n: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut v = n;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&buf[i..]);
}

fn push_hex(out: &mut Vec<u8>, n: u64, digits: usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for d in (0..digits).rev() {
        let nibble = ((n >> (4 * d)) & 0xf) as usize;
        out.push(HEX[nibble]);
    }
}

fn push_record(out: &mut Vec<u8>, rng: &mut Rng, id: u64, marker: &str, pretty: bool) {
    const FIRST: [&str; 8] = [
        "ada", "grace", "alan", "edsger", "barbara", "linus", "margaret", "ken",
    ];
    const LAST: [&str; 8] = [
        "lovelace", "hopper", "turing", "dijkstra", "liskov", "torvalds", "hamilton", "thompson",
    ];
    const TAGS: [&str; 8] = [
        "alpha", "beta", "gamma", "delta", "stable", "legacy", "canary", "archived",
    ];
    const REGIONS: [&str; 4] = ["eu-west-1", "us-east-1", "ap-south-1", "sa-east-1"];

    let first = FIRST[rng.below(8)];
    let last = LAST[rng.below(8)];
    let region = REGIONS[rng.below(4)];
    let tag_a = TAGS[rng.below(8)];
    let tag_b = TAGS[rng.below(8)];
    let uuid = rng.next_u64();
    let whole = rng.below(100) as u64;
    let frac = rng.below(100) as u64;
    let retries = rng.below(5) as u64;
    let active = rng.below(2) == 0;

    nl(out, pretty, 4);
    out.push(b'{');

    nl(out, pretty, 6);
    out.extend_from_slice(b"\"id\":");
    push_num(out, id);

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"uuid\":\"");
    push_hex(out, uuid, 16);
    out.push(b'"');

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"name\":\"");
    out.extend_from_slice(first.as_bytes());
    out.push(b' ');
    out.extend_from_slice(last.as_bytes());
    out.push(b'"');

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"email\":\"");
    out.extend_from_slice(first.as_bytes());
    out.push(b'.');
    out.extend_from_slice(last.as_bytes());
    push_num(out, id);
    out.extend_from_slice(b"@example.test\"");

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"marker\":\"");
    out.extend_from_slice(marker.as_bytes());
    out.push(b'"');

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"score\":");
    push_num(out, whole);
    out.push(b'.');
    if frac < 10 {
        out.push(b'0');
    }
    push_num(out, frac);

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"active\":");
    out.extend_from_slice(if active { b"true" } else { b"false" });

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"tags\":[\"");
    out.extend_from_slice(tag_a.as_bytes());
    out.extend_from_slice(b"\",\"");
    out.extend_from_slice(tag_b.as_bytes());
    out.extend_from_slice(b"\"]");

    out.push(b',');
    nl(out, pretty, 6);
    out.extend_from_slice(b"\"meta\":{");
    nl(out, pretty, 8);
    out.extend_from_slice(b"\"region\":\"");
    out.extend_from_slice(region.as_bytes());
    out.extend_from_slice(b"\",");
    nl(out, pretty, 8);
    out.extend_from_slice(b"\"created\":\"2026-11-09T12:34:56Z\",");
    nl(out, pretty, 8);
    out.extend_from_slice(b"\"retries\":");
    push_num(out, retries);
    nl(out, pretty, 6);
    out.push(b'}');

    nl(out, pretty, 4);
    out.push(b'}');
}
