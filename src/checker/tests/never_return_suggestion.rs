//! Opt-in `never`-return-type suggestion (`return.neverCandidate`,
//! ROADMAP Phase 97 AA5).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                throw new \Exception();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn an_always_throwing_void_function_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                throw new \Exception();
            }
        "#,
        |r| r.check_never_return_suggestions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn an_undeclared_return_type_that_always_throws_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f() {
                throw new \Exception();
            }
        "#,
        |r| r.check_never_return_suggestions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_with_a_reachable_return_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(bool $x): void {
                if ($x) {
                    throw new \Exception();
                }
                return;
            }
        "#,
        |r| r.check_never_return_suggestions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_that_falls_off_the_end_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(bool $x): void {
                if ($x) {
                    throw new \Exception();
                }
            }
        "#,
        |r| r.check_never_return_suggestions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn an_already_never_declared_function_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): never {
                throw new \Exception();
            }
        "#,
        |r| r.check_never_return_suggestions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_declaring_a_real_type_but_never_returning_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): int {
                throw new \Exception();
            }
        "#,
        |r| r.check_never_return_suggestions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn an_infinite_loop_with_no_break_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                while (true) {
                    echo "loop";
                }
            }
        "#,
        |r| r.check_never_return_suggestions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.neverCandidate"),
        "got: {d:?}"
    );
}
