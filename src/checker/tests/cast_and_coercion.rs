//! Phase 51: InvalidCastRule (`cast.int`/`cast.double`/`cast.string`),
//! EchoRule (`echo.nonString`), and PrintRule (`print.nonString`) - a
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

// Phase 51: IntegerRangeType completion - abs()/toAbsoluteNumber() and
// `~`/toBitwiseNotType().

#[test]
fn abs_of_a_negative_range_computes_the_real_reflected_range() {
    let d = diagnostics_for(
        r#"<?php
            function needsExact(int $x): void {}
            /** @param int<-10, -1> $x */
            function test(int $x): void {
                $y = abs($x);
                /** @var int<1, 10> $y */
                needsExact($y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    // A target range one narrower than the real result must still be
    // rejected - proves the computed range is exact, not just "some
    // unbounded positive int".
    let d = diagnostics_for(
        r#"<?php
            /** @param int<1, 9> $x */
            function needsNarrower(int $x): void {}
            /** @param int<-10, -1> $x */
            function test(int $x): void {
                needsNarrower(abs($x));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<1, 10>"), "got: {d:?}");
}

#[test]
fn abs_of_an_already_non_negative_range_is_unchanged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 9> $x */
            function needsNarrower(int $x): void {}
            /** @param int<0, 10> $x */
            function test(int $x): void {
                needsNarrower(abs($x));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<0, 10>"), "got: {d:?}");
}

#[test]
fn abs_of_a_range_straddling_zero_takes_the_larger_magnitude_bound() {
    // abs(int<-3, 10>) is int<0, 10> - 10 is the larger magnitude, not 3.
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 9> $x */
            function needsNarrower(int $x): void {}
            /** @param int<-3, 10> $x */
            function test(int $x): void {
                needsNarrower(abs($x));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<0, 10>"), "got: {d:?}");
}

#[test]
fn abs_of_a_plain_int_literal_computes_its_own_absolute_value() {
    let d = diagnostics_for(
        r#"<?php
            /** @param 4 $x */
            function needsFour(int $x): void {}
            function test(): void {
                needsFour(abs(-5));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("expects 4, 5 given"), "got: {d:?}");
}

#[test]
fn bitwise_not_of_an_int_range_reverses_and_complements_the_bounds() {
    // ~int<a, b> = int<~b, ~a> (order reverses).
    let d = diagnostics_for(
        r#"<?php
            /** @param int<-11, -1> $x */
            function needsNarrower(int $x): void {}
            /** @param int<0, 10> $x */
            function test(int $x): void {
                needsNarrower(~$x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let d = diagnostics_for(
        r#"<?php
            /** @param int<-11, -2> $x */
            function needsNarrower(int $x): void {}
            /** @param int<0, 10> $x */
            function test(int $x): void {
                needsNarrower(~$x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<-11, -1>"), "got: {d:?}");
}

#[test]
fn bitwise_not_of_a_plain_unbounded_int_or_string_is_unchanged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $i, string $s): void {
                $a = ~$i;
                $b = ~$s;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// IntegerRangeType-consumer extensions: strlen()/count()/sizeof()/
// random_int()/rand()/mt_rand() - real PHPStan's own `Strlen`/`Count`/
// `RandomInt` `FunctionReturnTypeExtension`s, narrowed to the always-
// safe, zero-risk slice of each (see builtin_calls.rs's own doc
// comments on each arm for the exact scoping and why).

#[test]
fn strlen_is_never_negative() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 5> $x */
            function needsNarrow(int $x): void {}
            function test(string $s): void {
                needsNarrow(strlen($s));
            }
        "#,
    );
    // strlen()'s own real range is unbounded above - a target upper
    // bound narrower than that must still be rejected, proving this
    // isn't silently widened all the way to plain Mixed/int.
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<0, max>"), "got: {d:?}");
}

#[test]
fn strlen_of_anything_is_accepted_by_a_non_negative_target() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, max> $x */
            function needsNonNegative(int $x): void {}
            function test(string $s): void {
                needsNonNegative(strlen($s));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn count_of_a_known_literal_array_shape_is_exact() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 2> $x */
            function needsNarrower(int $x): void {}
            function test(): void {
                needsNarrower(count(['a', 'b', 'c']));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains(" 3 "), "got: {d:?}");
}

#[test]
fn count_of_an_unshaped_array_is_never_negative() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, max> $x */
            function needsNonNegative(int $x): void {}
            function test(array $a): void {
                needsNonNegative(count($a));
                needsNonNegative(sizeof($a));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn random_int_range_is_bounded_by_its_own_literal_arguments() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<1, 10> $x */
            function needsExact(int $x): void {}
            function test(): void {
                needsExact(random_int(1, 10));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let d = diagnostics_for(
        r#"<?php
            /** @param int<1, 9> $x */
            function needsNarrower(int $x): void {}
            function test(): void {
                needsNarrower(random_int(1, 10));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<1, 10>"), "got: {d:?}");
}

#[test]
fn rand_and_mt_rand_get_the_same_range_treatment_as_random_int() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<1, 10> $x */
            function needsExact(int $x): void {}
            function test(): void {
                needsExact(rand(1, 10));
                needsExact(mt_rand(1, 10));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn random_int_with_a_non_literal_argument_degrades_that_side_to_unbounded() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 100> $x */
            function needsBounded(int $x): void {}
            function test(int $min): void {
                needsBounded(random_int($min, 10));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<min, 10>"), "got: {d:?}");
}

#[test]
fn hrtime_false_and_no_args_return_the_two_element_list_shape() {
    let d = diagnostics_for(
        r#"<?php
            function needsSeconds(int $s): void {}
            function needsNanoseconds(int $ns): void {}
            function test(): void {
                [$s1, $ns1] = hrtime();
                [$s2, $ns2] = hrtime(false);
                needsSeconds($s1);
                needsSeconds($s2);
                needsNanoseconds($ns1);
                needsNanoseconds($ns2);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn hrtime_true_returns_a_number_not_the_list_shape() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<1, max>|float $x */
            function needsNumber(int|float $x): void {}
            function test(): void {
                needsNumber(hrtime(true));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn hrtime_nanoseconds_are_bounded_below_one_billion() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 999999998> $ns */
            function needsNarrower(int $ns): void {}
            function test(): void {
                [$s, $ns] = hrtime();
                needsNarrower($ns);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<0, 999999999>"), "got: {d:?}");
}

// InvalidPartOfEncapsedStringRule (`encapsedStringPart.nonString`) - an
// interpolated `"{$x}"` part implicitly casts `$x` to string, exactly
// like `(string)`/`echo`/`print`.

#[test]
fn interpolating_a_plain_object_with_no_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): string {
                return "value: {$f}";
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "encapsedStringPart.nonString");
}

#[test]
fn interpolating_an_array_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $a */
            function test(array $a): string {
                return "value: {$a}";
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "encapsedStringPart.nonString");
}

#[test]
fn interpolating_ordinary_scalars_and_a_stringable_object_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(int $i, string $s, Foo $f): string {
                return "{$i} {$s} {$f}";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
