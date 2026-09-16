//! Opt-in base-Exception/Error-class detection (PHASE_BACKLOG Phase 147
//! 147e).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                throw new Exception("oops");
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throw.baseClass"),
        "got: {d:?}"
    );
}

#[test]
fn throwing_the_bare_exception_class_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                throw new Exception("oops");
            }
        "#,
        |r| r.check_throw_base_class = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throw.baseClass"),
        "got: {d:?}"
    );
}

#[test]
fn throwing_the_bare_error_class_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                throw new Error("oops");
            }
        "#,
        |r| r.check_throw_base_class = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throw.baseClass"),
        "got: {d:?}"
    );
}

#[test]
fn throwing_a_specific_subclass_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class MyException extends Exception {}
            throw new MyException("oops");
        "#,
        |r| r.check_throw_base_class = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throw.baseClass"),
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
        |r| r.check_throw_base_class = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throw.baseClass"),
        "got: {d:?}"
    );
}
