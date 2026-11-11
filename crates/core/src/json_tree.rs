//! No-DOM JSON tree index for the response viewer (FR-RES-03).
//!
//! [`JsonTree::parse`] scans the body once and records one [`Node`] per JSON value, in document
//! order, holding only `u32` byte offsets into the caller's slice — never a copy of the data. A
//! 10 MB body therefore costs a `Vec<Node>` instead of a value tree (NFR-PERF-05, D18), the same
//! philosophy as [`crate::lines`] and [`crate::pretty`]. Every accessor that needs text takes the
//! original `src` back and converts at the edge (lossily, so non-UTF-8 bytes cannot panic).
//!
//! The scanner is iterative with an explicit stack: response bodies are untrusted, so nesting is
//! bounded by [`MAX_DEPTH`] and reported as an error instead of recursing into a stack overflow.
//!
//! Colours are the UI's job; a node only reports its [`Kind`].
//!
//! Strictness: full RFC 8259 grammar — rejects trailing data, unterminated strings and containers,
//! trailing commas, `NaN`/`Infinity`, leading zeros and bad escapes. Two deliberate relaxations:
//! a leading UTF-8 BOM is skipped, and raw control bytes inside strings are accepted (real servers
//! emit them and the viewer must still show the body).

use std::ops::Range;

/// Deepest container nesting [`JsonTree::parse`] accepts; deeper input is rejected, never recursed.
pub const MAX_DEPTH: usize = 1024;

/// Value of [`Node::parent`] for the root node: it has no parent.
pub const NO_PARENT: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

impl Kind {
    pub fn is_container(self) -> bool {
        matches!(self, Kind::Object | Kind::Array)
    }
}

/// One JSON value. 32 bytes; see the `node_size_budget` test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: Kind,
    /// 0 for the root value, +1 for each enclosing container.
    pub depth: u16,
    /// Byte range of the key's *contents* in `src` (no quotes, still escaped) for object members;
    /// `None` for the root and for array elements. Use [`JsonTree::key_text`] to read it.
    pub key: Option<Range<u32>>,
    /// Byte range in `src` of the scalar literal (quotes included for strings), or of the whole
    /// `{..}` / `[..]` span including both brackets.
    pub value: Range<u32>,
    /// Members or elements directly inside a container; 0 for scalars.
    pub child_count: u32,
    /// Index of the enclosing container, or [`NO_PARENT`] for the root.
    pub parent: u32,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JsonTreeError {
    #[error("body is too large to index ({0} bytes; the limit is 4 GiB)")]
    TooLarge(usize),
    #[error("unexpected byte {byte:#04x} at offset {offset}")]
    Unexpected { offset: u32, byte: u8 },
    #[error("unexpected end of input at offset {offset}")]
    UnexpectedEof { offset: u32 },
    #[error("unterminated string starting at offset {offset}")]
    UnterminatedString { offset: u32 },
    #[error("invalid escape at offset {offset}")]
    InvalidEscape { offset: u32 },
    #[error("invalid number at offset {offset}")]
    InvalidNumber { offset: u32 },
    #[error("trailing data after the JSON value at offset {offset}")]
    TrailingData { offset: u32 },
    #[error("nesting deeper than {limit} levels at offset {offset}")]
    TooDeep { offset: u32, limit: usize },
}

/// An index of every value in one JSON document, in document order (so the parent of a node always
/// comes before it, and a container's whole subtree is contiguous). The root is node 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonTree {
    nodes: Vec<Node>,
}

/// One open container while scanning.
struct Frame {
    ix: u32,
    object: bool,
}

fn skip_ws(src: &[u8], mut i: usize) -> usize {
    while let Some(&b) = src.get(i) {
        if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        } else {
            break;
        }
    }
    i
}

fn unexpected(src: &[u8], i: usize) -> JsonTreeError {
    match src.get(i) {
        Some(&byte) => JsonTreeError::Unexpected {
            offset: i as u32,
            byte,
        },
        None => JsonTreeError::UnexpectedEof { offset: i as u32 },
    }
}

/// `i` points at an opening quote. Returns the range of the contents and the index just past the
/// closing quote.
fn scan_string(src: &[u8], i: usize) -> Result<(Range<u32>, usize), JsonTreeError> {
    let start = i + 1;
    let mut j = start;
    while let Some(&b) = src.get(j) {
        match b {
            b'"' => return Ok((start as u32..j as u32, j + 1)),
            b'\\' => match src.get(j + 1) {
                Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => j += 2,
                Some(b'u') => {
                    let hex = src
                        .get(j + 2..j + 6)
                        .ok_or(JsonTreeError::InvalidEscape { offset: j as u32 })?;
                    if !hex.iter().all(u8::is_ascii_hexdigit) {
                        return Err(JsonTreeError::InvalidEscape { offset: j as u32 });
                    }
                    j += 6;
                }
                Some(_) => return Err(JsonTreeError::InvalidEscape { offset: j as u32 }),
                None => {
                    return Err(JsonTreeError::UnterminatedString { offset: i as u32 });
                }
            },
            _ => j += 1,
        }
    }
    Err(JsonTreeError::UnterminatedString { offset: i as u32 })
}

