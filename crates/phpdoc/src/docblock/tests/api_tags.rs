//! Tests for parsing `@api`/`@api-do-not-implement` tags.
use super::super::*;

#[test]
fn parses_bare_api_tag() {
    let block = parse(
        r#"/**
         * @api
         */"#,
    );
    assert!(block.api);
    assert!(!block.api_do_not_implement);
}

#[test]
fn parses_api_do_not_implement_alongside_api() {
    let block = parse(
        r#"/**
         * @api
         * @api-do-not-implement
         */"#,
    );
    assert!(block.api);
    assert!(block.api_do_not_implement);
}

#[test]
fn api_do_not_implement_alone_still_implies_api() {
    let block = parse(
        r#"/**
         * @api-do-not-implement
         */"#,
    );
    assert!(block.api);
    assert!(block.api_do_not_implement);
}

#[test]
fn a_docblock_with_neither_tag_is_not_api() {
    let block = parse(
        r#"/**
         * Just a regular class.
         */"#,
    );
    assert!(!block.api);
    assert!(!block.api_do_not_implement);
}

#[test]
fn phpstan_prefixed_api_tag_is_recognized_the_same_as_the_plain_one() {
    let block = parse(
        r#"/**
         * @phpstan-api
         */"#,
    );
    assert!(block.api);
}

#[test]
fn a_see_target_is_its_first_word_with_an_argument_list_kept_whole() {
    let block = parse(
        "/**\n * @see Foo::bar(int $x) for the details\n * @see https://example.com/a b\n * @see \\Baz\n */",
    );
    assert_eq!(
        block.see,
        vec![
            "Foo::bar(int $x)".to_string(),
            "https://example.com/a".to_string(),
            "\\Baz".to_string(),
        ]
    );
}
