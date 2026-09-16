//! `int<min,max>` range arithmetic (add/subtract/multiply/divide/modulo/power).

use super::helpers::*;

// `strpos()`'s `$offset` parameter (typed `int<0, max>`) must not
// collapse to bare `Int` the moment it's reassigned via `$offset = $pos
// + 1;` inside a loop - `infer_binary`'s `Add`/`Sub` arms need real,
// narrow `int<min,max>` arithmetic, mirroring `vendor/phpstan-src`'s own
// `InitializerExprTypeResolver::integerRangeMath`. See each of
// `mul_int_range`/`div_int_range`/`mod_int_range`/`pow_int_range`/
// `sub_int_range_full`'s own doc comments (`checker/expr.rs`) for the
// exact transcription each is based on.
#[test]
fn adding_a_literal_to_a_bounded_int_range_shifts_both_ends() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, 10> $x */
            function f(int $x): void {
                assertType('int<1, 11>', $x + 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_a_literal_to_an_upper_unbounded_int_range_keeps_it_unbounded() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, max> $x */
            function f(int $x): void {
                assertType('int<1, max>', $x + 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_a_literal_from_a_bounded_int_range_shifts_both_ends() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, 10> $x */
            function f(int $x): void {
                assertType('int<-1, 9>', $x - 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_a_literal_from_an_upper_unbounded_int_range_keeps_it_unbounded() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, max> $x */
            function f(int $x): void {
                assertType('int<-1, max>', $x - 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `PHP_INT_MAX + 1` overflows in real PHP by silently promoting to
// `float`, never crashing - `checked_add`/`checked_sub`'s own doc
// comments (`checker/expr.rs`/`checker/expr/ranges.rs`) collapse such
// a bound to unbounded (`None`) for the same reason `pow_int_range`
// already does, rather than letting plain `i64::+`/`-` panic/wrap.
#[test]
fn adding_past_i64_max_unions_in_float_instead_of_panicking() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, 9223372036854775807> $x */
            function f(int $x): void {
                assertType('int<1, max>|float', $x + 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_past_i64_min_unions_in_float_instead_of_panicking() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<-9223372036854775808, 0> $x */
            function f(int $x): void {
                assertType('int<min, -1>|float', $x - 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_two_int_ranges_with_a_shared_unbounded_end_stays_unbounded() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<0, 10> $x
             * @param int<0, max> $y
             */
            function f(int $x, int $y): void {
                assertType('int<0, max>', $x + $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `Sub` must not only preserve the range for a constant right operand -
// PHPStan's own `Minus` arm has extra branches specifically for when the
// subtracted side is itself an unbounded range. `sub_int_range_full`
// transcribes PHPStan's full range-minus-range formula directly, so a
// genuinely ranged (not single-valued) right operand is handled too,
// not just a constant one.
#[test]
fn subtracting_a_non_constant_int_range_computes_the_real_range() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<0, 10> $x
             * @param int<0, 5> $y
             */
            function f(int $x, int $y): void {
                assertType('int<-5, 10>', $x - $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real PHPStan's own `Minus` arm's remaining asymmetric shape (the
// second block of `sub_int_range_full` can still overwrite `min` again,
// in exactly one sub-case, per its own doc comment): a lower-unbounded
// right operand (`int<min, 8>`) against a fully-bounded left one
// (`int<3, 20>`) - the first block's own `min` computation requires a
// *known* right-side min, so `min` starts `None` there and is only
// really computed in the second block, from `l_min - r_max`.
#[test]
fn subtracting_a_lower_unbounded_range_recomputes_min_in_the_second_block() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<3, 20> $x
             * @param int<min, 8> $y
             */
            function f(int $x, int $y): void {
                assertType('int<-5, max>', $x - $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn multiplying_two_literals_folds_to_the_exact_product() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('15', 5 * 3);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn multiplying_two_bounded_int_ranges_computes_the_real_range() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, 5> $x
             * @param int<2, 4> $y
             */
            function f(int $x, int $y): void {
                assertType('int<2, 20>', $x * $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `mul_int_range`'s own zero-corner special case - without it, an
// unbounded end multiplied against a literal `0` would compute `0 * INF`
// (`NaN` in plain floating point), not the real answer, `0`.
#[test]
fn multiplying_an_unbounded_range_by_zero_is_always_zero() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, max> $x */
            function f(int $x): void {
                assertType('0', $x * 0);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn raising_two_literals_to_a_power_folds_to_the_exact_result() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('8', 2 ** 3);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn modulo_of_two_literals_folds_to_the_exact_remainder() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('1', 7 % 3);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `mod_int_range`'s own magnitude-based fallback (real PHPStan's own
// `getModType`): the result's own bound is keyed off the *divisor*'s
// magnitude, narrowed to non-negative only when the dividend is itself
// provably non-negative.
#[test]
fn modulo_by_a_bounded_int_range_bounds_the_result_by_its_magnitude() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, 5> $x
             * @param int<2, 4> $y
             */
            function f(int $x, int $y): void {
                assertType('int<0, 3>', $x % $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real PHP division isn't guaranteed to itself produce an int (`10 / 4`
// is always exactly `2.5`) - `div_int_range`'s own rounding step
// (`ceil`ing the min, `floor`ing the max independently) can invert an
// otherwise-valid range into an empty one, which must collapse the int
// half away entirely rather than surface as a nonsensical `int<3, 2>`.
#[test]
fn dividing_two_literals_with_no_exact_int_result_is_bare_float() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('float', 10 / 4);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn dividing_two_bounded_int_ranges_computes_a_refined_int_or_float_union() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, 5> $x
             * @param int<2, 4> $y
             */
            function f(int $x, int $y): void {
                assertType('int<1, 2>|float', $x / $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real PHP silently promotes an overflowing int +/-/* to float at
// runtime - Add/Sub/Mul now union Float in when a bound genuinely
// overflows i64, not just when an operand was already unbounded.

#[test]
fn adding_near_php_int_max_unions_in_float_on_overflow() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<9223372036854775800, 9223372036854775807> $x
             */
            function f(int $x): void {
                assertType('int<9223372036854775801, max>|float', $x + 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_near_php_int_min_unions_in_float_on_overflow() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<-9223372036854775808, -9223372036854775800> $x
             */
            function f(int $x): void {
                assertType('int<min, -9223372036854775801>|float', $x - 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn multiplying_two_large_ranges_unions_in_float_on_overflow() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<9223372036854775800, 9223372036854775807> $x
             * @param int<2, 3> $y
             */
            function f(int $x, int $y): void {
                assertType('int|float', $x * $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_two_small_ranges_never_unions_in_float() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, 5> $x
             * @param int<1, 5> $y
             */
            function f(int $x, int $y): void {
                assertType('int<2, 10>', $x + $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_a_genuinely_unbounded_operand_stays_plain_int_not_float() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<1, max> $x
             */
            function f(int $x): void {
                assertType('int<2, max>', $x + 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
