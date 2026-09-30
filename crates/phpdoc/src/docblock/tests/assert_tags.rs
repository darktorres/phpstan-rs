//! `@phpstan-assert`/`-if-true`/`-if-false` tags - real PHPStan's
//! custom-assertion-function convention.
use super::super::*;
use super::node;

#[test]
fn parses_an_assert_if_true_tag_naming_a_this_property() {
    let block = parse("/** @phpstan-assert-if-true !null $this->testFilesFile */");
    assert_eq!(
        block.assert_if_true,
        vec![AssertTag {
            equality: false,
            negated: true,
            ty: node("null"),
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
            equality: false,
            negated: true,
            ty: node("null"),
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
            equality: false,
            negated: false,
            ty: node("string"),
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
            equality: false,
            negated: false,
            ty: node("After"),
            target: "$this".to_string(),
        }]
    );
}

#[test]
fn a_generic_type_argument_is_kept_as_part_of_the_type_text() {
    let block = parse("/** @phpstan-assert-if-true Collection<TKey,T> $this->collection */");
    assert_eq!(
        block.assert_if_true,
        vec![AssertTag {
            equality: false,
            negated: false,
            ty: node("Collection<TKey, T>"),
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

#[test]
fn an_equality_assertion_is_read_with_its_negation() {
    let block = parse("/**\n * @phpstan-assert =T $actual\n * @phpstan-assert !=T $other\n */");
    assert_eq!(
        block.assert,
        vec![
            AssertTag {
                equality: true,
                negated: false,
                ty: node("T"),
                target: "$actual".to_string(),
            },
            AssertTag {
                equality: true,
                negated: true,
                ty: node("T"),
                target: "$other".to_string(),
            },
        ]
    );
}

#[test]
fn an_inequality_form_on_a_call_target_is_read() {
    let block = parse("/** @phpstan-assert-if-true !=null $this->getId() */");
    assert_eq!(
        block.assert_if_true,
        vec![AssertTag {
            equality: true,
            negated: true,
            ty: node("null"),
            target: "$this->getId()".to_string(),
        }]
    );
}
