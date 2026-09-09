//! Phase 52: `InvalidBinaryOperationRule` (`binaryOp.invalid`/
//! `assignOp.invalid`), `InvalidComparisonOperationRule` (`*.invalid`),
//! `InvalidUnaryOperationRule` (`unaryOp.invalid`), and
//! `InvalidIncDecOperationRule` (`*.expr`/`*.type`) - an operand-validity
//! matrix, not a full port of real PHPStan's own operator type resolver.
//! See `src/checker/operators.rs`'s own module doc for the scope
//! decisions this deliberately makes.

use super::helpers::*;

#[test]
fn adding_an_array_and_a_plain_object_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param int[] $xs */
            function test(array $xs, Foo $f): void {
                $xs + $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "binaryOp.invalid");
    assert!(d[0].message.contains("\"+\""), "got: {d:?}");
}

#[test]
fn adding_two_arrays_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $a
             *  @param int[] $b */
            function test(array $a, array $b): void {
                $a + $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_an_array_and_mixed_is_never_flagged_dont_guess() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function test(array $xs, mixed $m): void {
                $xs + $m;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_two_arrays_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $a
             *  @param int[] $b */
            function test(array $a, array $b): void {
                $a - $b;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "binaryOp.invalid");
}

#[test]
fn adding_a_plain_string_and_an_int_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $s): void {
                1 + $s;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "binaryOp.invalid");
}

#[test]
fn adding_a_numeric_string_and_an_int_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function test(string $s): void {
                1 + $s;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn compound_assign_add_on_an_array_and_a_non_array_is_flagged_as_assign_op() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function test(array $xs): void {
                $xs += "foo";
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "assignOp.invalid");
    assert!(d[0].message.contains("\"+=\""), "got: {d:?}");
}

#[test]
fn concatenating_a_string_and_an_object_without_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(string $s, Foo $f): void {
                $s . $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "binaryOp.invalid");
}

#[test]
fn concatenating_a_string_and_a_stringable_object_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(string $s, Foo $f): void {
                $s . $f;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bitwise_and_between_a_plain_string_and_an_int_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $s): void {
                $s & 5;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "binaryOp.invalid");
}

