//! Token line/column position and span tracking.
use super::super::*;
use crate::token::StringPart;

// `tokenize_with_spans` - source-position tracking.

#[test]
fn tokens_on_a_later_line_get_real_line_and_column_positions() {
    let src = "<?php\n$x = 1;\n$y = 2;\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    // toks: Variable(x), Equals, Integer(1), Semicolon, Variable(y), ...
    let (tok, span, _) = &toks[4];
    assert_eq!(*tok, Token::Variable("y".to_string().into()));
    assert_eq!(span.start, Position { line: 3, col: 1 });
    assert_eq!(span.end, Position { line: 3, col: 3 });
}

#[test]
fn a_token_spanning_multiple_lines_does_not_throw_off_the_next_token_s_position() {
    let src = "<?php\n$x = <<<EOT\nhello\nworld\nEOT;\n$y = 1;\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let heredoc_end = toks
        .iter()
        .find(|(t, _, _)| matches!(t, Token::InterpString(_)))
        .unwrap()
        .1
        .end;
    // The heredoc body itself spans lines 3-4, but the token's own
    // extent runs through its closing marker on line 5, not just the
    // last content line.
    assert_eq!(heredoc_end.line, 5);
    let (next_tok, next_span, _) = toks
        .iter()
        .find(|(t, _, _)| *t == Token::Variable("y".to_string().into()))
        .unwrap();
    assert_eq!(*next_tok, Token::Variable("y".to_string().into()));
    assert_eq!(next_span.start, Position { line: 6, col: 1 });
}

#[test]
fn a_multi_byte_character_earlier_on_a_line_does_not_shift_later_columns() {
    // "é" is 2 bytes in UTF-8 but 1 character - a byte-offset column
    // would report column 8 for $x below; the real (character) column
    // is 7.
    let src = "<?php\n'é'; $x;\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let (_, span, _) = toks
        .iter()
        .find(|(t, _, _)| *t == Token::Variable("x".to_string().into()))
        .unwrap();
    assert_eq!(span.start, Position { line: 2, col: 6 });
}

#[test]
fn complex_interpolation_gets_a_real_base_position_on_a_later_line() {
    let src = "<?php\n$s = \"a\\n{$obj->prop}\";\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let (tok, _, _) = toks
        .iter()
        .find(|(t, _, _)| matches!(t, Token::InterpString(_)))
        .unwrap();
    let Token::InterpString(parts) = tok else {
        unreachable!()
    };
    let part = parts
        .iter()
        .find_map(|p| match p {
            StringPart::PropertyAccess {
                var,
                var_start,
                var_end,
                prop,
                end,
                ..
            } => Some((var.clone(), *var_start, *var_end, prop.clone(), *end)),
            _ => None,
        })
        .expect("expected a PropertyAccess part");
    let (var, var_start, var_end, prop, end) = part;
    assert_eq!(var, "obj");
    assert_eq!(prop, "prop");
    // The literal "a\n" inside the string doesn't introduce a real
    // newline in the source - {$obj->prop} is still on line 2.
    // `$s = "a\n{$obj->prop}";` - the `$` of `$obj` is the 11th
    // character (right after `{`, the 10th), `->` starts at the 15th
    // (right after `obj`), and the closing `}` sits at the 21st.
    assert_eq!(var_start, Position { line: 2, col: 11 });
    assert_eq!(var_end, Position { line: 2, col: 15 });
    assert_eq!(end, Position { line: 2, col: 21 });
}

#[test]
fn shorthand_property_interpolation_base_points_at_the_dollar_sign() {
    let src = "<?php\n$s = \"x $obj->prop y\";\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    let (tok, _, _) = toks
        .iter()
        .find(|(t, _, _)| matches!(t, Token::InterpString(_)))
        .unwrap();
    let Token::InterpString(parts) = tok else {
        unreachable!()
    };
    let part = parts
        .iter()
        .find_map(|p| match p {
            StringPart::PropertyAccess {
                var,
                var_start,
                var_end,
                prop,
                end,
                ..
            } => Some((var.clone(), *var_start, *var_end, prop.clone(), *end)),
            _ => None,
        })
        .expect("expected a PropertyAccess part");
    let (var, var_start, var_end, prop, end) = part;
    assert_eq!(var, "obj");
    assert_eq!(prop, "prop");
    // `$s = "x $obj->prop y";` - the `$` of `$obj` is the 9th character,
    // `->` starts at the 13th (right after `obj`), and `prop` ends right
    // before the trailing space at the 19th.
    assert_eq!(var_start, Position { line: 2, col: 9 });
    assert_eq!(var_end, Position { line: 2, col: 13 });
    assert_eq!(end, Position { line: 2, col: 19 });
}

#[test]
fn many_tokens_on_one_extremely_long_line_does_not_blow_up_quadratically() {
    // `line_col_at` must not recount every character from the line's own
    // start on every call - fine for ordinary code, but a genuine O(line
    // length x tokens on it) blowup on a single very long line with many
    // tokens. One 200,000-character line with thousands of tokens on it
    // is enough to fail this test's time budget under a quadratic
    // implementation; a linear one finishes instantly.
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

#[test]
fn a_short_echo_tag_after_inline_html_still_echoes() {
    let src = "<title><?= $t ?></title><?= $u ?>";
    let toks: Vec<Token> = Lexer::new(src)
        .tokenize_with_spans()
        .into_iter()
        .map(|(t, _, _)| t)
        .collect();
    let echoes = toks
        .iter()
        .filter(|t| matches!(t, Token::Keyword(Keyword::Echo)))
        .count();
    assert_eq!(echoes, 2, "{toks:?}");
    assert!(
        matches!(&toks[0], Token::InlineHtml(h) if &**h == "<title>"),
        "{toks:?}"
    );
}

#[test]
fn an_unexpected_byte_is_a_warning_at_its_position_not_output() {
    let (tokens, _, warnings) = Lexer::new("<?php\n$a = 1;\x01 $b;").tokenize_with_comments();
    assert_eq!(
        warnings,
        vec![LexWarning {
            span: Span {
                start: Position { line: 2, col: 8 },
                end: Position { line: 2, col: 9 },
            },
            message: "unexpected byte 0x01 in PHP source, skipping".to_string(),
            is_error: false,
        }]
    );
    assert!(
        tokens
            .iter()
            .any(|(t, _, _)| matches!(t, Token::Variable(v) if &**v == "b"))
    );
}
