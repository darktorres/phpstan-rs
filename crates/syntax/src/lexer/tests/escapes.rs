//! String escape sequence decoding.
use super::super::*;
use crate::php_string::PhpString;
use crate::token::StringPart;

fn assert_lone_literal(tok: &Token, expected: PhpString) {
    let Token::InterpString(parts) = tok else {
        panic!("expected an interpolated string, got {tok:?}");
    };
    let [StringPart::Literal(s, _)] = parts.as_slice() else {
        panic!("expected one literal part, got {parts:?}");
    };
    assert_eq!(*s, expected);
}

// All of PHP's real double-quoted escape sequences must decode, not
// just a handful - anything unrecognized keeps the literal backslash
// and following character verbatim.

#[test]
fn hex_escape_decodes_to_the_real_byte() {
    let src = r#"<?php "\x41BC";"#;
    let toks = Lexer::new(src).tokenize();
    assert_lone_literal(&toks[0], "ABC".to_string().into());
}

#[test]
fn a_multi_byte_utf8_character_in_a_double_quoted_literal_run_survives_intact() {
    // scan_interpolated's literal-run handling batches via
    // `String::from_utf8_lossy` over the whole run - pushing plain bytes
    // one at a time via `b as char` would reinterpret each raw UTF-8
    // continuation byte as its own Latin-1 codepoint, corrupting any
    // multi-byte character.
    let src = "<?php \"caf\u{e9} \u{2764}\";";
    let toks = Lexer::new(src).tokenize();
    assert_lone_literal(&toks[0], "caf\u{e9} \u{2764}".to_string().into());
}

#[test]
fn a_multi_byte_utf8_character_in_a_single_quoted_string_survives_intact() {
    // Single-quoted twin: `lex_single_quoted` has its own separate
    // batched literal path needing the same handling.
    let src = "<?php 'caf\u{e9} \u{2764}';";
    let toks = Lexer::new(src).tokenize();
    assert_eq!(toks[0], Token::StringLit("caf\u{e9} \u{2764}".into()));
}

#[test]
fn octal_escape_decodes_to_the_real_byte() {
    let src = r#"<?php "\101BC";"#;
    let toks = Lexer::new(src).tokenize();
    assert_lone_literal(&toks[0], "ABC".to_string().into());
}

#[test]
fn unicode_escape_decodes_to_the_real_codepoint() {
    let src = r#"<?php "\u{41}BC";"#;
    let toks = Lexer::new(src).tokenize();
    assert_lone_literal(&toks[0], "ABC".to_string().into());
}

#[test]
fn vertical_tab_escape_and_form_feed_decode_correctly() {
    let src = r#"<?php "\v\e\f";"#;
    let toks = Lexer::new(src).tokenize();
    assert_lone_literal(&toks[0], "\u{0B}\u{1B}\u{0C}".to_string().into());
}

#[test]
fn an_unrecognized_escape_still_keeps_the_literal_backslash_and_character() {
    let src = r#"<?php "\d+";"#;
    let toks = Lexer::new(src).tokenize();
    assert_lone_literal(&toks[0], "\\d+".to_string().into());
}

#[test]
fn the_original_six_escapes_still_decode_correctly() {
    let src = r#"<?php "a\nb\tc\r\"d\\e\$f";"#;
    let toks = Lexer::new(src).tokenize();
    assert_lone_literal(&toks[0], "a\nb\tc\r\"d\\e$f".to_string().into());
}

#[test]
fn a_hex_or_octal_escape_is_one_byte_not_a_codepoint() {
    let toks = Lexer::new(r#"<?php "\xff\377";"#).tokenize();
    assert_lone_literal(&toks[0], PhpString::from(vec![0xff, 0xff]));
}

#[test]
fn a_non_utf8_source_byte_stays_as_written() {
    let src: &[u8] = b"<?php '\xe9t\xe9';";
    let toks = Lexer::from_bytes(src).tokenize();
    assert_eq!(
        toks[0],
        Token::StringLit(PhpString::from(vec![0xe9, b't', 0xe9]))
    );
}

#[test]
fn a_heredoc_keeps_a_backslash_before_a_double_quote() {
    let toks = Lexer::new("<?php <<<EOT\nsay \\\"hi\\\"\nEOT;\n").tokenize();
    assert_lone_literal(&toks[0], "say \\\"hi\\\"".into());
}
