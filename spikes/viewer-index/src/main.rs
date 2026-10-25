//! Spike A (UI-independent half): can we hold a big JSON response as raw bytes + a line-offset index,
//! fetch visible lines instantly, and search it fast, with low RSS?
//! Usage: spike-viewer-index [size_mb=10]
use std::io::Write;
use std::time::Instant;

fn rss_mb() -> f64 {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .map(|kb| kb / 1024.0)
        .unwrap_or(-1.0)
}

/// Pretty-printed JSON array of records, written directly (no DOM).
fn generate(target: usize) -> Vec<u8> {
    let mut s: Vec<u8> = Vec::with_capacity(target + 1024);
    s.extend_from_slice(b"[\n");
    let mut i: u64 = 0;
    while s.len() < target {
        if i > 0 {
            s.extend_from_slice(b",\n");
        }
        write!(
            s,
            "  {{\n    \"id\": {i},\n    \"name\": \"user-{i}\",\n    \"email\": \"user{i}@example.com\",\n    \"active\": {},\n    \"score\": {}.{},\n    \"tags\": [\"alpha\", \"beta\", \"needle-{}\"],\n    \"address\": null\n  }}",
            i % 2 == 0,
            i % 100,
            i % 10,
            i % 1000
        )
        .unwrap();
        i += 1;
    }
    s.extend_from_slice(b"\n]\n");
    s
}

/// Start offset of every line. 4 bytes per line (u32 is enough below 4 GB).
fn index_lines(b: &[u8]) -> Vec<u32> {
    let mut v = Vec::with_capacity(b.len() / 30);
    v.push(0u32);
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' && i + 1 < b.len() {
            v.push((i + 1) as u32);
        }
    }
    v
}

fn line<'a>(b: &'a [u8], idx: &[u32], n: usize) -> &'a [u8] {
    let start = idx[n] as usize;
    let end = idx.get(n + 1).map(|&e| e as usize).unwrap_or(b.len());
    &b[start..end]
}

fn matches_at(hay: &[u8], i: usize, needle: &[u8]) -> bool {
    hay[i..i + needle.len()]
        .iter()
        .zip(needle)
        .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

fn find_first(hay: &[u8], needle: &str) -> Option<usize> {
    let n: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();
    if n.is_empty() {
        return None;
    }
    (0..=hay.len().saturating_sub(n.len())).find(|&i| hay[i].to_ascii_lowercase() == n[0] && matches_at(hay, i, &n))
}

fn count_all(hay: &[u8], needle: &str) -> usize {
    let n: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();
    if n.is_empty() || hay.len() < n.len() {
        return 0;
    }
    let (mut count, mut i) = (0, 0);
    while i + n.len() <= hay.len() {
        if hay[i].to_ascii_lowercase() == n[0] && matches_at(hay, i, &n) {
            count += 1;
            i += n.len();
        } else {
            i += 1;
        }
    }
    count
}

fn main() {
    let mb: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    println!("target size: {mb} MB   (RSS at start: {:.1} MB)", rss_mb());

    let t = Instant::now();
    let data = generate(mb * 1024 * 1024);
    println!("generate:    {:>9.1?}   {} bytes   RSS {:.1} MB", t.elapsed(), data.len(), rss_mb());

    let t = Instant::now();
    let idx = index_lines(&data);
    println!("index lines: {:>9.1?}   {} lines   RSS {:.1} MB   (index = {:.1} MB)", t.elapsed(), idx.len(), rss_mb(), (idx.len() * 4) as f64 / 1048576.0);

    let t = Instant::now();
    let mut total = 0usize;
    for n in (idx.len() / 2)..(idx.len() / 2 + 60) {
        total += line(&data, &idx, n).len(); // 60 visible lines in the middle
    }
    println!("fetch 60 visible lines: {:>9.1?}   ({total} bytes)", t.elapsed());

    let t = Instant::now();
    let first = find_first(&data, "NEEDLE-999");
    println!("first match 'NEEDLE-999': {:>9.1?}   at byte {:?}", t.elapsed(), first);

    let t = Instant::now();
    let c = count_all(&data, "needle-999");
    println!("count all 'needle-999':   {:>9.1?}   {c} matches", t.elapsed());

    let t = Instant::now();
    let c = count_all(&data, "zzzzzz-not-present");
    println!("worst case (no match):    {:>9.1?}   {c} matches", t.elapsed());

    println!("final RSS: {:.1} MB", rss_mb());
}
