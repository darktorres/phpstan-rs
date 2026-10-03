use super::super::*;

/// What lexing a single-quoted string gave before the closing quote was
/// searched for: the bytes after unescaping, and where the lexer stood.
fn byte_by_byte(src: &[u8]) -> (Vec<u8>, usize) {
    let peek = |pos: usize| *src.get(pos).unwrap_or(&0);
    let mut pos = 1;
    let mut s = Vec::new();
    let mut run_start = pos;
    while pos < src.len() && peek(pos) != b'\'' {
        if peek(pos) == b'\\' && (peek(pos + 1) == b'\'' || peek(pos + 1) == b'\\') {
            s.extend_from_slice(&src[run_start..pos]);
            s.push(peek(pos + 1));
            pos += 2;
            run_start = pos;
        } else {
            pos += 1;
        }
    }
    s.extend_from_slice(&src[run_start..pos]);
    (s, pos + 1)
}

#[test]
fn a_single_quoted_string_is_what_the_byte_by_byte_scan_made_of_it() {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let alphabet = b"ab'\\'\\ \n\xc3\xa9\xff";
    for _ in 0..100_000 {
        let len = next() as usize % 12;
        let mut src = vec![b'\''];
        src.extend((0..len).map(|_| alphabet[next() as usize % alphabet.len()]));
        let mut lexer = Lexer::from_bytes(&src);
        let Token::StringLit(string) = lexer.lex_single_quoted() else {
            panic!("not a string literal: {src:?}");
        };
        let (bytes, end) = byte_by_byte(&src);
        assert_eq!(string.as_bytes(), bytes, "{src:?}");
        assert_eq!(lexer.pos, end, "{src:?}");
    }
}

#[test]
fn the_usual_single_quoted_strings() {
    for (source, value) in [
        (r"'abc'", &b"abc"[..]),
        (r"''", b""),
        (r"'it\'s'", b"it's"),
        (r"'a\\b'", b"a\\b"),
        (r"'a\nb'", b"a\\nb"),
        (r"'\\'", b"\\"),
    ] {
        let mut lexer = Lexer::from_bytes(source.as_bytes());
        let Token::StringLit(string) = lexer.lex_single_quoted() else {
            panic!("{source}");
        };
        assert_eq!(string.as_bytes(), value, "{source}");
    }
}
