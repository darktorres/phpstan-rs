//! String escape sequence decoding.
use super::super::*;
use crate::token::StringPart;

// Finding 20 from the architecture audit - double-quoted strings only
// decoded 6 of PHP's real escape sequences; anything else kept the
// literal backslash and following character verbatim.

#[test]
fn hex_escape_decodes_to_the_real_byte() {
    let src = r#"<?php "\x41BC";"#;
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[0],
        Token::InterpString(vec![StringPart::Literal("ABC".to_string())])
    );
}

#[test]
fn a_multi_byte_utf8_character_in_a_double_quoted_literal_run_survives_intact() {
    // Regression test for a perf-optimization fix: scan_interpolated's
    // literal-run handling used to push plain bytes one at a time via
    // `b as char`, which reinterprets each raw UTF-8 continuation byte
    // as its own Latin-1 codepoint - silently corrupting any multi-byte
    // character. Now batched via `String::from_utf8_lossy` over the
    // whole run instead.
    let src = "<?php \"caf\u{e9} \u{2764}\";";
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[0],
        Token::InterpString(vec![StringPart::Literal("caf\u{e9} \u{2764}".to_string())])
    );
}

#[test]
fn a_multi_byte_utf8_character_in_a_single_quoted_string_survives_intact() {
    // Same fix, single-quoted twin (lex_single_quoted has its own
    // separate byte-by-byte-turned-batched literal path).
    let src = "<?php 'caf\u{e9} \u{2764}';";
    let toks = Lexer::new(src).tokenize();
    assert_eq!(toks[0], Token::StringLit("caf\u{e9} \u{2764}".to_string()));
}

#[test]
fn octal_escape_decodes_to_the_real_byte() {
    let src = r#"<?php "\101BC";"#;
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[0],
        Token::InterpString(vec![StringPart::Literal("ABC".to_string())])
    );
}

#[test]
fn unicode_escape_decodes_to_the_real_codepoint() {
    let src = r#"<?php "\u{41}BC";"#;
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[0],
        Token::InterpString(vec![StringPart::Literal("ABC".to_string())])
    );
}

#[test]
fn vertical_tab_escape_and_form_feed_decode_correctly() {
    let src = r#"<?php "\v\e\f";"#;
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[0],
        Token::InterpString(vec![StringPart::Literal("\u{0B}\u{1B}\u{0C}".to_string())])
    );
}

#[test]
fn an_unrecognized_escape_still_keeps_the_literal_backslash_and_character() {
    let src = r#"<?php "\d+";"#;
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[0],
        Token::InterpString(vec![StringPart::Literal("\\d+".to_string())])
    );
}

#[test]
fn the_original_six_escapes_still_decode_correctly() {
    let src = r#"<?php "a\nb\tc\r\"d\\e\$f";"#;
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks[0],
        Token::InterpString(vec![StringPart::Literal("a\nb\tc\r\"d\\e$f".to_string())])
    );
}
