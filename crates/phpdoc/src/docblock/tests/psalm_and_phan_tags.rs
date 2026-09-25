//! Tests for `@psalm-`/`@phan-`-prefixed tag spellings and their
//! precedence against plain and `@phpstan-` tags.
use super::super::*;
use super::node;

#[test]
fn psalm_param_and_return_override_the_plain_tags() {
    let block = parse(
        r#"/**
         * @param array $items
         * @return array
         * @psalm-param list<int> $items
         * @psalm-return non-empty-list<string>
         */"#,
    );
    assert_eq!(block.params.get("items"), Some(&node("list<int>")));
    assert_eq!(block.return_type, Some(node("non-empty-list<string>")));
}

#[test]
fn phpstan_spelling_beats_psalm_which_beats_phan_regardless_of_order() {
    let block = parse(
        r#"/**
         * @phpstan-return int
         * @psalm-return string
         * @phan-return float
         * @return bool
         */"#,
    );
    assert_eq!(block.return_type, Some(node("int")));

    let block = parse(
        r#"/**
         * @psalm-param string $x
         * @phan-param float $x
         */"#,
    );
    assert_eq!(block.params.get("x"), Some(&node("string")));
}

#[test]
fn psalm_var_sets_the_var_type() {
    let block = parse("/** @psalm-var array<string, int> $map */");
    assert_eq!(block.var_type, Some(node("array<string, int>")));
    assert_eq!(block.var_name, Some("map".to_string()));
}

#[test]
fn psalm_assert_is_used_when_no_phpstan_assert_exists() {
    let block = parse(
        r#"/**
         * @psalm-assert string $value
         */"#,
    );
    assert_eq!(block.assert.len(), 1);
    assert_eq!(block.assert[0].ty, node("string"));
    assert_eq!(block.assert[0].target, "$value");
}

#[test]
fn phpstan_assert_wins_over_psalm_assert_entirely() {
    let block = parse(
        r#"/**
         * @psalm-assert string $value
         * @psalm-assert-if-true int $other
         * @phpstan-assert non-empty-string $value
         */"#,
    );
    assert_eq!(block.assert.len(), 1);
    assert_eq!(block.assert[0].ty, node("non-empty-string"));
    assert!(block.assert_if_true.is_empty());
}

#[test]
fn psalm_assert_wins_over_phan_assert() {
    let block = parse(
        r#"/**
         * @phan-assert int $value
         * @psalm-assert string $value
         */"#,
    );
    assert_eq!(block.assert.len(), 1);
    assert_eq!(block.assert[0].ty, node("string"));
}

#[test]
fn a_psalm_tag_with_an_unparsable_type_is_skipped_and_the_plain_tag_stays() {
    let block = parse(
        r#"/**
         * @return array
         * @psalm-return array<int
         */"#,
    );
    assert_eq!(block.return_type, Some(node("array")));
    assert!(block.parse_errors.is_empty());
}

#[test]
fn psalm_only_vocabulary_is_ignored() {
    let block = parse(
        r#"/**
         * @psalm-suppress MixedAssignment
         * @psalm-internal App\Internal
         */"#,
    );
    assert!(!block.internal);
}

#[test]
fn psalm_private_mutation_tags_map_onto_their_phpstan_meaning() {
    let block = parse("/** @psalm-allow-private-mutation */");
    assert!(block.allow_private_mutation);
    assert!(!block.readonly);

    let block = parse("/** @psalm-readonly-allow-private-mutation */");
    assert!(block.allow_private_mutation);
    assert!(block.readonly);

    let block = parse("/** @phpstan-readonly-allow-private-mutation */");
    assert!(block.allow_private_mutation);
    assert!(block.readonly);
}

#[test]
fn renamed_psalm_and_phan_spellings_map_onto_their_phpstan_meaning() {
    let block = parse(
        r#"/**
         * @psalm-inheritors Foo|Bar
         * @psalm-template-covariant T
         * @psalm-require-extends Base
         * @psalm-consistent-constructor
         * @psalm-readonly
         * @phan-side-effect-free
         */"#,
    );
    assert_eq!(block.sealed, vec![node("Foo|Bar")]);
    assert_eq!(block.templates, vec!["T".to_string()]);
    assert!(block.covariant_templates.contains("T"));
    assert_eq!(block.require_extends, vec![node("Base")]);
    assert!(block.consistent_constructor);
    assert!(block.readonly);
    assert!(block.pure);

    let block = parse(
        r#"/**
         * @phan-real-return int
         * @phan-extends Collection<int>
         * @phan-read-only
         */"#,
    );
    assert_eq!(block.return_type, Some(node("int")));
    assert_eq!(block.extends, Some(node("Collection<int>")));
    assert!(block.readonly);
}

#[test]
fn psalm_type_and_import_type_are_recognized() {
    let block = parse(
        r#"/**
         * @psalm-type Row = array{id: int}
         * @psalm-import-type Other from Source
         */"#,
    );
    assert_eq!(block.type_aliases.get("Row"), Some(&node("array{id: int}")));
    assert_eq!(block.type_imports.len(), 1);
    assert_eq!(block.type_imports[0].alias, "Other");
}
