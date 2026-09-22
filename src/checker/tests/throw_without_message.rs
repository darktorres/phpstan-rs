//! Opt-in missing-exception-message detection (ROADMAP Phase 147
//! 147d).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                throw new Exception();
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "throw.missingExceptionMessage"),
        "got: {d:?}"
    );
}

#[test]
fn throwing_with_no_constructor_args_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                throw new Exception();
            }
        "#,
        |r| r.check_throw_without_message = true,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "throw.missingExceptionMessage"),
        "got: {d:?}"
    );
}

#[test]
fn throwing_with_a_message_argument_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                throw new Exception("something went wrong");
            }
        "#,
        |r| r.check_throw_without_message = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "throw.missingExceptionMessage"),
        "got: {d:?}"
    );
}

#[test]
fn rethrowing_a_caught_exception_variable_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                try {
                    doSomething();
                } catch (Exception $e) {
                    throw $e;
                }
            }
            function doSomething(): void {}
        "#,
        |r| r.check_throw_without_message = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "throw.missingExceptionMessage"),
        "got: {d:?}"
    );
}
