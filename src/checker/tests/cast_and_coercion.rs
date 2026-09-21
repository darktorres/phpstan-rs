//! InvalidCastRule (`cast.int`/`cast.double`/`cast.string`), EchoRule
//! (`echo.nonString`), and PrintRule (`print.nonString`) - a
//! cast/echo/print whose operand's own type PHP itself cannot legally
//! coerce that way.

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

// `type_to_integer` must handle `Type::Intersection`, not just a bare
// `Type::Object`/`Type::Union` falling through to the permissive
// `_ => Some(Type::Int)` catch-all - `(int) $x` on an intersection of
// two non-castable objects (a real, common shape:
// `narrow_instanceof_true`'s own deliberate design intersects an
// unrelated-looking `instanceof` target with the original type) must
// still be flagged.
#[test]
fn casting_an_intersection_of_two_non_castable_objects_to_int_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            class Foo {}
            function test(View $view): int {
                if ($view instanceof Foo) {
                    return (int) $view;
                }
                return 0;
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
fn printing_a_plain_object_with_no_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): void {
                print $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "print.nonString");
    // Real PHPStan's own PrintRule message has no `#%d` position number
    // at all (unlike echo.nonString) - `print` only ever has one operand.
    assert!(!d[0].message.contains('#'), "got: {d:?}");
    assert!(
        d[0].message.starts_with("Parameter Foo of print"),
        "got: {d:?}"
    );
}

#[test]
fn printing_ordinary_scalars_and_a_stringable_object_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(int $i, Foo $f): void {
                print $i;
                print $f;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn print_always_evaluates_to_the_literal_int_one() {
    let d = diagnostics_for(
        r#"<?php
            function needsOne(int $x): void {}
            function test(): void {
                needsOne(print "hi");
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

// `(unset)` cast - `UnsetCastRule` (`cast.unset`): removed as of PHP 8.0.

#[test]
fn an_unset_cast_on_php_8_or_later_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test($x) {
                $y = (unset) $x;
            }
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(d.iter().any(|x| x.identifier == "cast.unset"), "got: {d:?}");
}

#[test]
fn an_unset_cast_below_php_8_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test($x) {
                $y = (unset) $x;
            }
        "#,
        |r| r.php_version = Some((7, 4)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.unset"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_unset_cast_with_no_configured_php_version_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test($x) {
                $y = (unset) $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.unset"),
        "unexpected diagnostics: {d:?}"
    );
}

// `(void)` cast - `VoidCastRule` (`cast.voidNotSupported`/`cast.void`):
// needs PHP 8.5+, and is only ever legal as its own top-level statement.

#[test]
fn a_void_cast_as_its_own_statement_below_php_8_5_is_flagged_as_unsupported_only() {
    let d = diagnostics_for_with(
        r#"<?php
            function test() {
                (void) foo();
            }
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "cast.voidNotSupported"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.void"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_void_cast_as_its_own_statement_on_php_8_5_or_later_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test() {
                (void) foo();
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "cast.voidNotSupported" || x.identifier == "cast.void"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_void_cast_nested_inside_an_expression_is_flagged_even_on_php_8_5() {
    let d = diagnostics_for_with(
        r#"<?php
            function test() {
                $x = (void) foo();
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(d.iter().any(|x| x.identifier == "cast.void"), "got: {d:?}");
    assert!(
        !d.iter().any(|x| x.identifier == "cast.voidNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_void_cast_with_no_configured_php_version_is_never_flagged_for_version_support() {
    let d = diagnostics_for(
        r#"<?php
            function test() {
                (void) foo();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.voidNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

// Non-canonical cast spellings - `DeprecatedCastRule` (`cast.deprecated`):
// `(integer)`/`(boolean)`/`(double)`/`(binary)` are deprecated as of PHP
// 8.5; `(real)` is a faithful, unflagged gap (see `ExprKind::Cast`'s doc
// comment - real PHPStan's own rule never checks it either).

#[test]
fn each_non_canonical_cast_spelling_on_php_8_5_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test($x) {
                $a = (integer) $x;
                $b = (boolean) $x;
                $c = (double) $x;
                $d = (binary) $x;
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    let deprecated: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "cast.deprecated")
        .collect();
    assert_eq!(deprecated.len(), 4, "got: {d:?}");
}

#[test]
fn real_is_never_flagged_as_a_deprecated_cast_spelling() {
    let d = diagnostics_for_with(
        r#"<?php
            function test($x) {
                $a = (real) $x;
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn canonical_cast_spellings_are_never_flagged_as_deprecated() {
    let d = diagnostics_for_with(
        r#"<?php
            function test($x) {
                $a = (int) $x;
                $b = (bool) $x;
                $c = (float) $x;
                $d = (string) $x;
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_canonical_cast_spelling_below_php_8_5_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test($x) {
                $a = (integer) $x;
            }
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_canonical_cast_spelling_with_no_configured_php_version_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test($x) {
                $a = (integer) $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "cast.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}
