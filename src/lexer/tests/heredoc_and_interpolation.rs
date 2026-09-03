//! Heredoc/nowdoc parsing and string interpolation.
use super::super::*;

// Coverage measurement (cargo llvm-cov) found lex_heredoc and
// strip_heredoc_indent had zero test executions - lexer.rs had no
// test module at all before this. Heredoc/nowdoc are real, common
// PHP syntax (flexible/PHP-7.3+ indentation stripping in
// particular), verified manually correct before writing these.

#[test]
fn nowdoc_strips_the_closing_markers_indentation_with_no_interpolation() {
    let src = "<?php\n$x = <<<'EOT'\n    hello\n    world\n    EOT;\n";
    let toks = Lexer::new(src).tokenize();
    assert_eq!(toks[2], Token::StringLit("hello\nworld".to_string()));
}

#[test]
fn heredoc_strips_indentation_and_still_interpolates_variables() {
    let src = "<?php\n$x = <<<EOT\n    Hello, $name!\n    EOT;\n";
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[2],
        Token::InterpString(vec![
            StringPart::Literal("Hello, ".to_string()),
            StringPart::SimpleVar("name".to_string()),
            StringPart::Literal("!".to_string()),
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

// Finding 10 from the architecture audit - the same string-
// unawareness bug as Finding 6, in {$...} complex interpolation's
// own brace-depth counters (two independent ones: lex_interpolated_run
// scanning for the block's own extent, and scan_interpolated's
// separate pass carving it into StringPart pieces).

#[test]
fn complex_interpolation_is_not_fooled_by_a_brace_inside_a_nested_string_argument() {
    let src = r#"<?php "value: {$f->wrap('}')} end";"#;
    let toks = Lexer::new(src).tokenize();
    // A successful, non-panicking tokenize that reaches Eof (rather
    // than the old parse error stopping partway through the file)
    // is the real regression test here - the interpolation's own
    // ComplexExpr part additionally proves the block's real extent
    // (through the true closing brace, not the one inside the
    // string) was found correctly.
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