#[test]
fn bitwise_and_between_two_plain_strings_is_never_flagged_byte_op() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $a, string $b): void {
                $a & $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bitwise_and_between_two_ints_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $a, int $b): void {
                $a & $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A real corpus check (`Composer\Pcre\Preg::match`, `Amp\Websocket\
// Parser\Rfc6455Parser::parse`) found `mixed | <flags>` and `mixed &
// '...'` wrongly flagged - `Mixed` on either side must skip the whole
// check, not just each operand's own individual legality (a `Mixed`
// might satisfy either of `&`/`|`/`^`'s two legal shapes at runtime,
// but can't be proven to satisfy the *same* one as its other operand).
#[test]
fn bitwise_or_with_a_mixed_operand_is_never_flagged_dont_guess() {
    let d = diagnostics_for(
        r#"<?php
            function test(mixed $flags): void {
                $flags | 'x';
                'x' | $flags;
                $flags & 1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A real corpus check (`Composer\Pcre\Preg::match`'s own `@phpstan-type
// int-mask<...>` PHPDoc type alias, which this project surfaces as a
// `Type::Object` with no matching registry entry) found an unresolvable
// class-shaped type wrongly treated as confidently non-numeric, the
// same "don't guess" gap `type_to_string` already handles for casts.
#[test]
fn bitwise_or_with_an_unresolvable_class_shaped_operand_is_never_flagged_dont_guess() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $mask): void {
                /** @var \Some\Unresolvable\IntMask<1> $mask */
                $mask | 1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn arithmetic_between_two_bcmath_number_instances_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(\BcMath\Number $a, \BcMath\Number $b): void {
                $a + $b;
                $a - $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn shifting_two_plain_strings_is_flagged_no_byte_op_exception() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $a, string $b): void {
                $a << $b;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "binaryOp.invalid");
}

#[test]
fn bitwise_not_of_an_array_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function test(array $xs): void {
                ~$xs;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "unaryOp.invalid");
    assert!(d[0].message.contains("\"~\""), "got: {d:?}");
}

#[test]
fn bitwise_not_of_a_plain_object_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): void {
                ~$f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "unaryOp.invalid");
}

#[test]
fn bitwise_not_of_a_string_int_or_float_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $s, int $i, float $f): void {
                ~$s;
                ~$i;
                ~$f;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A real corpus check found `~$mixed` wrongly flagged - `is_bitwise_
// not_operand`'s own catch-all defaulted to illegal instead of "don't
// guess."
#[test]
fn bitwise_not_of_mixed_is_never_flagged_dont_guess() {
    let d = diagnostics_for(
        r#"<?php
            function test(mixed $m): void {
                ~$m;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn unary_minus_on_a_plain_object_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): void {
                -$f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "unaryOp.invalid");
    assert!(d[0].message.contains("\"-\""), "got: {d:?}");
}

#[test]
fn unary_minus_on_a_gmp_instance_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(\GMP $g): void {
                -$g;
                +$g;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real corpus find: unary `-` on an int literal used to leave the
// literal's own *value* unnegated (only `+`'s numeric-string coercion was
// actually implemented; `-` fell through to the same "return unchanged"
// branch) - `-5` inferred as `IntLiteral(5)`, not `IntLiteral(-5)`. Only
// a consumer that cares about the exact *value* (not just "is this an
// int") can tell the difference - `checker::expr::loose_compare_
// literals`'s own constant-folding does: with the bug, `-5 == 5` would
// wrongly fold to `equal.alwaysTrue` (both sides silently the same
// literal); correctly negated, it's `equal.alwaysFalse`.
#[test]
fn unary_minus_negates_an_int_literal_s_own_value() {
    let d = diagnostics_for(
        r#"<?php
            function f(): bool {
                return -5 == 5;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "equal.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn comparing_an_int_to_a_possibly_null_array_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[]|null $xs */
            function test(int $i, ?array $xs): void {
                $i < $xs;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "smaller.invalid");
}

#[test]
fn comparing_an_int_to_a_possibly_null_object_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(float $x, ?Foo $f): void {
                $x > $f;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "greater.invalid");
}

// Real gap found and fixed this session: `is_array_or_null` matched
// `Type::Array`/`Type::ArrayShape` directly with no `Type::Intersection`
// case, so a `list<T>`/`non-empty-array<T>`-typed value (both represented
// as `Intersection([Array(_), Accessory(_)])`, Phase 14) fell to the
// permissive `_ => false` catch-all - meaning comparing a real `list<T>`
// against a number was never flagged, unlike a plain `array`.
#[test]
fn comparing_an_int_to_a_possibly_null_list_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int>|null $xs */
            function test(int $i, ?array $xs): void {
                $i < $xs;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "smaller.invalid");
}

#[test]
fn comparing_two_ints_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $a, int $b): void {
                $a < $b;
                $a <=> $b;
                $a === $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A real corpus check (`NumberComparator::assertEquals`) found a
// `BcMath\Number` compared against a plain `int` wrongly flagged -
// `is_strictly_number` now counts a number-castable object as a number
// in its own right, matching real PHPStan's own `SimpleXMLElement`/
// `GMP` exception for this same rule.
#[test]
fn comparing_a_number_castable_object_to_an_int_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(\GMP $g, int $i): void {
                $g < $i;
                $g > $i;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn comparing_two_arrays_is_never_flagged_neither_side_is_a_number() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $a
             *  @param int[] $b */
            function test(array $a, array $b): void {
                $a == $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn incrementing_a_non_variable_target_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function five(): int { return 5; }
            function test(): void {
                five()++;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "postInc.expr");
    assert!(d[0].message.contains("non-variable"), "got: {d:?}");
}

#[test]
fn incrementing_a_plain_object_is_flagged_as_a_type_error() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $f): void {
                $f++;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "postInc.type");
}

#[test]
fn incrementing_an_array_element_is_flagged_when_the_element_is_itself_an_array() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<string, int[]> $rows
             */
            function test(array $rows): void {
                $rows['x']++;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "postInc.type");
}

#[test]
fn incrementing_an_int_a_float_or_a_string_variable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $i, float $f, string $s): void {
                $i++;
                --$f;
                $s++;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real corpus find: `++`/`--` on an `IntLiteral`-typed variable used to
// leave the literal's own *value* completely unchanged (returning the
// operand's own type as-is, the same "value never applied" bug class
// unary `-`'s own fix (`negate_numeric_literal`) already found). Only a
// consumer that cares about the exact value can tell the difference -
// `checker::expr::loose_compare_literals`'s own constant-folding does:
// with the bug, `0++ == 1` (well-formed as `$n = 0; $n++; $n == 1`)
// would wrongly fold to `equal.alwaysFalse` (`$n` silently still `0`).
#[test]
fn incrementing_an_int_literal_advances_its_own_value() {
    let d = diagnostics_for(
        r#"<?php
            function f(): bool {
                $n = 0;
                $n++;
                return $n == 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "equal.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn decrementing_an_int_literal_reduces_its_own_value() {
    let d = diagnostics_for(
        r#"<?php
            function f(): bool {
                $n = 5;
                $n--;
                return $n == 4;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "equal.alwaysTrue"),
        "got: {d:?}"
    );
}

// Phase 52 follow-up: division/modulo by a literal zero.

#[test]
fn dividing_by_a_literal_zero_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): int|float {
                return $x / 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "division.byZero");
}

#[test]
fn modulo_by_a_literal_zero_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): int {
                return $x % 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "modulo.byZero");
}

#[test]
fn dividing_by_a_literal_zero_point_zero_float_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(float $x): float {
                return $x / 0.0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "division.byZero");
}

#[test]
fn dividing_by_a_non_zero_literal_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): int {
                return $x / 2;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "division.byZero"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn dividing_by_an_ordinary_int_variable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x, int $y): int {
                return $x / $y;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "division.byZero"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn compound_assign_division_by_a_literal_zero_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): void {
                $x /= 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "division.byZero");
}
