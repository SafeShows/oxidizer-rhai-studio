//! Rhai syntax colors: splits a line into colored spans. Block comments and backtick strings can span lines, so
//! the state at the end of one line goes into the next.

#[derive(Clone, Copy, Default, PartialEq)]
pub enum State {
    #[default]
    Code,
    BlockComment,
    Template,
}

pub const PLAIN: [f32; 4] = [0.88, 0.90, 0.93, 1.0];
pub const KEYWORD: [f32; 4] = [1.0, 0.70, 0.32, 1.0];
pub const STRING: [f32; 4] = [0.58, 0.86, 0.45, 1.0];
pub const NUMBER: [f32; 4] = [0.50, 0.78, 1.0, 1.0];
pub const COMMENT: [f32; 4] = [0.47, 0.53, 0.58, 1.0];
pub const CALL: [f32; 4] = [0.98, 0.88, 0.50, 1.0];
pub const MODULE: [f32; 4] = [0.40, 0.86, 0.78, 1.0];
pub const PUNCT: [f32; 4] = [0.68, 0.72, 0.78, 1.0];

const KEYWORDS: &[&str] = &[
    "let", "const", "fn", "if", "else", "switch", "while", "loop", "for", "in", "do", "until", "break", "continue",
    "return", "throw", "try", "catch", "import", "export", "as", "private", "this", "true", "false", "global", "is",
];

/// Bytes of a line and their color.
pub type Span = (std::ops::Range<usize>, [f32; 4]);

/// Spans covering the whole line, and the state the next line starts in.
pub fn spans(line: &str, mut state: State) -> (Vec<Span>, State) {
    let b = line.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let color = match state {
            State::BlockComment => {
                i = line[i..].find("*/").map_or(b.len(), |p| i + p + 2);
                if line[start..i].ends_with("*/") {
                    state = State::Code;
                }
                COMMENT
            }
            State::Template => {
                i = line[i..].find('`').map_or(b.len(), |p| i + p + 1);
                if i <= b.len() && line[start..i].ends_with('`') {
                    state = State::Code;
                }
                STRING
            }
            State::Code => match b[i] {
                b'/' if b.get(i + 1) == Some(&b'/') => {
                    i = b.len();
                    COMMENT
                }
                b'/' if b.get(i + 1) == Some(&b'*') => {
                    i += 2;
                    state = State::BlockComment;
                    COMMENT
                }
                b'`' => {
                    i += 1;
                    state = State::Template;
                    STRING
                }
                q @ (b'"' | b'\'') => {
                    i += 1;
                    while i < b.len() && b[i] != q {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                    i = (i + 1).min(b.len());
                    STRING
                }
                c if c.is_ascii_digit() => {
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                        // "1..5" is a range, not a number with two dots
                        if b[i] == b'.' && b.get(i + 1) == Some(&b'.') {
                            break;
                        }
                        i += 1;
                    }
                    NUMBER
                }
                c if c.is_ascii_alphabetic() || c == b'_' => {
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    let word = &line[start..i];
                    let rest = line[i..].trim_start();
                    if KEYWORDS.contains(&word) {
                        KEYWORD
                    } else if rest.starts_with("::") {
                        MODULE
                    } else if rest.starts_with('(') {
                        CALL
                    } else {
                        PLAIN
                    }
                }
                c if c.is_ascii_whitespace() => {
                    while i < b.len() && b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    PLAIN
                }
                _ => {
                    // one character (a whole UTF-8 sequence)
                    i += line[i..].chars().next().map_or(1, char::len_utf8);
                    PUNCT
                }
            },
        };
        out.push((start..i, color));
    }
    (out, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(line: &str) -> Vec<(&str, [f32; 4])> {
        spans(line, State::Code).0.into_iter().filter(|(r, _)| !line[r.clone()].trim().is_empty()).map(|(r, c)| (&line[r], c)).collect()
    }

    #[test]
    fn tokens() {
        assert_eq!(
            colors(r#"let v = ksa::Program::controlled_vehicle(); // hi"#),
            vec![
                ("let", KEYWORD), ("v", PLAIN), ("=", PUNCT), ("ksa", MODULE), (":", PUNCT), (":", PUNCT), ("Program", MODULE),
                (":", PUNCT), (":", PUNCT), ("controlled_vehicle", CALL), ("(", PUNCT), (")", PUNCT), (";", PUNCT), ("// hi", COMMENT),
            ]
        );
        assert_eq!(colors(r#"print("a \" b"); 1..5"#)[2], (r#""a \" b""#, STRING));
        assert_eq!(colors("x = 1..5")[2], ("1", NUMBER));
    }

    #[test]
    fn state_carries_over() {
        let (_, s) = spans("a /* start", State::Code);
        assert!(s == State::BlockComment);
        let (sp, s) = spans("still */ b", s);
        assert!(s == State::Code && sp[0].1 == COMMENT);
        let (_, s) = spans("let t = `multi", State::Code);
        assert!(s == State::Template);
        let (sp, s) = spans("line` + 1", s);
        assert!(s == State::Code && sp[0].1 == STRING);
    }
}