/// `i` points at the first byte of a number. Returns the index just past it.
fn scan_number(src: &[u8], i: usize) -> Result<usize, JsonTreeError> {
    let bad = JsonTreeError::InvalidNumber { offset: i as u32 };
    let digits = |src: &[u8], mut j: usize| {
        let from = j;
        while matches!(src.get(j), Some(b'0'..=b'9')) {
            j += 1;
        }
        (j, j > from)
    };
    let mut j = i;
    if src.get(j) == Some(&b'-') {
        j += 1;
    }
    match src.get(j) {
        Some(b'0') => j += 1,
        Some(b'1'..=b'9') => j = digits(src, j).0,
        _ => return Err(bad),
    }
    if src.get(j) == Some(&b'.') {
        let (end, any) = digits(src, j + 1);
        if !any {
            return Err(bad);
        }
        j = end;
    }
    if matches!(src.get(j), Some(b'e' | b'E')) {
        j += 1;
        if matches!(src.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let (end, any) = digits(src, j);
        if !any {
            return Err(bad);
        }
        j = end;
    }
    Ok(j)
}

fn scan_word(src: &[u8], i: usize, word: &[u8]) -> Result<usize, JsonTreeError> {
    if src.get(i..i + word.len()) == Some(word) {
        Ok(i + word.len())
    } else {
        Err(unexpected(src, i))
    }
}

/// `i` points at an object member's opening quote. Stores the key's contents in `key` and returns
/// the index of the member's value (just past the `:`).
fn read_key(src: &[u8], i: usize, key: &mut Option<Range<u32>>) -> Result<usize, JsonTreeError> {
    if src.get(i) != Some(&b'"') {
        return Err(unexpected(src, i));
    }
    let (range, after) = scan_string(src, i)?;
    let colon = skip_ws(src, after);
    if src.get(colon) != Some(&b':') {
        return Err(unexpected(src, colon));
    }
    *key = Some(range);
    Ok(colon + 1)
}

fn push_node(nodes: &mut Vec<Node>, node: Node) {
    // `NO_PARENT` is out of bounds, so the root bumps nothing.
    if let Some(parent) = nodes.get_mut(node.parent as usize) {
        parent.child_count += 1;
    }
    nodes.push(node);
}

/// Closes the innermost container, whose closing bracket is the byte at `at`.
fn close_frame(nodes: &mut [Node], stack: &mut Vec<Frame>, at: usize) {
    if let Some(frame) = stack.pop() {
        if let Some(node) = nodes.get_mut(frame.ix as usize) {
            node.value.end = at as u32 + 1;
        }
    }
}

fn slice<'a>(src: &'a [u8], range: &Range<u32>) -> &'a [u8] {
    let start = (range.start as usize).min(src.len());
    let end = (range.end as usize).clamp(start, src.len());
    &src[start..end]
}

impl JsonTree {
    /// Indexes one complete JSON document. Whitespace-insensitive, so the raw and the
    /// pretty-printed form of a body give the same tree.
    pub fn parse(src: &[u8]) -> Result<Self, JsonTreeError> {
        if src.len() > u32::MAX as usize {
            return Err(JsonTreeError::TooLarge(src.len()));
        }
        let mut nodes: Vec<Node> = Vec::new();
        let mut stack: Vec<Frame> = Vec::new();
        let mut key: Option<Range<u32>> = None;
        let mut i = if src.starts_with(&[0xEF, 0xBB, 0xBF]) {
            3
        } else {
            0
        };
        let mut want_value = true;
        loop {
            if want_value {
                i = skip_ws(src, i);
                let b = *src.get(i).ok_or_else(|| unexpected(src, i))?;
                let depth = stack.len() as u16;
                let parent = stack.last().map_or(NO_PARENT, |f| f.ix);
                let ix = nodes.len() as u32;
                if b == b'{' || b == b'[' {
                    if stack.len() >= MAX_DEPTH {
                        return Err(JsonTreeError::TooDeep {
                            offset: i as u32,
                            limit: MAX_DEPTH,
                        });
                    }
                    let object = b == b'{';
                    push_node(
                        &mut nodes,
                        Node {
                            kind: if object { Kind::Object } else { Kind::Array },
                            depth,
                            key: key.take(),
                            // `end` is patched in by `close_frame`.
                            value: i as u32..i as u32,
                            child_count: 0,
                            parent,
                        },
                    );
                    stack.push(Frame { ix, object });
                    i = skip_ws(src, i + 1);
                    let close = if object { b'}' } else { b']' };
                    if src.get(i) == Some(&close) {
                        close_frame(&mut nodes, &mut stack, i);
                        i += 1;
                        want_value = false;
                    } else if object {
                        i = read_key(src, i, &mut key)?;
                    }
                    // An array's first element is read by the next turn of the loop.
                } else {
                    let (kind, end) = match b {
                        b'"' => (Kind::String, scan_string(src, i)?.1),
                        b'-' | b'0'..=b'9' => (Kind::Number, scan_number(src, i)?),
                        b't' => (Kind::Bool, scan_word(src, i, b"true")?),
                        b'f' => (Kind::Bool, scan_word(src, i, b"false")?),
                        b'n' => (Kind::Null, scan_word(src, i, b"null")?),
                        _ => return Err(unexpected(src, i)),
                    };
                    push_node(
                        &mut nodes,
                        Node {
                            kind,
                            depth,
                            key: key.take(),
                            value: i as u32..end as u32,
                            child_count: 0,
                            parent,
                        },
                    );
                    i = end;
                    want_value = false;
                }
            } else {
                if stack.is_empty() {
                    break;
                }
                i = skip_ws(src, i);
                let b = *src.get(i).ok_or_else(|| unexpected(src, i))?;
                let object = stack.last().is_some_and(|f| f.object);
                match b {
                    b',' => {
                        i = skip_ws(src, i + 1);
                        if object {
                            i = read_key(src, i, &mut key)?;
                        }
                        want_value = true;
                    }
                    b'}' if object => {
                        close_frame(&mut nodes, &mut stack, i);
                        i += 1;
                    }
                    b']' if !object => {
                        close_frame(&mut nodes, &mut stack, i);
                        i += 1;
                    }
                    _ => return Err(unexpected(src, i)),
                }
            }
        }
        i = skip_ws(src, i);
        if i != src.len() {
            return Err(JsonTreeError::TrailingData { offset: i as u32 });
        }
        Ok(JsonTree { nodes })
    }

