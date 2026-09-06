//! Tests for parsing `@phpstan-type Name TypeExpr` local type aliases.
use super::super::*;

#[test]
fn parses_a_simple_type_alias() {
    let block = parse("/** @phpstan-type Error array{code: int, message: string} */");
    assert_eq!(
        block.type_aliases.get("Error"),
        Some(&"array{code: int, message: string}".to_string())
    );
}

#[test]
fn parses_a_type_alias_referencing_another_one() {
    let block = parse("/** @phpstan-type ErrorList list<Error> */");
    assert_eq!(
        block.type_aliases.get("ErrorList"),
        Some(&"list<Error>".to_string())
    );
}

#[test]
fn parses_multiple_type_aliases_on_the_same_docblock() {
    let block = parse(
        r#"/**
         * @phpstan-type Error array{code: int, message: string}
         * @phpstan-type ErrorList list<Error>
         */"#,
    );
    assert_eq!(block.type_aliases.len(), 2);
    assert_eq!(
        block.type_aliases.get("Error"),
        Some(&"array{code: int, message: string}".to_string())
    );
    assert_eq!(
        block.type_aliases.get("ErrorList"),
        Some(&"list<Error>".to_string())
    );
}

#[test]
fn parses_a_type_alias_using_the_equals_sign_variant() {
    let block = parse("/** @phpstan-type ConcreteMapping = OneToOne|ManyToOne */");
    assert_eq!(
        block.type_aliases.get("ConcreteMapping"),
        Some(&"OneToOne|ManyToOne".to_string())
    );
}

#[test]
fn parses_a_type_alias_of_a_quoted_string_literal_union() {
    let block = parse("/** @phpstan-type ValidCombinatorValue ' '|'>'|'+'|'~' */");
    assert_eq!(
        block.type_aliases.get("ValidCombinatorValue"),
        Some(&"' '|'>'|'+'|'~'".to_string())
    );
}

#[test]
fn a_type_alias_with_no_type_expression_is_not_recorded() {
    let block = parse("/** @phpstan-type Error */");
    assert!(
        block.type_aliases.is_empty(),
        "got: {:?}",
        block.type_aliases
    );
}
