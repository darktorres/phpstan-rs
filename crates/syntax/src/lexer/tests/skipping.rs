use super::super::*;

/// What skipping blanks and comments finds, byte by byte: where it stops
/// and the byte ranges of the comments it passed.
fn byte_by_byte(src: &[u8], mut pos: usize) -> (usize, Vec<(usize, usize)>) {
    let starts = |pos: usize, s: &[u8]| src.get(pos..pos + s.len()).is_some_and(|h| h == s);
    let peek = |pos: usize| *src.get(pos).unwrap_or(&0);
    let peek_at = |pos: usize, n: usize| *src.get(pos + n).unwrap_or(&0);
    let mut comments = Vec::new();
    loop {
        match peek(pos) {
            b' ' | b'\t' | b'\r' | b'\n' => pos += 1,
            b'#' => {
                if starts(pos, b"#[") {
                    break;
                }
                let start = pos;
                while pos < src.len() && peek(pos) != b'\n' && !starts(pos, b"?>") {
                    pos += 1;
                }
                comments.push((start, pos));
            }
            b'/' if peek_at(pos, 1) == b'/' => {
                let start = pos;
                while pos < src.len() && peek(pos) != b'\n' && !starts(pos, b"?>") {
                    pos += 1;
                }
                comments.push((start, pos));
            }
            b'/' if starts(pos, b"/**") && peek_at(pos, 3) != b'/' => break,
            b'/' if peek_at(pos, 1) == b'*' => {
                let start = pos;
                pos += 2;
                while pos < src.len() && !starts(pos, b"*/") {
                    pos += 1;
                }
                pos = (pos + 2).min(src.len());
                comments.push((start, pos));
            }
            _ => break,
        }
    }
    (pos, comments)
}

#[test]
fn skipping_blanks_and_comments_stops_where_going_byte_by_byte_does() {
    const ALPHABET: &[u8] = b"#/*?> \n\t\r[a";
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |n: usize| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as usize) % n
    };
    for _ in 0..20_000 {
        let len = next(14);
        let src: Vec<u8> = (0..len).map(|_| ALPHABET[next(ALPHABET.len())]).collect();
        for start in 0..=src.len() {
            let mut lexer = Lexer::from_bytes(&src);
            lexer.in_php = true;
            lexer.pos = start;
            lexer.skip_whitespace_and_comments();
            let (pos, comments) = byte_by_byte(&src, start);
            assert_eq!(
                (lexer.pos, lexer.comments.clone()),
                (pos, comments),
                "{:?} from {start}",
                String::from_utf8_lossy(&src)
            );
        }
    }
}

#[test]
fn identifier_bytes_are_those_the_character_classes_name() {
    for b in 0..=255u8 {
        let start = b == b'_' || b.is_ascii_alphabetic() || b >= 0x80;
        assert_eq!(is_ident_start(b), start, "start {b}");
        assert_eq!(
            is_ident_continue(b),
            start || b.is_ascii_digit(),
            "continue {b}"
        );
    }
    for src in [
        &b"abc_1 x"[..],
        b"\xc3\xa9t\xc3\xa9 ",
        b"123",
        b"",
        b"a",
        b"_",
    ] {
        for from in 0..=src.len() {
            let mut lexer = Lexer::from_bytes(src);
            lexer.pos = from;
            let span = lexer.lex_ident_span();
            let mut end = from;
            while end < src.len() && is_ident_continue(src[end]) {
                end += 1;
            }
            assert_eq!(span, from..end, "{src:?} from {from}");
        }
    }
}

#[test]
fn a_name_the_lexer_repeats_is_the_one_the_interner_holds() {
    let interner = Arc::clone(Interner::global());
    let tokens = Lexer::new_in_php("<?php $a = foo($a, foo);")
        .with_interner(&interner)
        .tokenize();
    let names: Vec<&Name> = tokens
        .iter()
        .filter_map(|t| match t {
            Token::Variable(n) | Token::Identifier(n) => Some(n),
            _ => None,
        })
        .collect();
    let first = |text: &str| names.iter().find(|n| n.as_str() == text).copied().unwrap();
    for text in ["a", "foo"] {
        let held = interner.name(text);
        for n in names.iter().filter(|n| n.as_str() == text) {
            assert!(n.same_allocation(&held), "{text}");
        }
        assert!(first(text).same_allocation(&held));
    }
    // Another lexer, while these are alive, finds the same names.
    let again = Lexer::new_in_php("$a").with_interner(&interner).tokenize();
    let Token::Variable(n) = &again[0] else {
        panic!("a variable")
    };
    assert!(n.same_allocation(first("a")));
}

#[test]
fn a_doc_comment_ends_where_going_byte_by_byte_ends_it() {
    const ALPHABET: &[u8] = b"/*a\n @";
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move |n: usize| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as usize) % n
    };
    for _ in 0..20_000 {
        let mut src = b"/**".to_vec();
        src.extend((0..next(10)).map(|_| ALPHABET[next(ALPHABET.len())]));
        let mut pos = 3;
        while pos < src.len() && !src[pos..].starts_with(b"*/") {
            pos += 1;
        }
        let expected = (pos + 2).min(src.len());
        let mut lexer = Lexer::from_bytes(&src);
        lexer.in_php = true;
        lexer.lex_doc_comment();
        assert_eq!(lexer.pos, expected, "{:?}", String::from_utf8_lossy(&src));
    }
}
