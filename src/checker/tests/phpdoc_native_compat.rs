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

#[test]
fn unresolvable_param_and_return_types_are_reported() {
    let m = messages(
        r#"<?php
            namespace App;
            class Known { const A = 1; }
            /**
             * @param string[optional] $a
             * @param Closure(int): string $b
             * @param Known::MISSING $c
             * @param self $d
             * @param int<5, 1> $e
             * @param int&string $f
             * @param array<int, string, bool> $g
             * @param non-empty-foo $h
             * @param-out Unknown::X $i
             * @return list<int, string>
             */
            function f($a, $b, $c, $d, $e, $f, $g, $h, &$i) { return []; }
        "#,
        "parameter.unresolvableType",
    );
    assert_eq!(
        m,
        [
            "PHPDoc tag @param for parameter $a contains unresolvable type.",
            "PHPDoc tag @param for parameter $b contains unresolvable type.",
            "PHPDoc tag @param for parameter $c contains unresolvable type.",
            "PHPDoc tag @param for parameter $d contains unresolvable type.",
            "PHPDoc tag @param for parameter $e contains unresolvable type.",
            "PHPDoc tag @param for parameter $f contains unresolvable type.",
            "PHPDoc tag @param for parameter $g contains unresolvable type.",
            "PHPDoc tag @param for parameter $h contains unresolvable type.",
            "PHPDoc tag @param-out for parameter $i contains unresolvable type.",
        ]
    );
    let r = messages(
        r#"<?php
            /** @return list<int, string> */
            function g(): array { return []; }
        "#,
        "return.unresolvableType",
    );
    assert_eq!(r, ["PHPDoc tag @return contains unresolvable type."]);
}

#[test]
fn resolvable_types_are_never_reported_unresolvable() {
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            use Closure;
            enum Suit: string { case Hearts = 'h'; case Spades = 's'; }
            /** @phpstan-type Element array{type: string} */
            class Known {
                const A_ONE = 1;
                const A_TWO = 2;
                /**
                 * @param array{a: int}['a'] $a
                 * @param Closure(int): string $b
                 * @param self::A_* $c
                 * @param Suit::Hearts $d
                 * @param string[] $e
                 * @param int<1, 5> $f
                 * @param non-empty-array<int, string> $g
                 * @param list<int>&non-empty-array $h
                 * @param \Closure(int): void $i
                 * @param static $j
                 * @param Known::class $k
                 * @param closed-resource|open-resource $l
                 * @param \OCI-Lob $m
                 * @param Element&array{type: 'a'} $n
                 * @return array<int, string>
                 */
                public function f($a, $b, $c, $d, $e, $f, $g, $h, $i, $j, $k, $l, $m, $n) { return []; }
            }
        "#,
    );
    let bad: Vec<_> = d
        .iter()
        .filter(|x| x.identifier.ends_with(".unresolvableType"))
        .collect();
    assert!(bad.is_empty(), "got: {bad:?}");
}

#[test]
fn an_unresolvable_property_type_is_reported() {
    let m = messages(
        r#"<?php
            class Holder {
                /** @var string[optional] */
                public string $a = '';
                public function __construct(
                    /** @var int<9, 1> */
                    public int $b,
                ) {}
            }
        "#,
        "property.unresolvableType",
    );
    assert_eq!(
        m,
        [
            "PHPDoc tag @var for property Holder::$a contains unresolvable type.",
            "PHPDoc type for property Holder::$b contains unresolvable type.",
        ]
    );
}

#[test]
fn a_closed_resource_is_not_a_resource_to_is_resource() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Handle {
                /** @var closed-resource|resource|null */
                private mixed $stream = null;
                public function close(): void {
                    if (is_resource($this->stream)) {
                        assertType('resource', $this->stream);
                    }
                }
                /** @param closed-resource $c */
                public function dead($c): void {
                    if (is_resource($c)) {}
                }
            }
        "#,
    );
    let ids: Vec<(&str, &str)> = d
        .iter()
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect();
    assert_eq!(
        ids,
        [(
            "function.impossibleType",
            "Call to function is_resource() with closed-resource will always evaluate to false."
        )],
        "got: {d:?}"
    );
}

#[test]
fn an_unresolvable_or_underspecified_asserted_type_is_reported() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-assert string[optional] $x */
            function a($x): void {}
            /** @phpstan-assert-if-true array $x */
            function b($x): bool { return true; }
        "#,
    );
    let m: Vec<(&str, &str)> = d
        .iter()
        .filter(|x| {
            x.identifier.starts_with("assert.") || x.identifier == "missingType.iterableValue"
        })
        .filter(|x| x.message.contains("PHPDoc tag @phpstan-assert"))
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect();
    assert_eq!(
        m,
        [
            (
                "assert.unresolvableType",
                "PHPDoc tag @phpstan-assert for $x contains unresolvable type."
            ),
            (
                "missingType.iterableValue",
                "PHPDoc tag @phpstan-assert-if-true for $x has no value type specified in iterable type array."
            ),
        ]
    );
}

#[test]
fn an_unresolvable_inline_var_tag_is_reported_on_the_comment_line() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /**
                 * @var string[optional] $x
                 */
                $x = g();
                /** @var int<3, 1> */
                $y = g();
                /** @var list<int> $z */
                $z = g();
            }
            function g(): mixed { return 1; }
        "#,
    );
    let m: Vec<(u32, &str)> = d
        .iter()
        .filter(|x| x.identifier == "varTag.unresolvableType")
        .map(|x| (x.span.start.line, x.message.as_str()))
        .collect();
    assert_eq!(
        m,
        [
            (
                3,
                "PHPDoc tag @var for variable $x contains unresolvable type."
            ),
            (7, "PHPDoc tag @var contains unresolvable type."),
        ]
    );
}

#[test]
fn a_second_declaration_of_a_function_is_checked_against_its_own_phpdoc() {
    let d = all_diagnostics_for(
        r#"<?php
            if (PHP_VERSION_ID > 80000) {
                function twice($a) { return 1; }
            } else {
                /** @param array $a */
                function twice($a) { return 1; }
            }
        "#,
    );
    let m: Vec<&str> = d
        .iter()
        .filter(|x| x.identifier == "missingType.iterableValue")
        .map(|x| x.message.as_str())
        .collect();
    assert_eq!(
        m,
        ["Function twice() has parameter $a with no value type specified in iterable type array."]
    );
}
