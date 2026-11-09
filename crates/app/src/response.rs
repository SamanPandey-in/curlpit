//! The response side of a tab: how a body is turned into displayable bytes, the row
//! index that lets the viewer show a 200 MB body without touching all of it, and the
//! span splitting that highlights every Find match inside its line.

use at_core::lines::{build_rows, count_chars, RowIndex, MAX_ROW_CHARS};
use at_core::pretty::pretty_json;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ResTab {
    Body,
    Tree,
    Headers,
    Tests,
    Code,
}

/// Bytes to show for a response body: pretty-printed when it looks like JSON.
pub(crate) fn display_body(bytes: &[u8], content_type: &str) -> Vec<u8> {
    let first = bytes.iter().find(|ch| !ch.is_ascii_whitespace()).copied();
    if content_type.contains("json") || matches!(first, Some(b'{') | Some(b'[')) {
        pretty_json(bytes, 2)
    } else {
        bytes.to_vec()
    }
}

pub(crate) fn content_type(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

pub(crate) struct ViewData {
    pub(crate) data: Arc<Vec<u8>>,
    /// Rows without wrapping (hard lines, plus a safety split for monstrous lines).
    pub(crate) base: Arc<RowIndex>,
    /// Rows wrapped to a column count; rebuilt when the window width changes.
    pub(crate) wrapped: Option<(usize, Arc<RowIndex>)>,
    /// Characters in the longest row; sizes the horizontal scroll range.
    pub(crate) longest_chars: usize,
}

impl ViewData {
    /// Index a body for the viewer. Pure, so big bodies can be indexed off the UI thread.
    pub(crate) fn new(data: Vec<u8>) -> ViewData {
        let base = build_rows(&data, MAX_ROW_CHARS);
        let longest = base.longest_row(&data);
        let longest_chars = count_chars(base.row(&data, longest));
        ViewData {
            data: Arc::new(data),
            base: Arc::new(base),
            wrapped: None,
            longest_chars,
        }
    }

    /// The rows currently on screen (wrapped or not).
    pub(crate) fn rows(&self, wrap: bool) -> Arc<RowIndex> {
        match (&self.wrapped, wrap) {
            (Some((_, r)), true) => r.clone(),
            _ => self.base.clone(),
        }
    }
}

/// Largest spilled body loaded into the viewer (the viewer was measured at ~130 MB of RAM for
/// 50 MB); bigger ones can still be saved with "Save…".
pub(crate) const VIEW_MAX_BYTES: u64 = 200 * 1024 * 1024;

/// Most matches counted by Find (searching stops there).
pub(crate) const FIND_MAX: usize = 10_000;

/// A rendered piece of one viewer row: plain text, a Find match, or the current Find match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Span {
    Plain,
    Hit,
    Current,
}

/// Split one row into spans so each Find match is highlighted inside the line, not just the
/// whole row (FR-RES-05). `row_start` is the row's byte offset in the body, `hits` the sorted
/// match offsets from `find_all`, and `hit_len` the needle's byte length. Tabs are expanded
/// per span, so joining the spans gives exactly the text the row would otherwise show.
pub(crate) fn row_spans(
    raw: &[u8],
    row_start: usize,
    hits: &[usize],
    hit_len: usize,
    current: Option<usize>,
) -> Vec<(String, Span)> {
    let text = |b: &[u8]| String::from_utf8_lossy(b).replace('\t', "    ");
    if hit_len == 0 || hits.is_empty() {
        return vec![(text(raw), Span::Plain)];
    }
    let row_end = row_start + raw.len();
    // hits are sorted, so skip everything that ends before this row starts
    let from = hits.partition_point(|&h| h + hit_len <= row_start);
    let mut out: Vec<(String, Span)> = Vec::new();
    let mut at = 0usize;
    for &h in &hits[from..] {
        if h >= row_end {
            break;
        }
        // clamp to this row: a match can straddle a wrapped row boundary
        let end = (h + hit_len).saturating_sub(row_start).min(raw.len());
        if end <= at {
            continue;
        }
        let start = h.saturating_sub(row_start).min(raw.len()).max(at);
        if start > at {
            out.push((text(&raw[at..start]), Span::Plain));
        }
        let kind = if Some(h) == current {
            Span::Current
        } else {
            Span::Hit
        };
        out.push((text(&raw[start..end]), kind));
        at = end;
    }
    if at < raw.len() {
        out.push((text(&raw[at..]), Span::Plain));
    }
    if out.is_empty() {
        out.push((text(raw), Span::Plain));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_spans_highlight_each_match_in_the_row() {
        let row = b"id: hello, hello world";
        assert_eq!(
            row_spans(row, 0, &[], 0, None),
            vec![("id: hello, hello world".to_string(), Span::Plain)],
            "no query: one plain span"
        );

        // two matches in one row; the second one is the current match
        let got = row_spans(row, 0, &[4usize, 11], 5, Some(11));
        assert_eq!(
            got,
            vec![
                ("id: ".to_string(), Span::Plain),
                ("hello".to_string(), Span::Hit),
                (", ".to_string(), Span::Plain),
                ("hello".to_string(), Span::Current),
                (" world".to_string(), Span::Plain),
            ]
        );
        // joining the spans always reproduces the row
        let joined: String = got.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, String::from_utf8_lossy(row));

        // a row further down the body: offsets are relative to row_start
        assert_eq!(
            row_spans(b"hello", 100, &[100], 5, Some(100)),
            vec![("hello".to_string(), Span::Current)]
        );
        // matches belonging to other rows are ignored
        assert_eq!(
            row_spans(b"hello", 100, &[0, 50, 200], 5, None),
            vec![("hello".to_string(), Span::Plain)]
        );
        // a match straddling a wrapped row boundary is clamped to the visible part
        assert_eq!(
            row_spans(b"abcde", 0, &[3], 5, None),
            vec![
                ("abc".to_string(), Span::Plain),
                ("de".to_string(), Span::Hit),
            ]
        );
        // the same match seen from the next row: bytes 3..8 cover all of "fgh" at 5..8
        assert_eq!(
            row_spans(b"fgh", 5, &[3], 5, None),
            vec![("fgh".to_string(), Span::Hit)]
        );
        // a match that ends inside the row leaves the rest plain
        assert_eq!(
            row_spans(b"fgh", 5, &[3], 4, None),
            vec![
                ("fg".to_string(), Span::Hit),
                ("h".to_string(), Span::Plain),
            ]
        );
        // tabs are expanded inside each span, so columns still line up
        assert_eq!(
            row_spans(b"a\thi", 0, &[2], 2, None),
            vec![
                ("a    ".to_string(), Span::Plain),
                ("hi".to_string(), Span::Hit),
            ]
        );
        // multi-byte text is not split mid-character
        let uni = "caf\u{e9} caf\u{e9}".as_bytes();
        let got = row_spans(uni, 0, &[6], 5, None);
        assert_eq!(
            got.last().map(|(s, k)| (s.as_str(), *k)),
            Some(("caf\u{e9}", Span::Hit))
        );
    }
}
