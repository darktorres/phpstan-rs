//! Opt-in "only booleans are allowed in a condition" family
//! (PHASE_BACKLOG Phase 101 E1, mirroring real `phpstan-strict-rules`'
//! `BooleansInConditions`).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): void {
                if ($x) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "if.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_if_condition_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): void {
                if ($x) {}
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "if.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_bool_if_condition_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(bool $x): void {
                if ($x) {}
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "if.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_mixed_typed_if_condition_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($x): void {
                if ($x) {}
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "if.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_elseif_condition_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x, int $y): void {
                if ($x) {
                } elseif ($y) {
                }
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "elseif.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_while_condition_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): void {
                while ($x) {}
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "while.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_do_while_condition_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): void {
                do {} while ($x);
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "doWhile.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_ternary_condition_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): int {
                return $x ? 1 : 2;
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "ternary.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_short_ternary_condition_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): int {
                return $x ?: 2;
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "ternary.condNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_negated_non_bool_expression_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): bool {
                return !$x;
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanNot.exprNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_and_operand_is_flagged_on_each_side() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x, int $y): bool {
                return $x && $y;
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanAnd.leftNotBoolean"),
        "got: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanAnd.rightNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_or_operand_is_flagged_on_each_side() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x, int $y): bool {
                return $x || $y;
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanOr.leftNotBoolean"),
        "got: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanOr.rightNotBoolean"),
        "got: {d:?}"
    );
}

#[test]
fn bool_typed_and_operands_are_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(bool $x, bool $y): bool {
                return $x && $y;
            }
        "#,
        |r| r.check_boolean_conditions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("booleanAnd.")),
        "got: {d:?}"
    );
}