    /// Every value in document order; node 0 is the root.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The member name of node `ix`, unescaped. `None` for the root and for array elements.
    pub fn key_text(&self, src: &[u8], ix: u32) -> Option<String> {
        let key = self.nodes.get(ix as usize)?.key.clone()?;
        Some(unescape(slice(src, &key)))
    }

    /// Text for *copy value*: strings are unescaped (`\n`, `\t`, `\uXXXX` including surrogate
    /// pairs; a lone surrogate becomes U+FFFD), numbers / `true` / `false` / `null` are verbatim,
    /// and a container yields its raw source span, i.e. that subtree's JSON. *Copy all* is
    /// `value_text(src, 0)`. An out-of-range index gives an empty string.
    pub fn value_text(&self, src: &[u8], ix: u32) -> String {
        let Some(node) = self.nodes.get(ix as usize) else {
            return String::new();
        };
        if node.kind == Kind::String {
            let inner = node.value.start.saturating_add(1)..node.value.end.saturating_sub(1);
            unescape(slice(src, &inner))
        } else {
            String::from_utf8_lossy(slice(src, &node.value)).into_owned()
        }
    }

    /// JSON path for *copy JSON path*, e.g. `data.users[0].name`. Identifier-safe keys are dotted,
    /// anything else is bracketed and quoted (`data["weird key"]`); array elements use `[i]`. The
    /// root (and any out-of-range index) is `$`, and descendants carry no `$` prefix, so a field
    /// path reads exactly as FR-RES-03 writes it.
    pub fn path(&self, src: &[u8], ix: u32) -> String {
        let mut segments: Vec<String> = Vec::new();
        let mut cur = ix;
        while let Some(node) = self.nodes.get(cur as usize) {
            if node.parent == NO_PARENT {
                break;
            }
            segments.push(match &node.key {
                Some(key) => key_segment(&unescape(slice(src, key))),
                None => format!("[{}]", self.element_index(cur)),
            });
            cur = node.parent;
        }
        if segments.is_empty() {
            return "$".to_string();
        }
        let mut out = String::new();
        for segment in segments.iter().rev() {
            if out.is_empty() {
                out.push_str(segment.strip_prefix('.').unwrap_or(segment));
            } else {
                out.push_str(segment);
            }
        }
        out
    }

    /// Position of node `ix` among its parent's children, for `[i]` path segments. Linear in the
    /// parent's span, which is fine for a click-driven copy but not for a per-row loop.
    fn element_index(&self, ix: u32) -> u32 {
        let Some(node) = self.nodes.get(ix as usize) else {
            return 0;
        };
        let parent = node.parent;
        let from = parent.saturating_add(1) as usize;
        self.nodes.get(from..ix as usize).map_or(0, |span| {
            span.iter().filter(|c| c.parent == parent).count() as u32
        })
    }

    /// Collapsed one-line preview of a container: `{3 fields}`, `[12 items]`, `{}`, `[]`. Scalars
    /// have no summary (the UI renders the value itself), so they give an empty string.
    pub fn summary(&self, ix: u32) -> String {
        let Some(node) = self.nodes.get(ix as usize) else {
            return String::new();
        };
        let n = node.child_count;
        let s = if n == 1 { "" } else { "s" };
        match node.kind {
            Kind::Object if n == 0 => "{}".to_string(),
            Kind::Object => format!("{{{n} field{s}}}"),
            Kind::Array if n == 0 => "[]".to_string(),
            Kind::Array => format!("[{n} item{s}]"),
            _ => String::new(),
        }
    }

    /// Node indices to render, top to bottom, with the subtrees of collapsed containers left out.
    /// The result feeds a virtualized uniform list: row `r` draws node `visible()[r]`.
    pub fn visible(&self, collapsed: &CollapseState) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::with_capacity(self.nodes.len().min(4096));
        let mut i = 0usize;
        while let Some(node) = self.nodes.get(i) {
            let ix = i as u32;
            out.push(ix);
            i += 1;
            if node.kind.is_container() && collapsed.is_collapsed(ix) {
                // The subtree is contiguous and strictly deeper, so skipping is a scan.
                while self.nodes.get(i).is_some_and(|c| c.depth > node.depth) {
                    i += 1;
                }
            }
        }
        out
    }
}

