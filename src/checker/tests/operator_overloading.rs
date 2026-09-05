//! Phase 81: `GMP`/`BcMath\Number` operator-overloading return-type
//! precision (`GmpOperatorTypeSpecifyingExtension`,
//! `BcMathNumberOperatorTypeSpecifyingExtension`, and their unary
//! counterparts) - distinct from `operator_validity.rs`'s own Phase 52
//! operand-*legality* matrix, which already recognizes these two classes
//! as legal operands but doesn't refine the *result* type.

use super::helpers::*;

#[test]
fn gmp_plus_gmp_returns_gmp() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $a, \GMP $b): void {
                assertType('GMP', $a + $b);
                assertType('GMP', $a - $b);
                assertType('GMP', $a * $b);
                assertType('GMP', $a / $b);
                assertType('GMP', $a ** $b);
                assertType('GMP', $a % $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn gmp_also_overloads_the_bitwise_and_shift_operators() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $a, \GMP $b): void {
                assertType('GMP', $a & $b);
                assertType('GMP', $a | $b);
                assertType('GMP', $a ^ $b);
                assertType('GMP', $a << $b);
                assertType('GMP', $a >> $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn gmp_combines_with_an_int_or_a_numeric_string_on_either_side() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $g, int $i): void {
                assertType('GMP', $g + $i);
                assertType('GMP', $i + $g);
                assertType('GMP', $g + '123');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn gmp_comparisons_return_bool_and_spaceship_returns_an_int_range() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $a, \GMP $b): void {
                assertType('bool', $a < $b);
                assertType('bool', $a == $b);
                assertType('int<-1, 1>', $a <=> $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn gmp_unary_minus_plus_and_bitnot_stay_gmp() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $g): void {
                assertType('GMP', -$g);
                assertType('GMP', +$g);
                assertType('GMP', ~$g);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_float_operand_does_not_get_the_gmp_overload_treatment() {
    // Real PHPStan's own extension explicitly never accepts a float on
    // the other side - left alone (falls through to this project's
    // ordinary numeric inference) rather than guessing GMP.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $g, float $x): void {
                assertType('float', $g + $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bcmath_number_plus_bcmath_number_returns_bcmath_number() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\BcMath\Number $a, \BcMath\Number $b): void {
                assertType('BcMath\Number', $a + $b);
                assertType('BcMath\Number', $a - $b);
                assertType('BcMath\Number', $a * $b);
                assertType('BcMath\Number', $a / $b);
                assertType('BcMath\Number', $a ** $b);
                assertType('BcMath\Number', $a % $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bcmath_number_does_not_overload_the_bitwise_operators() {
    // Unlike GMP - `BcMath\Number`'s own real extension has no bitwise/
    // shift entries at all, so this must fall through to plain `int`
    // rather than guessing `BcMath\Number`.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\BcMath\Number $a, \BcMath\Number $b): void {
                assertType('int', $a & $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bcmath_number_does_not_overload_bitnot_either() {
    // Real bug found while writing this phase's own GMP tests: the
    // pre-existing `is_bitwise_not_operand` unconditionally rejected
    // every `Type::Object`, including GMP - flagging real, valid `~$gmp`
    // syntax as `unaryOp.invalid`. Fixed via a thin `Checker`-method
    // wrapper (`bitnot_operand_is_valid`) that adds the one GMP
    // exception; `BcMath\Number` genuinely doesn't overload `~` at all,
    // so it must still be flagged.
    let d = diagnostics_for(
        r#"<?php
            function f(\BcMath\Number $n): void {
                ~$n;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "unaryOp.invalid"),
        "got: {d:?}"
    );
}

#[test]
fn bcmath_number_unary_minus_and_plus_stay_bcmath_number() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(\BcMath\Number $n): void {
                assertType('BcMath\Number', -$n);
                assertType('BcMath\Number', +$n);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn plain_int_arithmetic_is_unaffected() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(int $a, int $b): void {
                assertType('int', $a + $b);
                assertType('int|float', $a / $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
