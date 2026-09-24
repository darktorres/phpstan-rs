//! `GMP`/`BcMath\Number` operator-overloading return-type precision
//! (`GmpOperatorTypeSpecifyingExtension`,
//! `BcMathNumberOperatorTypeSpecifyingExtension`, and their unary
//! counterparts) - distinct from `operator_validity.rs`'s own
//! operand-*legality* matrix, which already recognizes these two classes
//! as legal operands but doesn't refine the *result* type.

use super::helpers::*;

#[test]
fn bcmath_number_does_not_overload_bitnot_either() {
    // `is_bitwise_not_operand` unconditionally rejects every
    // `Type::Object`; the `bitnot_operand_is_valid` wrapper adds the one
    // GMP exception. `BcMath\Number` genuinely doesn't overload `~` at
    // all, so it must still be flagged.
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
