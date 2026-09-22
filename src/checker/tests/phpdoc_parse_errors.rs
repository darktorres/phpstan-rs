//! Real PHPStan's `InvalidPhpDocTagValueRule` (`phpDoc.parseError`): a tag
//! whose value phpdoc-parser can't parse. The shapes below are the ones a
//! real Piwigo corpus comparison against `vendor/bin/phpstan` turned up.
use super::helpers::*;

fn parse_errors(src: &str) -> Vec<String> {
    diagnostics_for(src)
        .into_iter()
        .filter(|d| d.identifier == "phpDoc.parseError")
        .map(|d| d.message)
        .collect()
}

#[test]
fn a_param_tag_with_a_type_but_no_variable_is_flagged() {
    let errors = parse_errors(
        r#"<?php
            /**
             * @param c13y object
             */
            function f($a) {}
        "#,
    );
    assert_eq!(
        errors,
        vec![
            "PHPDoc tag @param has invalid value (c13y object): Unexpected token \"object\", expected variable"
                .to_string()
        ]
    );
}

#[test]
fn a_typeless_param_tag_and_a_variableless_var_tag_are_both_valid() {
    let errors = parse_errors(
        r#"<?php
            /**
             * @param $a the first one
             * @return int the answer
             */
            function f($a) { /** @var int */ $x = 1; return $x; }
        "#,
    );
    assert!(errors.is_empty(), "got: {errors:?}");
}

#[test]
fn a_var_tag_with_the_variable_before_the_type_is_flagged() {
    let errors = parse_errors(
        r#"<?php
            function f(): void {
                /** @var $node \DOMNode */
                $node = null;
            }
        "#,
    );
    assert_eq!(
        errors,
        vec!["PHPDoc tag @var has invalid value ($node \\DOMNode): Unexpected token \"$node\", expected type".to_string()]
    );
}

#[test]
fn a_description_glued_to_its_type_is_flagged() {
    let errors = parse_errors(
        r#"<?php
            /** @return array(width, height) */
            function f() { return []; }
            /** @return array<array><string> */
            function g() { return []; }
        "#,
    );
    assert_eq!(errors.len(), 2, "got: {errors:?}");
    assert!(
        errors
            .iter()
            .all(|e| e.ends_with("expected TOKEN_HORIZONTAL_WS")),
        "got: {errors:?}"
    );
}

#[test]
fn a_return_tag_starting_with_a_variable_is_flagged_but_this_is_a_type() {
    let errors = parse_errors(
        r#"<?php
            class C {
                /** @return $url embellished */
                public function f() { return ''; }
                /** @return $this */
                public function g() { return $this; }
            }
        "#,
    );
    assert_eq!(
        errors,
        vec!["PHPDoc tag @return has invalid value ($url embellished): Unexpected token \"$url\", expected type".to_string()]
    );
}

#[test]
fn an_offset_access_type_is_valid_syntax() {
    let errors = parse_errors(
        r#"<?php
            /** @param string[optional] $path */
            function f($path) {}
        "#,
    );
    assert!(errors.is_empty(), "got: {errors:?}");
}

#[test]
fn a_type_alias_tag_names_its_alias_and_plain_type_or_assert_tags_are_never_parsed() {
    let errors = parse_errors(
        r#"<?php
            /**
             * @phpstan-type Row array<int
             * @type Whatever array<int
             */
            class C {}
            /** @assert (1, 2) == 3 */
            function f() {}
        "#,
    );
    assert_eq!(errors.len(), 1, "got: {errors:?}");
    assert!(
        errors[0].starts_with("PHPDoc tag @phpstan-type Row has invalid value: "),
        "got: {errors:?}"
    );
}

#[test]
fn a_psalm_prefixed_tag_with_an_invalid_value_is_not_reported() {
    let errors = parse_errors(
        r#"<?php
            /** @psalm-param object $x */
            function f($x) {}
        "#,
    );
    assert!(errors.is_empty(), "got: {errors:?}");
}

#[test]
fn an_invalid_tag_contributes_no_type_at_all() {
    // Real PHPStan's `InvalidTagValueNode` is ignored entirely - the
    // recoverable `array` in `array(width, height)` is not a return type,
    // so returning a string isn't a `return.type` mismatch.
    let d = diagnostics_for(
        r#"<?php
            /** @return array(width, height) */
            function f() { return 'not an array'; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "phpDoc.parseError"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.type"),
        "got: {d:?}"
    );
}

#[test]
fn well_formed_doctrine_annotations_are_valid() {
    let errors = parse_errors(
        r#"<?php
            /**
             * @ORM\Entity(repositoryClass="App\Repo")
             * @ORM\Table(name="users", indexes={@ORM\Index(name="idx", columns={"a", "b"})})
             * @Route("/users/{id}", name="user_show", methods={"GET", "POST"}, requirements={"id": "\d+"})
             * @Assert\Choice(choices={1, 2.5, true, null}, strict=true,)
             * @Serializer\Type("array<string>")
             * @Custom(value=Foo::BAR, all=Foo::*, key = 'single')
             * @Empty()
             */
            class User {}
        "#,
    );
    assert!(errors.is_empty(), "got: {errors:?}");
}

#[test]
fn a_parenthesized_free_text_value_on_an_unknown_tag_is_a_doctrine_parse_error() {
    // Piwigo's own `include/minify/src/CSS.php` shape - real PHPStan reads
    // `(image.jpg)` as Doctrine annotation arguments.
    let errors = parse_errors(
        r#"<?php
            /**
             * @url (image.jpg) images will be loaded and merged
             * @internal (do not use this)
             * @see (see below)
             */
            function f() {}
        "#,
    );
    assert_eq!(errors.len(), 3, "got: {errors:?}");
    assert!(
        errors[0].starts_with(
            "PHPDoc tag @url has invalid value ((image.jpg) images will be loaded and merged): "
        ),
        "got: {errors:?}"
    );
    assert!(
        errors.iter().all(|e| e.contains("expected ')'")),
        "got: {errors:?}"
    );
}

#[test]
fn a_parenthesized_value_on_a_tag_with_its_own_parser_is_not_a_doctrine_annotation() {
    let errors = parse_errors(
        r#"<?php
            /**
             * @deprecated (since 2.0, use g() instead)
             * @return (int|string) the result
             */
            function f() { return 1; }
        "#,
    );
    assert!(errors.is_empty(), "got: {errors:?}");
}
