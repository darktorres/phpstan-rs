//! `@phpstan-assert`/`-if-true`/`-if-false` tags - real PHPStan's
//! custom-assertion-function convention.
use super::super::*;

#[test]
fn parses_an_assert_if_true_tag_naming_a_this_property() {
    let block = parse("/** @phpstan-assert-if-true !null $this->testFilesFile */");
    assert_eq!(
        block.assert_if_true,
        vec![AssertTag {
            negated: true,
            ty: "null".to_string(),
            target: "$this->testFilesFile".to_string(),
        }]
    );
}

#[test]
fn parses_an_assert_if_false_tag() {
    let block = parse("/** @phpstan-assert-if-false !null $this->parseError() */");
    assert_eq!(
        block.assert_if_false,
        vec![AssertTag {
            negated: true,
            ty: "null".to_string(),
            target: "$this->parseError()".to_string(),
        }]
    );
}

#[test]
fn parses_an_unconditional_assert_tag_naming_a_parameter() {
    let block = parse("/** @phpstan-assert string $actual */");
    assert_eq!(
        block.assert,
        vec![AssertTag {
            negated: false,
            ty: "string".to_string(),
            target: "$actual".to_string(),
        }]
    );
}

#[test]
fn parses_a_bare_this_target() {
    let block = parse("/** @phpstan-assert-if-true After $this */");
    assert_eq!(
        block.assert_if_true,
        vec![AssertTag {
            negated: false,
            ty: "After".to_string(),
            target: "$this".to_string(),
        }]
    );
}

#[test]
fn a_template_equality_form_is_never_parsed() {
    let block = parse("/** @phpstan-assert =ExpectedType $actual */");
    assert!(block.assert.is_empty());
}

#[test]
fn a_value_inequality_form_is_never_parsed() {
    let block = parse("/** @phpstan-assert-if-true !=null $this->getId() */");
    assert!(block.assert_if_true.is_empty());
}

#[test]
fn a_generic_type_argument_is_kept_as_part_of_the_type_text() {
    let block = parse("/** @phpstan-assert-if-true Collection<TKey,T> $this->collection */");
    assert_eq!(
        block.assert_if_true,
        vec![AssertTag {
            negated: false,
            ty: "Collection<TKey,T>".to_string(),
            target: "$this->collection".to_string(),
        }]
    );
}

#[test]
fn no_assert_tags_at_all_leaves_every_list_empty() {
    let block = parse("/** @template T */");
    assert!(block.assert.is_empty());
    assert!(block.assert_if_true.is_empty());
    assert!(block.assert_if_false.is_empty());
}
