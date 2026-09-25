//! Tests for parsing `@template*`, `@extends`, and `@implements` docblock tags.
use super::super::*;
use super::node;

#[test]
fn parses_extends_with_generic_args() {
    let block = parse("/** @extends Collection<int> */");
    assert_eq!(block.extends, Some(node("Collection<int>")));
}

#[test]
fn parses_multiple_implements_tags() {
    let block = parse(
        r#"/**
         * @implements Iterator<int, string>
         * @implements Countable<int>
         */"#,
    );
    assert_eq!(
        block.implements,
        vec![node("Iterator<int, string>"), node("Countable<int>")]
    );
}

#[test]
fn parses_a_use_tag_with_generic_args() {
    let block = parse("/** @use TemplateTypeTrait<ObjectType> */");
    assert_eq!(block.uses, vec![node("TemplateTypeTrait<ObjectType>")]);
}

#[test]
fn parses_a_template_use_tag_as_a_synonym_for_use() {
    let block = parse("/** @template-use Repo<int> */");
    assert_eq!(block.uses, vec![node("Repo<int>")]);
}

#[test]
fn parses_a_phpstan_use_tag_as_a_synonym_for_use() {
    let block = parse("/** @phpstan-use Repo<int> */");
    assert_eq!(block.uses, vec![node("Repo<int>")]);
}

#[test]
fn generic_type_with_comma_and_space_is_not_truncated() {
    let block = parse("/** @param array<int, User> $users */");
    assert_eq!(block.params.get("users"), Some(&node("array<int, User>")));
}

#[test]
fn parses_template_name() {
    let block = parse(
        r#"/**
         * @template T
         * @param T $value
         * @return T
         */"#,
    );
    assert_eq!(block.templates, vec!["T".to_string()]);
    assert_eq!(block.params.get("value"), Some(&node("T")));
    assert_eq!(block.return_type, Some(node("T")));
}

#[test]
fn parses_bounded_template_name_and_its_bound() {
    let block = parse("/** @template T of \\Countable */");
    assert_eq!(block.templates, vec!["T".to_string()]);
    assert_eq!(block.template_bounds.get("T"), Some(&node("\\Countable")));
}

#[test]
fn a_template_with_no_bound_has_no_bound_entry() {
    let block = parse("/** @template T */");
    assert_eq!(block.templates, vec!["T".to_string()]);
    assert!(!block.template_bounds.contains_key("T"));
}

#[test]
fn parses_a_union_bound() {
    let block = parse("/** @template T of Foo|Bar */");
    assert_eq!(block.template_bounds.get("T"), Some(&node("Foo|Bar")));
}

#[test]
fn parses_a_template_default() {
    let block = parse("/** @template T = int */");
    assert_eq!(block.templates, vec!["T".to_string()]);
    assert_eq!(block.template_defaults.get("T"), Some(&node("int")));
    assert!(!block.template_bounds.contains_key("T"));
}

#[test]
fn parses_a_bound_and_a_default_together() {
    let block = parse("/** @template T of Shape = Circle */");
    assert_eq!(block.template_bounds.get("T"), Some(&node("Shape")));
    assert_eq!(block.template_defaults.get("T"), Some(&node("Circle")));
}

#[test]
fn a_template_with_no_default_has_no_default_entry() {
    let block = parse("/** @template T */");
    assert!(!block.template_defaults.contains_key("T"));
}

#[test]
fn parses_a_default_on_a_covariant_template_tag() {
    let block = parse("/** @template-covariant T = int */");
    assert_eq!(block.template_defaults.get("T"), Some(&node("int")));
    assert!(block.covariant_templates.contains("T"));
}

#[test]
fn parses_covariant_and_contravariant_template_tags() {
    let block = parse(
        r#"/**
         * @template-covariant TOut
         * @template-contravariant TIn of Bound
         */"#,
    );
    assert_eq!(block.templates, vec!["TOut".to_string(), "TIn".to_string()]);
    assert!(block.covariant_templates.contains("TOut"));
    assert!(!block.contravariant_templates.contains("TOut"));
    assert!(block.contravariant_templates.contains("TIn"));
    assert_eq!(block.template_bounds.get("TIn"), Some(&node("Bound")));
}

#[test]
fn parses_require_extends_and_require_implements() {
    let block = parse(
        r#"/**
         * @phpstan-require-extends Base
         * @phpstan-require-implements Contract
         */"#,
    );
    assert_eq!(block.require_extends, vec![node("Base")]);
    assert_eq!(block.require_implements, vec![node("Contract")]);
}

#[test]
fn parses_a_sealed_tag_naming_a_union_of_classes() {
    let block = parse("/** @phpstan-sealed ChildA|ChildB */");
    assert_eq!(block.sealed, vec![node("ChildA|ChildB")]);
}

#[test]
fn templates_keep_first_appearance_order_across_prefixes() {
    let d = parse(
        "/**\n * @phpstan-template TKey of array-key\n * @template-covariant T\n * @template-extends IteratorAggregate<TKey, T>\n */",
    );
    assert_eq!(d.templates, ["TKey", "T"]);
}
