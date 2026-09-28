//! Heredoc/nowdoc parsing and string interpolation.
use super::super::*;
use crate::token::{Position, Span, StringPart};

// Heredoc/nowdoc are real, common PHP syntax - flexible/PHP-7.3+
// indentation stripping (`strip_heredoc_indent`) in particular.

#[test]
fn nowdoc_strips_the_closing_markers_indentation_with_no_interpolation() {
    let src = "<?php\n$x = <<<'EOT'\n    hello\n    world\n    EOT;\n";
    let toks = Lexer::new(src).tokenize();
    assert_eq!(toks[2], Token::StringLit("hello\nworld".into()));
}

#[test]
fn heredoc_strips_indentation_and_still_interpolates_variables() {
    let src = "<?php\n$x = <<<EOT\n    Hello, $name!\n      {$o->p}\n    EOT;\n";
    let toks = Lexer::new(src).tokenize();
    let at = |line, col| Position { line, col };
    let span = |start, end| Span { start, end };
    assert_eq!(
        toks[2],
        Token::InterpString(vec![
            StringPart::Literal("Hello, ".into(), span(at(3, 5), at(3, 12))),
            StringPart::SimpleVar("name".into(), span(at(3, 12), at(3, 17))),
            StringPart::Literal("!\n  ".into(), span(at(3, 17), at(4, 7))),
            StringPart::PropertyAccess {
                var: "o".into(),
                var_start: at(4, 8),
                var_end: at(4, 10),
                prop: "p".into(),
                end: at(4, 13),
            },
        ])
    );
}

#[test]
fn each_part_of_a_double_quoted_string_spans_its_own_text() {
    let src = "<?php \"a $x b ${y}\";";
    let toks = Lexer::new(src).tokenize();
    let at = |line, col| Position { line, col };
    let span = |start, end| Span { start, end };
    assert_eq!(
        toks[0],
        Token::InterpString(vec![
            StringPart::Literal("a ".into(), span(at(1, 8), at(1, 10))),
            StringPart::SimpleVar("x".into(), span(at(1, 10), at(1, 12))),
            StringPart::Literal(" b ".into(), span(at(1, 12), at(1, 15))),
            StringPart::SimpleVar("y".into(), span(at(1, 15), at(1, 19))),
        ])
    );
}

#[test]
fn unterminated_heredoc_does_not_hang_or_panic() {
    let src = "<?php\n$x = <<<EOT\nno closing label here\n";
    // Only the absence of a panic/infinite loop matters here - an
    // unterminated heredoc is malformed input, and the documented
    // behavior is to treat the rest of the file as its body rather
    // than error out, matching this lexer's error-recovery style
    // elsewhere.
    let _ = Lexer::new(src).tokenize();
}

// {$...} complex interpolation's own brace-depth counters (two
// independent ones: lex_interpolated_run scanning for the block's own
// extent, and scan_interpolated's separate pass carving it into
// StringPart pieces) must stay string-aware - a brace inside a nested
// string literal must not throw off the count.

#[test]
fn complex_interpolation_is_not_fooled_by_a_brace_inside_a_nested_string_argument() {
    let src = r#"<?php "value: {$f->wrap('}')} end";"#;
    let toks = Lexer::new(src).tokenize();
    // Must tokenize successfully through to Eof - the interpolation's
    // own ComplexExpr part additionally proves the block's real extent
    // (through the true closing brace, not the one inside the string)
    // was found correctly.
    assert_eq!(toks.last(), Some(&Token::Eof));
    let Token::InterpString(parts) = &toks[0] else {
        panic!("expected an interpolated string, got {:?}", toks[0]);
    };
    assert!(
        parts
            .iter()
            .any(|p| matches!(p, StringPart::ComplexExpr(inner, _) if inner == "$f->wrap('}')")),
        "got: {parts:?}"
    );
}
