//! Number literals, read as PHP reads them.
use super::super::*;

fn float_of(src: &str) -> f64 {
    let toks = Lexer::new(&format!("<?php {src};")).tokenize_with_spans();
    match &toks[0].0 {
        Token::Float(f) => *f,
        other => panic!("{src} lexed as {other:?}"),
    }
}

#[test]
fn a_binary_or_octal_literal_past_php_int_max_rounds_as_php_computes_it() {
    // `zend_bin_strtod` adds the digit's character code, then subtracts
    // `'0'`: 2^63 comes out one float below.
    assert_eq!(
        float_of(&format!("0b1{}", "0".repeat(63))),
        9223372036854774784.0
    );
    assert_eq!(
        float_of(&format!("0b1{}", "0".repeat(64))),
        18446744073709549568.0
    );
    assert_eq!(
        float_of(&format!("0o1{}", "0".repeat(21))),
        9223372036854775808.0
    );
    assert_eq!(float_of("0x8000000000000000"), 9223372036854775808.0);
}