/// `.name` for an identifier-safe key, `["anything else"]` otherwise.
fn key_segment(key: &str) -> String {
    let safe = !key.is_empty()
        && key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if safe {
        return format!(".{key}");
    }
    let mut out = String::with_capacity(key.len() + 4);
    out.push_str("[\"");
    for c in key.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push_str("\"]");
    out
}

/// Decodes `\uXXXX` at `at` (pairing a high surrogate with a following low one). Returns the
/// character and how many input bytes it consumed.
fn unescape_u(b: &[u8], at: usize) -> (char, usize) {
    const BAD: char = char::REPLACEMENT_CHARACTER;
    let Some(hi) = hex4(b, at + 2) else {
        return (BAD, 2);
    };
    if (0xD800..0xDC00).contains(&hi) {
        if b.get(at + 6) == Some(&b'\\') && b.get(at + 7) == Some(&b'u') {
            if let Some(lo) = hex4(b, at + 8) {
                if (0xDC00..0xE000).contains(&lo) {
                    let c = 0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                    return (char::from_u32(c).unwrap_or(BAD), 12);
                }
            }
        }
        return (BAD, 6);
    }
    (char::from_u32(hi).unwrap_or(BAD), 6)
}

fn hex4(b: &[u8], at: usize) -> Option<u32> {
    let mut n = 0u32;
    for &c in b.get(at..at + 4)? {
        n = (n << 4) | (c as char).to_digit(16)?;
    }
    Some(n)
}

