//! `IncompatiblePhpDocTypeRule`: a function/method's `@param`/
//! `@param-out`/`@return` tags against its own parameters and native hints.

use super::helpers::*;

fn messages(src: &str, identifier: &str) -> Vec<String> {
    diagnostics_for(src)
        .into_iter()
        .filter(|d| d.identifier == identifier)
        .map(|d| d.message)
        .collect()
}

#[test]
fn a_param_tag_disjoint_from_its_native_hint_is_incompatible() {
    let m = messages(
        r#"<?php
            /** @param string $x */
            function f(int $x): void {}
        "#,
        "parameter.phpDocType",
    );
    assert_eq!(
        m,
        [
            "PHPDoc tag @param for parameter $x with type string is incompatible with native type int."
        ]
    );
}

#[test]
fn a_param_tag_wider_than_its_native_hint_is_not_a_subtype() {
    let m = messages(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            /** @param Shape|null $s */
            function f(Circle $s): void {}
        "#,
        "parameter.phpDocType",
    );
    assert_eq!(
        m,
        [
            "PHPDoc tag @param for parameter $s with type Shape|null is not subtype of native type Circle."
        ]
    );
}

#[test]
fn a_refining_tag_is_never_reported() {
    let m = messages(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            /**
             * @param list<int> $ids
             * @param non-empty-string $name
             * @param Circle|null $s
             * @param int ...$rest
             * @return positive-int
             */
            function f(array $ids, string $name, ?Shape $s = null, int ...$rest): int { return 1; }
        "#,
        "parameter.phpDocType",
    );
    assert!(m.is_empty(), "got: {m:?}");
}

#[test]
fn an_int_tag_on_a_float_hint_is_incompatible_and_an_absorbed_union_displays_its_wider_member() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /**
             * @param int $x
             * @param object|Foo $o
             * @return string
             */
            function f(float $x, Foo $o): void {}
        "#,
    );
    let m: Vec<&str> = d
        .iter()
        .filter(|d| d.identifier.ends_with(".phpDocType"))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        m,
        [
            "PHPDoc tag @param for parameter $x with type int is incompatible with native type float.",
            "PHPDoc tag @param for parameter $o with type object is not subtype of native type Foo.",
            "PHPDoc tag @return with type string is incompatible with native type void.",
        ]
    );
}

#[test]
fn a_template_typed_param_without_a_fitting_bound_is_not_a_subtype() {
    let m = messages(
        r#"<?php
            /**
             * @template T
             * @param T $x
             */
            function f(int $x): void {}
        "#,
        "parameter.phpDocType",
    );
    assert_eq!(
        m,
        ["PHPDoc tag @param for parameter $x with type T is not subtype of native type int."]
    );
}

#[test]
fn a_param_tag_naming_no_parameter_is_reported() {
    let m = messages(
        r#"<?php
            /**
             * @param int $a
             * @param int $missing
             * @param-out int $gone
             */
            function f(int $a): void {}
        "#,
        "parameter.notFound",
    );
    assert_eq!(
        m,
        [
            "PHPDoc tag @param references unknown parameter: $missing",
            "PHPDoc tag @param-out references unknown parameter: $gone",
        ]
    );
}

#[test]
fn a_param_out_tag_on_a_by_value_parameter_is_reported() {
    let m = messages(
        r#"<?php
            /**
             * @param-out int $a
             * @param-out int $b
             */
            function f($a, &$b): void {}
        "#,
        "parameter.notByRef",
    );
    assert_eq!(
        m,
        ["Parameter $a for PHPDoc tag @param-out is not passed by reference."]
    );
}

#[test]
fn a_property_var_tag_is_checked_against_its_native_type() {
    let m = messages(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            class Holder {
                /** @var string */
                public int $count = 0;
                /** @var Shape */
                public Circle $circle;
                /** @var list<int> */
                public array $ids = [];
                public function __construct(
                    /** @var non-empty-string */
                    public string $name,
                    /** @var int */
                    public string $label,
                ) {}
            }
        "#,
        "property.phpDocType",
    );
    assert_eq!(
        m,
        [
            "PHPDoc tag @var for property Holder::$count with type string is incompatible with native type int.",
            "PHPDoc tag @var for property Holder::$circle with type Shape is not subtype of native type Circle.",
            "PHPDoc type for property Holder::$label with type int is incompatible with native type string.",
        ]
    );
}

#[test]
fn a_property_doc_member_the_native_type_excludes_does_not_reach_its_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Holder {
                /** @var int|string */
                public int $count = 0;
                public function f(): void { assertType('int', $this->count); }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpstanRs.assertType"),
        "got: {d:?}"
    );
}
