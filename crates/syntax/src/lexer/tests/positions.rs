use super::super::*;
use crate::span::LineCol;

/// The line and column of a byte offset, counted from the start: columns
/// are characters.
fn naive(src: &str, offset: usize) -> LineCol {
    let before = &src[..offset];
    let line = before.matches('\n').count() as u32 + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    LineCol {
        line,
        col: src[line_start..offset].chars().count() as u32 + 1,
    }
}

#[test]
fn the_line_index_puts_every_offset_where_counting_does() {
    let sources = [
        "<?php\n$a = 'héllo wörld';\n\n  echo \"日本語 $a\";\n$b = [1, 2,\n 3];\nlast",
        "<?php\n$a = 'hello world';\n\n  echo \"abc $a\";\n$b = [1, 2,\n 3];\nlast",
        "\u{10348}\n\u{e9}\u{4e2d}x\n",
    ];
    for src in sources {
        let index = Lexer::new(src).run_lexed().line_index;
        for offset in (0..=src.len()).filter(|&i| src.is_char_boundary(i)) {
            assert_eq!(
                index.line_col(offset as u32),
                naive(src, offset),
                "{src:?} offset {offset}"
            );
        }
    }
}

#[test]
fn columns_on_a_line_are_the_characters_a_lossy_decoding_reads() {
    let sources: [&[u8]; 4] = [
        "caf\u{e9} \u{10348} \u{4e2d} x".as_bytes(),
        b"ascii only",
        b"bad \xff\xfe byte \xe2\x82 cut \xc3\xa9",
        "tail \u{e9}".as_bytes(),
    ];
    for src in sources {
        let index = crate::span::LineIndex::new(src);
        let boundaries = (0..=src.len()).filter(|&i| {
            let lossy = String::from_utf8_lossy(&src[..i]);
            i == src.len()
                || std::str::from_utf8(&src[i..])
                    .is_ok_and(|_| lossy.len() == lossy.chars().map(char::len_utf8).sum::<usize>())
        });
        for offset in boundaries {
            let before = lossy(&src[..offset]).chars().count() as u32;
            // A cut inside an invalid run reads as one character either way.
            if std::str::from_utf8(&src[offset..]).is_ok() {
                assert_eq!(
                    index.line_col(offset as u32).col,
                    before + 1,
                    "{src:?} {offset}"
                );
            }
        }
    }
}

#[test]
fn the_spans_of_a_fragment_are_its_base_plus_where_each_token_is() {
    let src = "$a = 'héllo';\n  echo $b;\n$c";
    let whole = Lexer::new_in_php(src).tokenize_with_spans();
    let shifted = Lexer::fragment(src, 1000).tokenize_with_spans();
    assert_eq!(whole.len(), shifted.len());
    for ((token, span, _), (shifted_token, shifted_span, _)) in whole.iter().zip(&shifted) {
        assert_eq!(token, shifted_token);
        assert_eq!(shifted_span.start, span.start + 1000);
        assert_eq!(shifted_span.end, span.end + 1000);
    }
}
