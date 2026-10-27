//! Line index for the response viewer: byte offset of every (soft) line start.
//! Very long lines are split every `MAX_LINE` bytes (on a UTF-8 boundary) so no single row
//! is huge to shape or measure (e.g. a 5 MB minified HTML body).

pub const MAX_LINE: usize = 4000;

pub fn index_lines(b: &[u8]) -> Vec<u32> {
    index_lines_max(b, MAX_LINE)
}

pub fn index_lines_max(b: &[u8], max_line: usize) -> Vec<u32> {
    let mut v: Vec<u32> = Vec::with_capacity(b.len() / 30 + 1);
    v.push(0);
    let (mut line_start, mut i) = (0usize, 0usize);
    while i < b.len() {
        if b[i] == b'\n' {
            if i + 1 < b.len() {
                v.push((i + 1) as u32);
            }
            line_start = i + 1;
            i += 1;
        } else if i - line_start >= max_line {
            let mut j = i;
            while j < b.len() && (b[j] & 0xC0) == 0x80 {
                j += 1; // never split inside a UTF-8 character
            }
            if j < b.len() {
                v.push(j as u32);
            }
            line_start = j;
            i = j;
        } else {
            i += 1;
        }
    }
    v
}

/// Index of the longest line (by bytes). Used to size the horizontal scroll area.
pub fn longest_line(b: &[u8], idx: &[u32]) -> usize {
    let (mut best_ix, mut best_len) = (0usize, 0usize);
    for ix in 0..idx.len() {
        let end = idx.get(ix + 1).map(|&e| e as usize).unwrap_or(b.len());
        let len = end - idx[ix] as usize;
        if len > best_len {
            best_len = len;
            best_ix = ix;
        }
    }
    best_ix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(b: &[u8], idx: &[u32]) -> Vec<Vec<u8>> {
        (0..idx.len())
            .map(|i| {
                let end = idx.get(i + 1).map(|&e| e as usize).unwrap_or(b.len());
                b[idx[i] as usize..end].to_vec()
            })
            .collect()
    }

    #[test]
    fn plain_lines() {
        let b = b"a\nbb\nccc\n";
        let idx = index_lines(b);
        assert_eq!(idx, vec![0, 2, 5]);
        assert_eq!(longest_line(b, &idx), 2);
    }

    #[test]
    fn no_trailing_newline_and_empty() {
        assert_eq!(index_lines(b"abc"), vec![0]);
        assert_eq!(index_lines(b""), vec![0]);
    }

    #[test]
    fn long_lines_split_and_content_preserved() {
        let b = "x".repeat(25).into_bytes();
        let idx = index_lines_max(&b, 10);
        assert_eq!(idx, vec![0, 10, 20]);
        let joined: Vec<u8> = lines(&b, &idx).concat();
        assert_eq!(joined, b);
    }

    #[test]
    fn never_splits_utf8() {
        let s = "é".repeat(20);
        let b = s.as_bytes();
        let idx = index_lines_max(b, 7);
        for l in lines(b, &idx) {
            assert!(std::str::from_utf8(&l).is_ok(), "split inside a character");
        }
        assert_eq!(lines(b, &idx).concat(), b);
    }
}
