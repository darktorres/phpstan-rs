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
