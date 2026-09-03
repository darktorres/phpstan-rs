//! Token line/column position and span tracking.
use super::super::*;

// `tokenize_with_spans` - Phase 11's source-position work.

#[test]
fn tokens_on_a_later_line_get_real_line_and_column_positions() {
    let src = "<?php\n$x = 1;\n$y = 2;\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    // toks: Variable(x), Equals, Integer(1), Semicolon, Variable(y), ...
    let (tok, span) = &toks[4];
    assert_eq!(*tok, Token::Variable("y".to_string()));
    assert_eq!(span.start, Position { line: 3, col: 1 });
    assert_eq!(span.end, Position { line: 3, col: 3 });
}

#[test]
fn a_token_spanning_multiple_lines_does_not_throw_off_the_next_token_s_position() {
    let src = "<?php\n$x = <<<EOT\nhello\nworld\nEOT;\n$y = 1;\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let heredoc_end = toks
        .iter()
        .find(|(t, _)| matches!(t, Token::InterpString(_)))
        .unwrap()
        .1
        .end;
    // The heredoc body itself spans lines 3-4, but the token's own
    // extent runs through its closing marker on line 5, not just the
    // last content line.
    assert_eq!(heredoc_end.line, 5);
    let (next_tok, next_span) = toks
        .iter()
        .find(|(t, _)| *t == Token::Variable("y".to_string()))
        .unwrap();
    assert_eq!(*next_tok, Token::Variable("y".to_string()));
    assert_eq!(next_span.start, Position { line: 6, col: 1 });
}

#[test]
fn a_multi_byte_character_earlier_on_a_line_does_not_shift_later_columns() {
    // "é" is 2 bytes in UTF-8 but 1 character - a byte-offset column
    // would report column 8 for $x below; the real (character) column
    // is 7.
    let src = "<?php\n'é'; $x;\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let (_, span) = toks
        .iter()
        .find(|(t, _)| *t == Token::Variable("x".to_string()))
        .unwrap();
    assert_eq!(span.start, Position { line: 2, col: 6 });
}

#[test]
fn complex_interpolation_gets_a_real_base_position_on_a_later_line() {
    let src = "<?php\n$s = \"a\\n{$obj->prop}\";\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let (tok, _) = toks
        .iter()
        .find(|(t, _)| matches!(t, Token::InterpString(_)))
        .unwrap();
    let Token::InterpString(parts) = tok else {
        unreachable!()
    };
    let (_, base) = parts
        .iter()
        .find_map(|p| match p {
            StringPart::ComplexExpr(src, base) => Some((src.clone(), *base)),
            _ => None,
        })
        .expect("expected a ComplexExpr part");
    // The literal "a\n" inside the string doesn't introduce a real
    // newline in the source - {$obj->prop} is still on line 2.
    // `$s = "a\n{$obj->prop}";` - the `$` of `$obj` is the 11th
    // character (right after `{`, the 10th).
    assert_eq!(base, Position { line: 2, col: 11 });
}

#[test]
fn shorthand_property_interpolation_base_points_at_the_dollar_sign() {
    let src = "<?php\n$s = \"x $obj->prop y\";\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let (tok, _) = toks
        .iter()
        .find(|(t, _)| matches!(t, Token::InterpString(_)))
        .unwrap();
    let Token::InterpString(parts) = tok else {
        unreachable!()
    };
    let (src_text, base) = parts
        .iter()
        .find_map(|p| match p {
            StringPart::ComplexExpr(src, base) => Some((src.clone(), *base)),
            _ => None,
        })
        .expect("expected a ComplexExpr part");
    assert_eq!(src_text, "$obj->prop");
    // `$s = "x $obj->prop y";` - the `$` of `$obj` is the 9th
    // character.
    assert_eq!(base, Position { line: 2, col: 9 });
}

#[test]
fn many_tokens_on_one_extremely_long_line_does_not_blow_up_quadratically() {
    // Real bug, not hypothetical: `line_col_at`'s first implementation
    // recounted every character from the line's own start on *every*
    // call, which is fine for ordinary code but a genuine O(line
    // length x tokens on it) blowup on a real file in this project's
    // own test corpus - `amphp/hpack`'s generated Huffman lookup
    // table has a single line 1.25 million characters long, and hung
    // for minutes (previously well under a second) before the
    // incremental-cache fix. This is a smaller version of the same
    // shape: one 200,000-character line with thousands of tokens on
    // it - a quadratic implementation is easily slow enough here to
    // fail this test's time budget; a linear one finishes instantly.
    let mut src = "<?php $a = [".to_string();
    for i in 0..40_000 {
        if i > 0 {
            src.push(',');
        }
        src.push_str(&i.to_string());
    }
    src.push_str("];\n");
    assert!(src.len() > 200_000);

    let start = std::time::Instant::now();
    let toks = Lexer::new(&src).tokenize_with_spans();
    let elapsed = start.elapsed();

    assert!(toks.len() > 40_000);
    assert!(
        elapsed.as_secs() < 2,
        "tokenizing one long line took {elapsed:?} - looks quadratic again"
    );
}
