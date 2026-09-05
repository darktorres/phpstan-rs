//! Phase 41 stage 1: `checker::noop`'s own supported slice of real
//! PHPStan's `NoopRule` - `xor`/`&&`/`||`/ternary as a bare, side-
//! effect-free statement.

use super::helpers::*;

#[test]
fn a_bare_xor_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a xor $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "logicalXor.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_boolean_and_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a && $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_boolean_or_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $a || $b;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanOr.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_ternary_statement_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, int $b, int $c): void {
                $a ? $b : $c;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "ternary.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_comparison_chain_joined_by_and_is_flagged() {
    // The real-world shape this rule mostly exists to catch: a
    // forgotten `if`, leaving a comparison's own result discarded.
    let d = diagnostics_for(
        r#"<?php
            function test(int $a, int $b, int $c, int $d): void {
                ($a === $b) && ($c !== $d);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn guard_or_die_idiom_is_never_flagged() {
    // Critical regression case: a real, legitimate, corpus-evidenced
    // idiom (`checker/stmt.rs`'s own "Phase 32 Finding K" narrowing
    // support exists for exactly this shape) - the right side has a
    // real side effect (a method call), so this must never be treated
    // as a no-op, even though its own top-level shape (a bare `||`
    // statement) otherwise looks identical to one.
    let d = diagnostics_for(
        r#"<?php
            class Guard {
                public function die_(string $msg): void {
                    exit($msg);
                }
            }
            function test(bool $cond, Guard $g): void {
                $cond || $g->die_('nope');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanOr.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_a_function_call_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function sideEffect(): bool { echo 'x'; return true; }
            function test(bool $a): void {
                $a && sideEffect();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_instantiation_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(bool $a): void {
                $a || new Foo();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanOr.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_an_assignment_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                ($a = $b) && $a;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_statement_containing_an_increment_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $i, bool $a): void {
                $i++ && $a;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_assignment_of_a_boolean_and_expression_is_never_flagged() {
    // The statement's own top-level shape is Assign, not Binary - never
    // even reaches the match arms that could flag it.
    let d = diagnostics_for(
        r#"<?php
            function test(bool $a, bool $b): void {
                $x = $a && $b;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}
