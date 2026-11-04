//! Row index for the response viewer.
//!
//! A *row* is one visual line. A *logical line* (what the user numbers) can span several rows when
//! it is wrapped: the first row carries the line number, continuation rows are flagged with `CONT`
//! so the viewer leaves their gutter blank and the numbers never shift. Rows are split on UTF-8
//! character boundaries, and we assume a monospace font (1 char = 1 column).

/// Set in `lines[ix]` when row `ix` is a continuation of the logical line above it.
pub const CONT: u32 = 0x8000_0000;

/// Without wrap, rows are still split at this many characters so a 5 MB minified line stays fast.
pub const MAX_ROW_CHARS: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowIndex {
    /// Byte offset where each row starts.
    pub starts: Vec<u32>,
    /// 1-based logical line number of each row, OR-ed with `CONT` for continuation rows.
    pub lines: Vec<u32>,
}

impl RowIndex {
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    pub fn row<'a>(&self, b: &'a [u8], ix: usize) -> &'a [u8] {
        let start = self.starts[ix] as usize;
        let end = self
            .starts
            .get(ix + 1)
            .map(|&e| e as usize)
            .unwrap_or(b.len());
        &b[start..end]
    }

    pub fn is_cont(&self, ix: usize) -> bool {
        self.lines[ix] & CONT != 0
    }

    pub fn line_no(&self, ix: usize) -> u32 {
        self.lines[ix] & !CONT
    }

    /// Index of the row with the most bytes (used to size the horizontal scroll area).
    pub fn longest_row(&self, b: &[u8]) -> usize {
        let (mut best_ix, mut best_len) = (0usize, 0usize);
        for ix in 0..self.len() {
            let len = self.row(b, ix).len();
            if len > best_len {
                best_len = len;
                best_ix = ix;
            }
        }
        best_ix
    }

    /// The row that contains byte `offset` (the last row for offsets past the end).
    pub fn row_of(&self, offset: usize) -> usize {
        self.starts
            .partition_point(|&s| s as usize <= offset)
            .saturating_sub(1)
    }
}

