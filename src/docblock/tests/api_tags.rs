//! Tests for parsing the Phase 40 `@api`/`@api-do-not-implement` tags.
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
