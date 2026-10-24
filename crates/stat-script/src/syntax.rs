//! Token classes of Rhai source, for syntax highlighting in an editor.
//!
//! A light lexer, not a parser: it never fails, so it can colour a script
//! while it is being typed (an unterminated string or comment runs to the end
//! of the text). Ranges are byte ranges into the source, always on character
//! boundaries, and together they cover the whole source in order.

use std::ops::Range;

/// What a stretch of source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A reserved word (`let`, `if`, `fn`, …) or `true` / `false`.
    Keyword,
    /// A string, character or template literal.
    String,
    /// A numeric literal.
    Number,
    /// A line or block comment (including doc comments).
    Comment,
    /// A name followed by `(`: a function call or definition.
    Function,
    /// Anything else: names, operators, punctuation, white space.
    Plain,
}

const KEYWORDS: &[&str] = &[
    "let", "const", "if", "else", "switch", "do", "while", "until", "loop", "for", "in", "break", "continue", "return",
    "throw", "try", "catch", "fn", "private", "import", "export", "as", "global", "true", "false", "this", "Fn", "is",
    "not",
];

/// Split `source` into classified, contiguous byte ranges.
#[must_use]
pub fn tokens(source: &str) -> Vec<(Range<usize>, TokenKind)> {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut out: Vec<(Range<usize>, TokenKind)> = Vec::new();
    let mut push = |range: Range<usize>, kind: TokenKind| match out.last_mut() {
        // Merge neighbouring plain text so callers get few, long runs.
        Some((last, TokenKind::Plain)) if kind == TokenKind::Plain && last.end == range.start => last.end = range.end,
        _ => out.push((range, kind)),
    };
    let mut i = 0;
    while i < n {
        let b = bytes[i];
        let start = i;
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = source[i..].find('\n').map_or(n, |k| i + k);
            push(start..i, TokenKind::Comment);
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            // Rhai block comments nest.
            let mut depth = 0usize;
            while i < n {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            push(start..i.min(n), TokenKind::Comment);
        } else if b == b'"' || b == b'\'' || b == b'`' {
            i += 1;
            while i < n && bytes[i] != b {
                // Backslash escapes in "…" and '…'; template strings have none.
                i += if bytes[i] == b'\\' && b != b'`' { 2 } else { 1 };
            }
            i = (i + 1).min(n);
            push(start..i, TokenKind::String);
        } else if b.is_ascii_digit() {
            i += 1;
            while i < n {
                let c = bytes[i];
                let exponent_sign = (c == b'+' || c == b'-') && matches!(bytes[i - 1], b'e' | b'E');
                let fraction = c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
                if c.is_ascii_alphanumeric() || c == b'_' || fraction || (exponent_sign && !is_hex(&bytes[start..i])) {
                    i += 1;
                } else {
                    break;
                }
            }
            push(start..i, TokenKind::Number);
        } else if b.is_ascii_alphabetic() || b == b'_' {
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &source[start..i];
            let kind = if KEYWORDS.contains(&word) {
                TokenKind::Keyword
            } else if source[i..].trim_start_matches([' ', '\t']).starts_with('(') {
                TokenKind::Function
            } else {
                TokenKind::Plain
            };
            push(start..i, kind);
        } else {
            // One character (which may be several bytes).
            i += source[i..].chars().next().map_or(1, char::len_utf8);
            push(start..i, TokenKind::Plain);
        }
    }
    out
}

fn is_hex(prefix: &[u8]) -> bool {
    prefix.len() >= 2 && prefix[0] == b'0' && matches!(prefix[1], b'x' | b'X')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(source: &str) -> Vec<(&str, TokenKind)> {
        tokens(source).into_iter().filter(|(_, k)| *k != TokenKind::Plain).map(|(r, k)| (&source[r], k)).collect()
    }

    #[test]
    fn classifies_the_parts_of_a_script() {
        use TokenKind::*;
        let src = "let t = open_sample(\"Norris\"); // data\nfor r in t.rows() { x += 1.5e-3 + 0xFF; }";
        assert_eq!(
            classes(src),
            [
                ("let", Keyword),
                ("open_sample", Function),
                ("\"Norris\"", String),
                ("// data", Comment),
                ("for", Keyword),
                ("in", Keyword),
                ("rows", Function),
                ("1.5e-3", Number),
                ("0xFF", Number),
            ]
        );
    }

    #[test]
    fn covers_the_source_and_tolerates_unfinished_text() {
        for src in ["\"open", "/* a /* nested */ still", "x = 'a' + \"\\\"\"", "r.Größe → 1..10", "`t ${x}`", ""] {
            let toks = tokens(src);
            let mut at = 0;
            for (r, _) in &toks {
                assert_eq!(r.start, at, "{src:?}");
                assert!(src.is_char_boundary(r.end));
                at = r.end;
            }
            assert_eq!(at, src.len(), "{src:?}");
        }
        assert_eq!(classes("/* a /* b */ c */ d"), [("/* a /* b */ c */", TokenKind::Comment)]);
        assert_eq!(classes("\"a\\\"b\" c"), [("\"a\\\"b\"", TokenKind::String)]);
        assert_eq!(classes("1..10"), [("1", TokenKind::Number), ("10", TokenKind::Number)]);
    }
}
