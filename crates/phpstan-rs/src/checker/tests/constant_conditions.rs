//! `if`/`while`/ternary/`elseif`/boolean-and-or-xor constant-condition detection.

use super::helpers::*;

// The `and`/`or` word-spelling identifier fidelity - real PHPStan's own
// `BooleanAndConstantConditionRule`/`BooleanOrConstantConditionRule`
// report a `logicalAnd.*`/`logicalOr.*` identifier (and an "and"/"or"
// message) for the word spelling, distinct from `booleanAnd.*`/
// `booleanOr.*` for `&&`/`||` - this project's own `BinOp::LogicalAnd`/
// `LogicalOr` (`src/ast.rs`) exist for exactly this distinction.

#[test]
fn a_boolean_and_used_directly_as_an_if_condition_does_not_double_report_the_right_side() {
    // `is_first_level` (`Checker::in_condition_position`) should suppress
    // the right-side/whole-result checks here - the `if` construct itself
    // already reports the whole condition being always-true via
    // `if.alwaysTrue`.
    let d = diagnostics_for("<?php function f(bool $x) { if ($x && true) { echo 1; } }");
    assert!(
        !d.iter()
            .any(|x| x.identifier == "booleanAnd.rightAlwaysTrue"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.alwaysTrue"),
        "got: {d:?}"
    );
}
