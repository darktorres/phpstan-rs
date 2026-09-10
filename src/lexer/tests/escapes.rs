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
