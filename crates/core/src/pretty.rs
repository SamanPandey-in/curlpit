//! Streaming JSON pretty-printer (FR-RES-04). Works on bytes with no DOM, so a 10 MB body
//! costs ~1.5x its size, not 10x. Best-effort: does not validate; invalid JSON stays readable.

fn newline(out: &mut Vec<u8>, depth: usize, indent: usize) {
    out.push(b'\n');
    out.extend(std::iter::repeat(b' ').take(depth * indent));
}

pub fn pretty_json(input: &[u8], indent: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + input.len() / 2);
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if in_str {
            out.push(b);
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
        } else {
            match b {
                b'"' => {
                    in_str = true;
                    out.push(b);
                }
                b'{' | b'[' => {
                    let close = if b == b'{' { b'}' } else { b']' };
                    let mut j = i + 1;
                    while j < input.len() && input[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < input.len() && input[j] == close {
                        out.push(b);
                        out.push(close);
                        i = j;
                    } else {
                        out.push(b);
                        depth += 1;
                        newline(&mut out, depth, indent);
                    }
                }
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                    newline(&mut out, depth, indent);
                    out.push(b);
                }
                b',' => {
                    out.push(b);
                    newline(&mut out, depth, indent);
                }
                b':' => out.extend_from_slice(b": "),
                b' ' | b'\n' | b'\r' | b'\t' => {}
                _ => out.push(b),
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> String {
        String::from_utf8(pretty_json(s.as_bytes(), 2)).unwrap()
    }

    #[test]
    fn nested_and_empty() {
        let expected = "{\n  \"a\": [\n    1,\n    2,\n    {\n      \"b\": \"x,y\"\n    }\n  ],\n  \"c\": {}\n}";
        assert_eq!(p(r#"{"a":[1,2,{"b":"x,y"}],"c":{}}"#), expected);
    }

    #[test]
    fn strings_with_escapes_and_unicode_untouched() {
        assert_eq!(
            p(r#"{"a":"q\"x,{y}","é":"✓"}"#),
            "{\n  \"a\": \"q\\\"x,{y}\",\n  \"é\": \"✓\"\n}"
        );
    }

    #[test]
    fn already_pretty_is_stable() {
        let once = p(r#"{"a":[1,2],"b":null}"#);
        assert_eq!(p(&once), once);
    }

    #[test]
    fn garbage_does_not_panic() {
        let _ = p("}}}]]]{{{[[[\"unterminated");
        let _ = p("");
    }
}