/// JSON string contents -> text. Lossy, so non-UTF-8 bytes from the wire cannot panic.
fn unescape(b: &[u8]) -> String {
    if !b.contains(&b'\\') {
        return String::from_utf8_lossy(b).into_owned();
    }
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let simple = match b.get(i + 1) {
            Some(b'"') => Some(b'"'),
            Some(b'\\') => Some(b'\\'),
            Some(b'/') => Some(b'/'),
            Some(b'b') => Some(0x08),
            Some(b'f') => Some(0x0C),
            Some(b'n') => Some(b'\n'),
            Some(b'r') => Some(b'\r'),
            Some(b't') => Some(b'\t'),
            _ => None,
        };
        match simple {
            Some(byte) => {
                out.push(byte);
                i += 2;
            }
            None if b.get(i + 1) == Some(&b'u') => {
                let (c, used) = unescape_u(b, i);
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                i += used;
            }
            // Not reachable for parsed input (the scanner rejects bad escapes); kept verbatim.
            None => {
                out.push(b'\\');
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Which containers are collapsed, one flag per node (so it stays ~1 byte per value and never
/// allocates per toggle). Sized to the tree it was built from; a [`Default`] one has nothing
/// collapsed and ignores toggles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollapseState {
    collapsed: Vec<bool>,
}

impl CollapseState {
    /// Containers at `depth` or deeper start collapsed. FR-RES-03's default is `2`.
    pub fn with_default_depth(tree: &JsonTree, depth: u16) -> Self {
        Self {
            collapsed: tree
                .nodes
                .iter()
                .map(|n| n.kind.is_container() && n.depth >= depth)
                .collect(),
        }
    }

    pub fn toggle(&mut self, ix: u32) {
        if let Some(flag) = self.collapsed.get_mut(ix as usize) {
            *flag = !*flag;
        }
    }

    pub fn is_collapsed(&self, ix: u32) -> bool {
        self.collapsed.get(ix as usize).copied().unwrap_or(false)
    }

    pub fn expand_all(&mut self) {
        self.collapsed.iter_mut().for_each(|flag| *flag = false);
    }

    /// Collapses every container including the root, so `visible` becomes one row.
    pub fn collapse_all(&mut self, tree: &JsonTree) {
        self.collapsed = tree.nodes.iter().map(|n| n.kind.is_container()).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    fn tree(s: &str) -> JsonTree {
        match JsonTree::parse(s.as_bytes()) {
            Ok(t) => t,
            Err(e) => panic!("{s:?} should parse: {e}"),
        }
    }

    /// (path, kind, summary-or-value) for every node, in document order.
    fn dump(src: &str, t: &JsonTree) -> Vec<(String, Kind, String)> {
        let b = src.as_bytes();
        (0..t.nodes().len() as u32)
            .map(|ix| {
                let kind = t.nodes()[ix as usize].kind;
                let text = if kind.is_container() {
                    t.summary(ix)
                } else {
                    t.value_text(b, ix)
                };
                (t.path(b, ix), kind, text)
            })
            .collect()
    }

    #[test]
    fn node_size_budget() {
        assert!(
            size_of::<Node>() <= 32,
            "Node grew to {} bytes (budget 32)",
            size_of::<Node>()
        );
        assert_eq!(size_of::<Kind>(), 1);
    }

    #[test]
    fn nesting_kinds_depths_and_parents() {
        let src = r#"{"a":[1,{"b":null}],"c":true}"#;
        let t = tree(src);
        let n = t.nodes();
        assert_eq!(n.len(), 6);
        let kinds: Vec<Kind> = n.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            vec![
                Kind::Object,
                Kind::Array,
                Kind::Number,
                Kind::Object,
                Kind::Null,
                Kind::Bool
            ]
        );
        assert_eq!(
            n.iter().map(|x| x.depth).collect::<Vec<_>>(),
            vec![0, 1, 2, 2, 3, 1]
        );
        assert_eq!(
            n.iter().map(|x| x.parent).collect::<Vec<_>>(),
            vec![NO_PARENT, 0, 1, 1, 3, 0]
        );
        assert_eq!(
            n.iter().map(|x| x.child_count).collect::<Vec<_>>(),
            vec![2, 2, 0, 1, 0, 0]
        );
        // container spans cover the brackets, scalars just the literal
        assert_eq!(slice(src.as_bytes(), &n[0].value), src.as_bytes());
        assert_eq!(slice(src.as_bytes(), &n[1].value), br#"[1,{"b":null}]"#);
        assert_eq!(slice(src.as_bytes(), &n[3].value), br#"{"b":null}"#);
        assert_eq!(slice(src.as_bytes(), &n[5].value), b"true");
        // keys are contents only, no quotes
        assert_eq!(t.key_text(src.as_bytes(), 0), None, "root has no key");
        assert_eq!(t.key_text(src.as_bytes(), 1).as_deref(), Some("a"));
        assert_eq!(t.key_text(src.as_bytes(), 2), None, "array element");
        assert_eq!(t.key_text(src.as_bytes(), 4).as_deref(), Some("b"));
    }

    #[test]
    fn whitespace_bom_and_root_scalars() {
        let minified = r#"{"a":[1,2],"b":{}}"#;
        let pretty = "\n\t {\n  \"a\" : [ 1 ,\r\n 2 ] ,\n  \"b\" : { }\n}  \n";
        assert_eq!(dump(minified, &tree(minified)), dump(pretty, &tree(pretty)));
        let with_bom = format!("\u{feff}{minified}");
        assert_eq!(tree(&with_bom).nodes().len(), 5);

        for (src, kind) in [
            ("42", Kind::Number),
            (r#""hi""#, Kind::String),
            ("true", Kind::Bool),
            ("false", Kind::Bool),
            ("null", Kind::Null),
            ("{}", Kind::Object),
            ("[]", Kind::Array),
        ] {
            let t = tree(src);
            assert_eq!(t.nodes().len(), 1, "{src}");
            assert_eq!(t.nodes()[0].kind, kind, "{src}");
            assert_eq!(t.nodes()[0].depth, 0);
            assert_eq!(t.nodes()[0].parent, NO_PARENT);
            assert_eq!(t.path(src.as_bytes(), 0), "$", "{src}");
        }
        assert_eq!(
            tree("  7  ").value_text(b"  7  ", 0),
            "7",
            "padding ignored"
        );
    }

    #[test]
    fn numbers_are_kept_verbatim() {
        let src = r#"[-0,0,1e-9,1E+10,2.5,-3.25e2,123456789012345678901234567890]"#;
        let t = tree(src);
        let texts: Vec<String> = (1..t.nodes().len() as u32)
            .map(|ix| t.value_text(src.as_bytes(), ix))
            .collect();
        assert_eq!(
            texts,
            vec![
                "-0",
                "0",
                "1e-9",
                "1E+10",
                "2.5",
                "-3.25e2",
                "123456789012345678901234567890"
            ]
        );
        assert!(t.nodes()[1..].iter().all(|n| n.kind == Kind::Number));
    }

    #[test]
    fn escapes_and_unicode_in_keys_and_values() {
        let src = r#"{"a\nb":"x\ty\"z\\\/","e":"\u00e9\u2713","emoji":"\ud83d\ude00!","lone":"\ud83d","lo":"\udc00"}"#;
        let b = src.as_bytes();
        let t = tree(src);
        assert_eq!(t.key_text(b, 1).as_deref(), Some("a\nb"));
        assert_eq!(t.value_text(b, 1), "x\ty\"z\\/");
        assert_eq!(t.value_text(b, 2), "é✓");
        assert_eq!(t.value_text(b, 3), "😀!");
        assert_eq!(t.value_text(b, 4), "\u{fffd}", "lone high surrogate");
        assert_eq!(t.value_text(b, 5), "\u{fffd}", "lone low surrogate");
        // control-character escapes
        let ctl = r#"["\b\f\r"]"#;
        assert_eq!(tree(ctl).value_text(ctl.as_bytes(), 1), "\u{8}\u{c}\r");
        // literal (already-decoded) UTF-8 passes through untouched, in keys too
        let raw = r#"{"é✓":"日本"}"#;
        let t = tree(raw);
        assert_eq!(t.key_text(raw.as_bytes(), 1).as_deref(), Some("é✓"));
        assert_eq!(t.value_text(raw.as_bytes(), 1), "日本");
    }

    #[test]
    fn non_utf8_bytes_are_lossy_not_a_panic() {
        let mut src: Vec<u8> = br#"{"k\u0041":"a"#.to_vec();
        src.extend_from_slice(&[0xFF, 0xFE]); // invalid UTF-8 inside the string
        src.extend_from_slice(br#"b"}"#);
        let t = match JsonTree::parse(&src) {
            Ok(t) => t,
            Err(e) => panic!("should parse: {e}"),
        };
        assert_eq!(t.key_text(&src, 1).as_deref(), Some("kA"));
        assert_eq!(t.value_text(&src, 1), "a\u{fffd}\u{fffd}b");
        assert_eq!(t.path(&src, 1), "kA");
    }

    #[test]
    fn paths_dotted_bracketed_and_indexed() {
        let src = r#"{"data":{"users":[{"name":"ada","tags":["x","y"]},{"name":"bob"}],"_odd-key":1,"":2,"a b":3,"q\"\\":4,"7up":5}}"#;
        let b = src.as_bytes();
        let t = tree(src);
        let paths: Vec<String> = dump(src, &t).into_iter().map(|(p, _, _)| p).collect();
        assert_eq!(
            paths,
            vec![
                "$",
                "data",
                "data.users",
                "data.users[0]",
                "data.users[0].name",
                "data.users[0].tags",
                "data.users[0].tags[0]",
                "data.users[0].tags[1]",
                "data.users[1]",
                "data.users[1].name",
                "data[\"_odd-key\"]",
                "data[\"\"]",
                "data[\"a b\"]",
                "data[\"q\\\"\\\\\"]",
                "data[\"7up\"]",
            ]
        );
        assert_eq!(t.value_text(b, 4), "ada");
        // underscore-led and digit-containing identifiers stay dotted
        let ids = r#"{"_a":{"b9":[0]}}"#;
        let t2 = tree(ids);
        assert_eq!(t2.path(ids.as_bytes(), 2), "_a.b9");
        assert_eq!(t2.path(ids.as_bytes(), 3), "_a.b9[0]");
        // a top-level array needs no root prefix either
        let arr = "[[1,2],3]";
        let t3 = tree(arr);
        let b3 = arr.as_bytes();
        assert_eq!(t3.path(b3, 0), "$");
        assert_eq!(t3.path(b3, 1), "[0]");
        assert_eq!(t3.path(b3, 3), "[0][1]");
        assert_eq!(t3.path(b3, 4), "[1]");
        assert_eq!(t3.path(b3, 99), "$", "out of range is not a panic");
    }

    #[test]
    fn summary_and_copy_value_of_containers() {
        let src = r#"{"one":[1],"many":[1,2],"empty":{},"none":[],"obj":{"a":1,"b":2,"c":3}}"#;
        let b = src.as_bytes();
        let t = tree(src);
        assert_eq!(t.summary(0), "{5 fields}");
        assert_eq!(t.summary(1), "[1 item]");
        assert_eq!(t.summary(3), "[2 items]");
        assert_eq!(t.summary(6), "{}");
        assert_eq!(t.summary(7), "[]");
        assert_eq!(t.summary(8), "{3 fields}");
        assert_eq!(t.summary(2), "", "scalars have no summary");
        assert_eq!(t.summary(999), "", "out of range is not a panic");
        // copy value on a container copies that subtree's JSON; copy all is node 0
        assert_eq!(t.value_text(b, 8), r#"{"a":1,"b":2,"c":3}"#);
        assert_eq!(t.value_text(b, 0), src);
        assert_eq!(t.value_text(b, 999), "");
    }

    #[test]
    fn duplicate_keys_are_both_kept_in_order() {
        let src = r#"{"a":1,"a":2,"a":{"b":1}}"#;
        let b = src.as_bytes();
        let t = tree(src);
        assert_eq!(t.nodes()[0].child_count, 3);
        assert_eq!(t.value_text(b, 1), "1");
        assert_eq!(t.value_text(b, 2), "2");
        assert_eq!(t.summary(3), "{1 field}");
        // all three share the same path; the node index is the identity
        assert_eq!(
            [t.path(b, 1), t.path(b, 2), t.path(b, 3)].join(","),
            "a,a,a"
        );
    }

    #[test]
    fn nested_empty_containers() {
        let src = r#"{"a":{"b":{},"c":[[],{}]},"d":[]}"#;
        let t = tree(src);
        let summaries: Vec<String> = (0..t.nodes().len() as u32)
            .map(|ix| t.summary(ix))
            .collect();
        assert_eq!(
            summaries,
            vec![
                "{2 fields}",
                "{2 fields}",
                "{}",
                "[2 items]",
                "[]",
                "{}",
                "[]"
            ]
        );
        assert_eq!(
            t.nodes().iter().map(|n| n.depth).collect::<Vec<_>>(),
            vec![0, 1, 2, 2, 3, 3, 1]
        );
        assert_eq!(t.value_text(src.as_bytes(), 3), "[[],{}]");
    }

    #[test]
    fn default_depth_two_collapses_grandchildren() {
        let src = r#"{"a":{"b":{"c":[1,2]}},"d":[{"e":1}],"f":2}"#;
        let b = src.as_bytes();
        let t = tree(src);
        let collapsed = CollapseState::with_default_depth(&t, 2);
        // depth 0 and 1 containers stay open, depth >= 2 containers are shut
        assert!(!collapsed.is_collapsed(0) && !collapsed.is_collapsed(1));
        assert!(collapsed.is_collapsed(2), "depth-2 object");
        let paths: Vec<String> = t
            .visible(&collapsed)
            .iter()
            .map(|&ix| t.path(b, ix))
            .collect();
        assert_eq!(paths, vec!["$", "a", "a.b", "d", "d[0]", "f"]);
        // scalars are never marked collapsed
        assert!(t
            .nodes()
            .iter()
            .enumerate()
            .all(|(ix, n)| n.kind.is_container() || !collapsed.is_collapsed(ix as u32)));
    }

    #[test]
    fn toggle_expand_all_and_collapse_all() {
        let src = r#"{"a":{"b":[1]},"c":2}"#;
        let b = src.as_bytes();
        let t = tree(src);
        let all = |c: &CollapseState| -> Vec<String> {
            t.visible(c).iter().map(|&ix| t.path(b, ix)).collect()
        };
        let mut c = CollapseState::with_default_depth(&t, 0);
        assert_eq!(all(&c), vec!["$"], "depth 0 collapses the root too");
        c.toggle(0);
        assert_eq!(all(&c), vec!["$", "a", "c"]);
        c.toggle(1);
        assert_eq!(all(&c), vec!["$", "a", "a.b", "c"]);
        c.toggle(2);
        assert_eq!(all(&c), vec!["$", "a", "a.b", "a.b[0]", "c"]);
        c.toggle(2);
        assert_eq!(all(&c), vec!["$", "a", "a.b", "c"]);
        c.expand_all();
        assert_eq!(all(&c), vec!["$", "a", "a.b", "a.b[0]", "c"]);
        c.collapse_all(&t);
        assert_eq!(all(&c), vec!["$"]);
        c.toggle(999);
        assert_eq!(all(&c), vec!["$"], "out-of-range toggle is ignored");
        let mut empty = CollapseState::default();
        empty.toggle(0);
        assert!(!empty.is_collapsed(0));
        assert_eq!(all(&empty), vec!["$", "a", "a.b", "a.b[0]", "c"]);
    }

    #[test]
    fn visible_skips_whole_subtrees_of_any_depth() {
        let src = r#"[{"a":[{"b":[1,2,3]}]},9]"#;
        let t = tree(src);
        let mut c = CollapseState::with_default_depth(&t, 99);
        assert_eq!(t.visible(&c).len(), t.nodes().len(), "nothing collapsed");
        c.toggle(1); // the first element, holding a 5-deep subtree
        assert_eq!(t.visible(&c), vec![0, 1, t.nodes().len() as u32 - 1]);
    }

    #[test]
    fn malformed_inputs_are_errors_not_panics() {
        let cases = [
            "",
            " ",
            "\u{feff}",
            "{",
            "[",
            "}",
            "]",
            ",",
            ":",
            "{\"a\"",
            "{\"a\":",
            "{\"a\":1",
            "{\"a\":1,",
            "{\"a\":1,}",
            "{\"a\":1 \"b\":2}",
            "{\"a\" 1}",
            "{a:1}",
            "{1:2}",
            "[1,]",
            "[1 2]",
            "[1}",
            "{\"a\":1]",
            "\"unterminated",
            "\"bad \\q escape\"",
            "\"short \\u12\"",
            "\"bad \\uZZZZ\"",
            "\"trailing escape \\",
            "01",
            "-",
            "-x",
            "1.",
            ".5",
            "1e",
            "1e+",
            "+1",
            "0x10",
            "NaN",
            "Infinity",
            "-Infinity",
            "nul",
            "tru",
            "True",
            "'single'",
            "{}{}",
            "[] []",
            "1 2",
            "null null",
            "{\"a\":1} trailing",
        ];
        for case in cases {
            assert!(
                JsonTree::parse(case.as_bytes()).is_err(),
                "{case:?} must be rejected"
            );
        }
        // raw bytes that are not even text must not panic either
        assert!(JsonTree::parse(&[0xFF, 0x00, 0x7B]).is_err());
        assert!(JsonTree::parse(&[0xEF, 0xBB, 0xBF]).is_err(), "BOM only");
    }

    #[test]
    fn error_offsets_and_messages() {
        assert_eq!(
            JsonTree::parse(b"{\"a\":1} x"),
            Err(JsonTreeError::TrailingData { offset: 8 })
        );
        assert_eq!(
            JsonTree::parse(b"[1,2"),
            Err(JsonTreeError::UnexpectedEof { offset: 4 })
        );
        assert_eq!(
            JsonTree::parse(b"[\"ab]"),
            Err(JsonTreeError::UnterminatedString { offset: 1 })
        );
        assert_eq!(
            JsonTree::parse(b"[\"\\q\"]"),
            Err(JsonTreeError::InvalidEscape { offset: 2 })
        );
        assert_eq!(
            JsonTree::parse(b"[1.]"),
            Err(JsonTreeError::InvalidNumber { offset: 1 })
        );
        // `01` tokenizes as `0` then `1`, so it fails as a stray byte, not as a bad number
        assert_eq!(
            JsonTree::parse(b"[01]"),
            Err(JsonTreeError::Unexpected {
                offset: 2,
                byte: b'1'
            })
        );
        assert_eq!(
            JsonTree::parse(b"[x]"),
            Err(JsonTreeError::Unexpected {
                offset: 1,
                byte: b'x'
            })
        );
        assert_eq!(
            JsonTree::parse(b"[x]").unwrap_err().to_string(),
            "unexpected byte 0x78 at offset 1"
        );
    }

    #[test]
    fn deep_nesting_errors_instead_of_overflowing_the_stack() {
        // 100k opening brackets: the scanner is iterative, so this returns, it does not crash.
        let bomb = "[".repeat(100_000);
        assert_eq!(
            JsonTree::parse(bomb.as_bytes()),
            Err(JsonTreeError::TooDeep {
                offset: MAX_DEPTH as u32,
                limit: MAX_DEPTH
            })
        );
        // the same bomb, balanced, is still rejected at the cap
        let balanced = format!("{bomb}{}", "]".repeat(100_000));
        assert!(matches!(
            JsonTree::parse(balanced.as_bytes()),
            Err(JsonTreeError::TooDeep { .. })
        ));
        // exactly at the cap is fine
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        let t = tree(&ok);
        assert_eq!(t.nodes().len(), MAX_DEPTH);
        assert_eq!(t.nodes()[MAX_DEPTH - 1].depth, MAX_DEPTH as u16 - 1);
        assert_eq!(
            JsonTree::parse(
                format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1)).as_bytes()
            ),
            Err(JsonTreeError::TooDeep {
                offset: MAX_DEPTH as u32,
                limit: MAX_DEPTH
            })
        );
        // deep objects too
        let deep_obj = format!("{}1{}", "{\"k\":".repeat(500), "}".repeat(500));
        assert_eq!(tree(&deep_obj).nodes().len(), 501);
    }

    #[test]
    fn large_body_index_stays_small() {
        let n = 12_000usize;
        let mut src = String::with_capacity(2_200_000);
        src.push_str(r#"{"data":{"users":["#);
        for i in 0..n {
            if i > 0 {
                src.push(',');
            }
            src.push_str(&format!(
                r#"{{"id":{i},"name":"user-{i}","email":"user-{i}@example.com","active":true,"tags":["alpha","beta","gamma"],"profile":{{"city":"Amsterdam","score":{i}.5,"rank":null}}}}"#
            ));
        }
        src.push_str("]}}");
        assert!(src.len() > 2_000_000, "generated {} bytes", src.len());
        let t = tree(&src);
        // root + data + users + 13 nodes per record
        assert_eq!(t.nodes().len(), 3 + 13 * n);
        // size_of_val of the slice is exactly nodes().len() * size_of::<Node>()
        assert!(
            std::mem::size_of_val(t.nodes()) < src.len() * 4,
            "index {} bytes for {} source bytes",
            std::mem::size_of_val(t.nodes()),
            src.len()
        );
        // spot-check the far end of the document
        let b = src.as_bytes();
        let last = t.nodes().len() as u32 - 1;
        assert_eq!(
            t.path(b, last),
            format!("data.users[{}].profile.rank", n - 1)
        );
        assert_eq!(t.value_text(b, last), "null");
        assert_eq!(t.summary(2), format!("[{n} items]"));
        // default depth 2 keeps the viewport tiny: root, data, users and nothing inside
        let collapsed = CollapseState::with_default_depth(&t, 2);
        assert_eq!(t.visible(&collapsed), vec![0, 1, 2]);
    }

    #[test]
    fn agrees_with_serde_json_on_accept_and_reject() {
        let cases = [
            r#"{"a":[1,2,{"b":"x,y"}],"c":{}}"#,
            r#"[-0,1e-9,2.5E+3,0]"#,
            r#"{"":{"":[]}}"#,
            r#"  {"a" : [ true , false , null ] }  "#,
            r#""\u00e9\ud83d\ude00\t""#,
            r#"[[[[[[[1]]]]]]]"#,
            "{",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "NaN",
            "Infinity",
            "{\"a\":1} x",
            "'x'",
            "\"\\q\"",
            "",
        ];
        for case in cases {
            let ours = JsonTree::parse(case.as_bytes()).is_ok();
            let theirs = serde_json::from_str::<serde_json::Value>(case).is_ok();
            assert_eq!(ours, theirs, "{case:?}: ours={ours} serde_json={theirs}");
        }
        // documented divergences: we tolerate a BOM and raw control bytes, serde_json does not
        for lenient in ["\u{feff}{}", "[\"raw\nnewline\"]"] {
            assert!(JsonTree::parse(lenient.as_bytes()).is_ok(), "{lenient:?}");
            assert!(serde_json::from_str::<serde_json::Value>(lenient).is_err());
        }
    }

    #[test]
    fn keys_and_values_match_serde_json_for_a_real_body() {
        let src = r#"{"data":{"users":[{"name":"ada","age":36},{"name":"bob","age":1e2}]},"ok":true,"n":null}"#;
        let b = src.as_bytes();
        let t = tree(src);
        let v: serde_json::Value = match serde_json::from_slice(b) {
            Ok(v) => v,
            Err(e) => panic!("fixture is not JSON: {e}"),
        };
        assert_eq!(t.nodes().len(), 11);
        assert_eq!(t.summary(0), "{3 fields}");
        assert_eq!(
            t.value_text(b, 4),
            v["data"]["users"][0]["name"].as_str().unwrap_or_default()
        );
        assert_eq!(t.path(b, 4), "data.users[0].name");
        assert_eq!(t.path(b, 7), "data.users[1].name");
        assert_eq!(t.value_text(b, 8), "1e2", "numbers stay as written");
        assert_eq!(t.path(b, 8), "data.users[1].age");
        assert_eq!(t.path(b, 9), "ok");
        assert_eq!(t.value_text(b, 9), "true");
        assert_eq!(t.nodes()[10].kind, Kind::Null);
        assert_eq!(t.path(b, 10), "n");
    }
}