/// Byte offsets of non-overlapping matches of `needle` in `hay`, ASCII case-insensitive,
/// at most `limit` of them (searching stops there, so a huge body stays fast).
pub fn find_all(hay: &[u8], needle: &str, limit: usize) -> Vec<usize> {
    let n = needle.as_bytes();
    let mut out = Vec::new();
    if n.is_empty() || n.len() > hay.len() || limit == 0 {
        return out;
    }
    let first = n[0].to_ascii_lowercase();
    let mut i = 0;
    while i + n.len() <= hay.len() {
        if hay[i].to_ascii_lowercase() == first && hay[i..i + n.len()].eq_ignore_ascii_case(n) {
            out.push(i);
            if out.len() >= limit {
                break;
            }
            i += n.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Number of characters (not bytes) in a UTF-8 slice.
pub fn count_chars(b: &[u8]) -> usize {
    b.iter().filter(|&&c| (c & 0xC0) != 0x80).count()
}

/// Split `b` into rows of at most `max_chars` characters, honouring hard `\n` breaks.
pub fn build_rows(b: &[u8], max_chars: usize) -> RowIndex {
    let max_chars = max_chars.max(1);
    let mut starts: Vec<u32> = Vec::with_capacity(b.len() / 30 + 1);
    let mut lines: Vec<u32> = Vec::with_capacity(b.len() / 30 + 1);
    starts.push(0);
    lines.push(1);
    let (mut line_no, mut cols, mut i) = (1u32, 0usize, 0usize);
    while i < b.len() {
        let byte = b[i];
        if byte == b'\n' {
            if i + 1 < b.len() {
                line_no += 1;
                starts.push((i + 1) as u32);
                lines.push(line_no);
            }
            cols = 0;
        } else if (byte & 0xC0) != 0x80 {
            // a character starts here (never break inside a multi-byte character)
            if cols >= max_chars {
                starts.push(i as u32);
                lines.push(line_no | CONT);
                cols = 0;
            }
            cols += 1;
        }
        i += 1;
    }
    RowIndex { starts, lines }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_and_locate_matches() {
        let text = b"Hello world\n\"name\": \"HELLO\"\nhello-hello";
        let hits = find_all(text, "hello", 100);
        assert_eq!(hits, vec![0, 21, 28, 34]);
        assert_eq!(find_all(text, "hello", 2), vec![0, 21], "limit");
        assert!(find_all(text, "", 10).is_empty());
        assert!(find_all(b"ab", "abc", 10).is_empty());
        // non-overlapping
        assert_eq!(find_all(b"aaaa", "aa", 10), vec![0, 2]);
        // multi-byte text is matched byte-exact
        assert_eq!(find_all("café café".as_bytes(), "café", 10), vec![0, 6]);

        let rows = build_rows(text, MAX_ROW_CHARS);
        assert_eq!(rows.row_of(0), 0);
        assert_eq!(rows.row_of(21), 1);
        assert_eq!(rows.row_of(34), 2);
        assert_eq!(rows.row_of(10_000), 2, "past the end -> last row");

        // wrapped rows: the match lands on the continuation row that holds it
        let wrapped = build_rows(b"abcdefghij", 4);
        assert_eq!(wrapped.row_of(5), 1);
        assert_eq!(wrapped.row_of(9), 2);
    }

    fn rows_text(b: &[u8], r: &RowIndex) -> Vec<Vec<u8>> {
        (0..r.len()).map(|i| r.row(b, i).to_vec()).collect()
    }

    #[test]
    fn plain_lines() {
        let b = b"a\nbb\nccc\n";
        let r = build_rows(b, 100);
        assert_eq!(r.starts, vec![0, 2, 5]);
        assert_eq!(r.lines, vec![1, 2, 3]);
        assert_eq!(r.longest_row(b), 2);
    }

    #[test]
    fn empty_and_no_trailing_newline() {
        assert_eq!(
            build_rows(b"", 10),
            RowIndex {
                starts: vec![0],
                lines: vec![1]
            }
        );
        assert_eq!(
            build_rows(b"abc", 10),
            RowIndex {
                starts: vec![0],
                lines: vec![1]
            }
        );
    }

    #[test]
    fn long_line_becomes_continuation_rows() {
        let b = "x".repeat(25).into_bytes();
        let r = build_rows(&b, 10);
        assert_eq!(r.starts, vec![0, 10, 20]);
        assert_eq!(r.lines, vec![1, 1 | CONT, 1 | CONT]);
        assert!(!r.is_cont(0) && r.is_cont(1) && r.is_cont(2));
        assert_eq!(rows_text(&b, &r).concat(), b);
    }

    #[test]
    fn wrapped_line_does_not_shift_following_numbers() {
        // line 1 wraps into two rows; the next logical line must still be number 2
        let b = b"abcde\nfg\n";
        let r = build_rows(b, 3);
        assert_eq!(r.starts, vec![0, 3, 6]);
        assert_eq!(r.lines, vec![1, 1 | CONT, 2]);
        assert_eq!((r.line_no(0), r.line_no(1), r.line_no(2)), (1, 1, 2));
    }

    #[test]
    fn never_splits_utf8() {
        let s = "é".repeat(20);
        let b = s.as_bytes();
        let r = build_rows(b, 7);
        assert_eq!(r.starts, vec![0, 14, 28]);
        for row in rows_text(b, &r) {
            assert!(std::str::from_utf8(&row).is_ok());
        }
        assert_eq!(rows_text(b, &r).concat(), b);
    }

    #[test]
    fn longest_and_char_count() {
        let b = b"a\nbbb\ncc\n";
        assert_eq!(build_rows(b, 100).longest_row(b), 1);
        assert_eq!(count_chars("é✓".as_bytes()), 2);
    }
}
