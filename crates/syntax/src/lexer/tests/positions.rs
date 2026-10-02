use super::super::*;

/// The line and column of a byte offset, counted from the start: columns
/// are characters.
fn naive(src: &str, offset: usize) -> Position {
    let before = &src[..offset];
    let line = before.matches('\n').count() as u32 + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Position {
        line,
        col: src[line_start..offset].chars().count() as u32 + 1,
    }
}

#[test]
fn positions_asked_in_any_order_are_where_counting_puts_them() {
    let src = "<?php\n$a = 'héllo wörld';\n\n  echo \"日本語 $a\";\n$b = [1, 2,\n 3];\nlast";
    let boundaries: Vec<usize> = (0..=src.len())
        .filter(|&i| src.is_char_boundary(i))
        .collect();
    let mut lexer = Lexer::new(src);
    // Forward, then backwards, then strided: the order tokens and the
    // strings inside them are resolved in.
    let forward = boundaries.iter().copied();
    let backward = boundaries.iter().rev().copied();
    let strided = (0..boundaries.len()).map(|i| boundaries[(i * 7) % boundaries.len()]);
    for offset in forward.chain(backward).chain(strided) {
        assert_eq!(
            lexer.line_col_at(offset),
            naive(src, offset),
            "offset {offset}"
        );
    }
}

#[test]
fn positions_in_an_ascii_source_are_where_counting_puts_them() {
    let src = "<?php\n$a = 'hello world';\n\n  echo \"abc $a\";\n$b = [1, 2,\n 3];\nlast";
    let mut lexer = Lexer::new(src);
    let all: Vec<usize> = (0..=src.len()).collect();
    let forward = all.iter().copied();
    let backward = all.iter().rev().copied();
    let strided = (0..all.len()).map(|i| all[(i * 7) % all.len()]);
    // Runs of nearby offsets, as one token's start and end are.
    let nearby = (0..all.len()).flat_map(|i| [i, (i + 3).min(src.len()), i]);
    for offset in forward.chain(backward).chain(strided).chain(nearby) {
        assert_eq!(
            lexer.line_col_at(offset),
            naive(src, offset),
            "offset {offset}"
        );
    }
}

#[test]
fn positions_in_a_fragment_are_where_counting_from_its_base_puts_them() {
    let src = "$a = 'héllo';\n  echo $b;\n$c";
    let base = Position { line: 7, col: 12 };
    let boundaries: Vec<usize> = (0..=src.len())
        .filter(|&i| src.is_char_boundary(i))
        .collect();
    let mut lexer = Lexer::new_in_php_at(src, base);
    let order = boundaries
        .iter()
        .copied()
        .chain(boundaries.iter().rev().copied())
        .chain((0..boundaries.len()).map(|i| boundaries[(i * 5) % boundaries.len()]));
    for offset in order {
        let relative = naive(src, offset);
        let expected = if relative.line == 1 {
            Position {
                line: base.line,
                col: base.col + relative.col - 1,
            }
        } else {
            Position {
                line: base.line + relative.line - 1,
                col: relative.col,
            }
        };
        assert_eq!(lexer.line_col_at(offset), expected, "offset {offset}");
    }
}

#[test]
fn columns_between_any_two_offsets_are_the_characters_a_lossy_decoding_reads() {
    let sources: [&[u8]; 4] = [
        "caf\u{e9} \u{10348} \u{4e2d} x".as_bytes(),
        b"ascii only",
        b"bad \xff\xfe byte \xe2\x82 cut \xc3\xa9",
        "tail \u{e9}".as_bytes(),
    ];
    for src in sources {
        let lexer = Lexer::from_bytes(src);
        for from in 0..=src.len() {
            for to in from..=src.len() {
                assert_eq!(
                    lexer.columns(from, to),
                    super::super::char_count(&src[from..to]),
                    "{src:?} {from}..{to}"
                );
            }
        }
    }
}
