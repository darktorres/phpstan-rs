//! Phase 51: InvalidCastRule (`cast.int`/`cast.double`/`cast.string`) and
//! EchoRule (`echo.nonString`) - a cast/echo whose operand's own type PHP
//! itself cannot legally coerce that way.

use super::helpers::*;

#[test]
fn casting_a_plain_object_with_no_tostring_to_string_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): string {
                return (string) $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "cast.string");
    assert!(
        d[0].message.contains("Cannot cast Foo to string"),
        "got: {d:?}"
    );
}

#[test]
fn casting_an_object_with_a_real_tostring_to_string_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(Foo $f): string {
                return (string) $f;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn casting_an_array_to_string_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function test(array $xs): string {
                return (string) $xs;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "cast.string");
}

#[test]
fn casting_a_plain_object_with_no_tostring_to_int_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): int {
                return (int) $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "cast.int");
}

#[test]
fn casting_an_array_to_int_or_float_is_not_flagged() {
    // Real PHP allows this (0 or 1, depending on emptiness) - only
    // (string) on an array is illegal.
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function test(array $xs): void {
                $i = (int) $xs;
                $f = (float) $xs;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn ordinary_scalar_casts_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $i, float $f, string $s, bool $b, ?string $n): void {
                $a = (int) $s;
                $c = (float) $i;
                $e = (string) $i;
                $g = (string) $f;
                $h = (int) $b;
                $j = (string) $n;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn casting_a_mixed_typed_value_is_never_flagged_dont_guess() {
    let d = diagnostics_for(
        r#"<?php
            function test(mixed $m): void {
                $s = (string) $m;
                $i = (int) $m;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn casting_the_native_object_pseudo_type_to_string_is_not_flagged() {
    // `object` is a real sentinel (no class of its own, see
    // registry::ClassInfo's own doc comment) - no real data to check a
    // __toString() against, so don't guess, matching this project's
    // leniency everywhere else (e.g. checker::generics::
    // is_type_fully_known).
    let d = diagnostics_for(
        r#"<?php
            function test(object $x): string {
                return (string) $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_union_mixing_a_stringable_and_a_non_stringable_object_is_not_flagged() {
    // No tri-state "possibly invalid" - only confidently, entirely
    // illegal is flagged.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            class Bar {}
            function test(Foo|Bar $x): string {
                return (string) $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_union_of_two_non_stringable_objects_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            class Bar {}
            function test(Foo|Bar $x): string {
                return (string) $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "cast.string");
}

#[test]
fn echoing_a_plain_object_with_no_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): void {
                echo $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "echo.nonString");
    assert!(d[0].message.contains("Parameter #1"), "got: {d:?}");
}

#[test]
fn echoing_the_second_argument_reports_its_own_one_based_position() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): void {
                echo "ok", $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("Parameter #2"), "got: {d:?}");
}

#[test]
fn echoing_ordinary_scalars_and_a_stringable_object_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(int $i, string $s, Foo $f): void {
                echo $i, $s, $f;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn casting_a_resource_to_int_or_float_is_not_flagged() {
    // A real, common PHP idiom (a resource's own unique id) - unlike an
    // arbitrary object, a resource is always legally int/float-castable.
    let d = diagnostics_for(
        r#"<?php
            function test(resource $r): void {
                $i = (int) $r;
                $f = (float) $r;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn casting_a_resource_to_string_is_flagged() {
    // Matches real PHPStan's own dedicated ResourceType::toString()
    // (always ErrorType), even though real PHP itself produces a
    // "Resource id #N" string with no warning.
    let d = diagnostics_for(
        r#"<?php
            function test(resource $r): string {
                return (string) $r;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "cast.string");
}

#[test]
fn casting_a_gmp_instance_to_string_int_and_float_is_never_flagged() {
    // Real PHP built-in extension class with native operator/coercion
    // overloads, not a __toString() declaration - real PHPStan
    // special-cases it the same way (ObjectType::toInteger/toFloat/
    // toString).
    let d = diagnostics_for(
        r#"<?php
            function test(\GMP $g): void {
                $s = (string) $g;
                $i = (int) $g;
                $f = (float) $g;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
