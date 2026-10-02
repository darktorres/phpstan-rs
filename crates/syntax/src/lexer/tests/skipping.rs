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
