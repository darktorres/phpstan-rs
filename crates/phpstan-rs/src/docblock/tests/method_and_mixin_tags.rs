//! Tests for parsing `@method`/`@mixin`.
use super::super::*;
use super::node;

#[test]
fn parses_a_bare_method_with_no_params_or_return_type() {
    let block = parse("/** @method getX() */");
    assert_eq!(block.method_tags.len(), 1, "got: {:?}", block.method_tags);
    let tag = &block.method_tags[0];
    assert!(!tag.is_static);
    assert_eq!(tag.return_type, None);
    assert_eq!(tag.name, "getX");
    assert!(tag.params.is_empty());
}

#[test]
fn parses_a_return_type_and_a_typed_param() {
    let block = parse("/** @method int getX(string $name) */");
    let tag = &block.method_tags[0];
    assert_eq!(tag.return_type, Some(node("int")));
    assert_eq!(tag.name, "getX");
    assert_eq!(tag.params.len(), 1);
    assert_eq!(tag.params[0].ty, Some(node("string")));
    assert_eq!(tag.params[0].name, "name");
    assert!(!tag.params[0].has_default);
}

#[test]
fn parses_a_static_method() {
    let block = parse("/** @method static self make() */");
    let tag = &block.method_tags[0];
    assert!(tag.is_static);
    assert_eq!(tag.return_type, Some(node("self")));
    assert_eq!(tag.name, "make");
}

#[test]
fn parses_a_generic_return_type_without_confusing_it_with_the_method_name() {
    let block = parse("/** @method array<int, int> ftText(float $size, ImageColor $color) */");
    let tag = &block.method_tags[0];
    assert_eq!(tag.return_type, Some(node("array<int, int>")));
    assert_eq!(tag.name, "ftText");
    assert_eq!(tag.params.len(), 2);
    assert_eq!(tag.params[0].name, "size");
    assert_eq!(tag.params[1].name, "color");
}

#[test]
fn a_default_value_containing_a_top_level_comma_does_not_split_the_param_list() {
    let block = parse("/** @method array getList(array<string, mixed> $options = []) */");
    let tag = &block.method_tags[0];
    assert_eq!(tag.params.len(), 1);
    assert_eq!(tag.params[0].ty, Some(node("array<string, mixed>")));
    assert_eq!(tag.params[0].name, "options");
    assert!(tag.params[0].has_default);
}

#[test]
fn parses_a_variadic_param() {
    let block = parse("/** @method bool hasList(string ...$names) */");
    let tag = &block.method_tags[0];
    assert_eq!(tag.params.len(), 1);
    assert_eq!(tag.params[0].name, "names");
    assert!(tag.params[0].variadic);
}

#[test]
fn parses_a_by_ref_param() {
    let block = parse("/** @method void sort(array &$items) */");
    let tag = &block.method_tags[0];
    assert_eq!(tag.params.len(), 1);
    assert_eq!(tag.params[0].name, "items");
    assert!(tag.params[0].by_ref);
}

#[test]
fn parses_a_param_with_no_declared_type() {
    let block = parse("/** @method void set($value) */");
    let tag = &block.method_tags[0];
    assert_eq!(tag.params.len(), 1);
    assert_eq!(tag.params[0].ty, None);
    assert_eq!(tag.params[0].name, "value");
}

#[test]
fn trailing_free_text_description_is_ignored() {
    let block =
        parse("/** @method bool hasStyle(string $style) Tells whether the style exists. */");
    let tag = &block.method_tags[0];
    assert_eq!(tag.name, "hasStyle");
    assert_eq!(tag.params.len(), 1);
}

#[test]
fn parses_multiple_method_tags() {
    let block = parse(
        r#"/**
         * @method int getX()
         * @method int getY()
         */"#,
    );
    assert_eq!(block.method_tags.len(), 2);
    assert_eq!(block.method_tags[0].name, "getX");
    assert_eq!(block.method_tags[1].name, "getY");
}

#[test]
fn parses_a_mixin_tag() {
    let block = parse("/** @mixin Doctrine\\Collection */");
    assert_eq!(block.mixins, vec![node("Doctrine\\Collection")]);
}

#[test]
fn parses_multiple_mixin_tags() {
    let block = parse(
        r#"/**
         * @mixin FirstMixin
         * @mixin SecondMixin
         */"#,
    );
    assert_eq!(block.mixins, vec![node("FirstMixin"), node("SecondMixin")]);
}
