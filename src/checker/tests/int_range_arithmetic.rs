//! `int<min,max>` range arithmetic (add/subtract/multiply/divide/modulo/power).

use super::helpers::*;

// Real corpus false positive found while corpus-verifying the family
// above: `strpos()`'s own `$offset` parameter (typed `int<0, max>`)
// collapsed to bare `Int` the moment it was reassigned via `$offset =
// $pos + 1;` inside a loop, surfacing only once the `While`-condition
// convergence fix made `strpos()`'s own later, real re-check of
// `$offset` visible at all (`Tools\p58\assign.php`). `infer_binary`'s
// `Add`/`Sub` arms gained real, narrow `int<min,max>` arithmetic -
// confirmed directly against `vendor/phpstan-src`'s own
// `InitializerExprTypeResolver::integerRangeMath`. `Mul`/`Div`/`Mod`/
// `Pow`'s own full range-arithmetic surface, and `Sub`'s own full
// range-minus-range case, were extended to match later in the same
// session (see the dedicated tests further below) - see each of
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

// Re-examining Phase 15's own disclosed simplification ("`Sub` only
// preserves the range for a *constant* right operand... real PHPStan's
// own `Minus` arm has extra branches specifically for when the
// subtracted side is itself an unbounded range") per the user's own
// explicit "do all those narrow [items], our corpus not having them is
// not a reason not to do them" - `sub_int_range_full` now transcribes
// real PHPStan's own full range-minus-range formula directly, so a
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
