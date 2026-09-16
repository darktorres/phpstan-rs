//! Structurally-identical operands on both sides of a comparison,
//! `Sub`, or a plain self-assignment (`expr.duplicateOperands`,
//! PHASE_BACKLOG Phase 96 D7).

use super::helpers::*;

#[test]
fn identical_comparison_operands_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): bool {
                return $x === $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn identical_subtraction_operands_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $a): int {
                return $a - $a;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn identical_property_access_chains_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class C { public int $x = 1; }
            function f(C $obj): bool {
                return $obj->x === $obj->x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn a_self_assignment_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): void {
                $x = $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn different_variables_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x, int $y): bool {
                return $x === $y;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn identical_function_calls_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function rand_(): int { return 4; }
            function f(): bool {
                return rand_() === rand_();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn identical_int_literals_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): bool {
                return 1 === 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn a_compound_self_assign_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): void {
                $x += $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn different_index_bases_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $a, array $b, int $k): bool {
                return $a[$k] === $b[$k];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}

#[test]
fn identical_index_expressions_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $a, int $k): bool {
                return $a[$k] === $a[$k];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "expr.duplicateOperands"),
        "got: {d:?}"
    );
}
