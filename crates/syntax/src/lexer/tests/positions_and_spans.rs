//! Token line/column position and span tracking.
use super::super::*;
use crate::span::LineCol;
use crate::token::StringPart;

/// The line and column of the byte offset `offset` of `src`.
fn position(src: &str, offset: u32) -> LineCol {
    crate::span::LineIndex::new(src.as_bytes()).line_col(offset)
}

// `tokenize_with_spans` - source-position tracking.

#[test]
fn tokens_on_a_later_line_get_real_line_and_column_positions() {
    let src = "<?php\n$x = 1;\n$y = 2;\n";
    let toks = Lexer::new(src).tokenize_with_spans();
    // toks: Variable(x), Equals, Integer(1), Semicolon, Variable(y), ...
    let (tok, span, _) = &toks[4];
    assert_eq!(*tok, Token::Variable("y".to_string().into()));
    assert_eq!(position(src, span.start), LineCol { line: 3, col: 1 });
    assert_eq!(position(src, span.end), LineCol { line: 3, col: 3 });
}

#[test]
fn grouped_doc_tokens_keep_each_source_span() {
    let src = "<?php /** first */ /** second */ function f() {}";
    let stream = Lexer::new(src).lex_token_stream();
    let function = stream
        .kinds
        .iter()
        .position(|kind| *kind == crate::token::TokenKind::KwFunction)
        .unwrap();
    let docs = stream
        .docs
        .iter()
        .filter(|doc| doc.before as usize == function)
        .collect::<Vec<_>>();
    let spans = docs.iter().map(|doc| doc.span).collect::<Vec<_>>();

    assert_eq!(spans.len(), 2);
    assert_eq!(
        &src[spans[0].start as usize..spans[0].end as usize],
        "/** first */"
    );
    assert_eq!(
        &src[spans[1].start as usize..spans[1].end as usize],
        "/** second */"
    );
    assert_eq!(
        docs.last().unwrap().span,
        *spans.last().unwrap(),
        "the declaration doc range is the last adjacent comment"
    );
    assert_eq!(
        spans
            .iter()
            .map(|span| &src[span.start as usize..span.end as usize])
            .collect::<Vec<_>>(),
        vec!["/** first */", "/** second */"]
    );
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
    assert_eq!(position(src, heredoc_end).line, 5);
    let (next_tok, next_span, _) = toks
        .iter()
        .find(|(t, _, _)| *t == Token::Variable("y".to_string().into()))
        .unwrap();
    assert_eq!(*next_tok, Token::Variable("y".to_string().into()));
    assert_eq!(position(src, next_span.start), LineCol { line: 6, col: 1 });
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
    assert_eq!(position(src, span.start), LineCol { line: 2, col: 6 });
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
    assert_eq!(position(src, var_start), LineCol { line: 2, col: 11 });
    assert_eq!(position(src, var_end), LineCol { line: 2, col: 15 });
    assert_eq!(position(src, end), LineCol { line: 2, col: 21 });
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
    assert_eq!(position(src, var_start), LineCol { line: 2, col: 9 });
    assert_eq!(position(src, var_end), LineCol { line: 2, col: 13 });
    assert_eq!(position(src, end), LineCol { line: 2, col: 19 });
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
fn many_double_quoted_strings_on_one_long_line_stay_linear_and_exact() {
    // A double-quoted string resolves positions inside itself before its
    // own start is resolved, so its token starts behind the last position
    // asked for; that must not recount the line from its start (a
    // generated Huffman table: one 1.2 MB line of `"\x00"=>[93,"0"]`).
    let mut src = "<?php $a = [".to_string();
    for i in 0..20_000 {
        if i > 0 {
            src.push(',');
        }
        src.push_str(&format!("\"\\x{:02X}é\"=>[{i},\"0\"]", i % 256));
    }
    src.push_str("];\n");

    let start = std::time::Instant::now();
    let toks = Lexer::new(&src).tokenize_with_spans();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 2,
        "tokenizing one long line of strings took {elapsed:?} - looks quadratic again"
    );

    // The last string starts where counting the line's characters says,
    // each `é` before it one character.
    let last_string = toks
        .iter()
        .rev()
        .find(|(t, _, _)| matches!(t, Token::StringLit(_) | Token::InterpString(_)))
        .expect("a string");
    let expected_col = src
        .rfind("\"0\"")
        .map(|byte| src[..byte].chars().count() + 1)
        .expect("the last string");
    assert_eq!(
        position(&src, last_string.1.start).col as usize,
        expected_col
    );
}

#[test]
fn a_short_echo_tag_after_inline_html_still_echoes() {
    let src = "<title><?= $t ?></title><?= $u ?>";
    let toks: Vec<(Token, Span)> = Lexer::new(src)
        .tokenize_with_spans()
        .into_iter()
        .map(|(t, span, _)| (t, span))
        .collect();
    let echoes = toks
        .iter()
        .filter(|(t, _)| matches!(t, Token::Keyword(Keyword::Echo)))
        .count();
    assert_eq!(echoes, 2, "{toks:?}");
    assert!(
        matches!(&toks[0].0, Token::InlineHtml(span) if span.slice(src.as_bytes()) == Some(b"<title>")),
        "{toks:?}"
    );
}

#[test]
fn inline_html_keeps_non_utf8_source_bytes_in_its_range() {
    let source = b"<x>\xff<?php";
    let tokens = Lexer::from_bytes(source).tokenize_with_spans();
    let Some((Token::InlineHtml(span), _, _)) = tokens.first() else {
        panic!("expected inline HTML token: {tokens:?}");
    };
    assert_eq!(span.slice(source), Some(&b"<x>\xff"[..]));
}

#[test]
fn an_unexpected_byte_is_a_warning_at_its_position_not_output() {
    let src = "<?php\n$a = 1;\x01 $b;";
    let (tokens, _, warnings) = Lexer::new(src).tokenize_with_comments();
    assert_eq!(
        warnings,
        vec![LexWarning {
            span: Span {
                start: src.find('\x01').unwrap() as u32,
                end: src.find('\x01').unwrap() as u32 + 1,
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
