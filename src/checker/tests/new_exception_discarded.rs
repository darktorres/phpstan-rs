//! Opt-in discarded-`new`-exception detection (ROADMAP Phase 147
//! 147c).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                new Exception("oops");
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "new.exceptionNotThrown"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_new_exception_statement_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                new Exception("oops");
            }
        "#,
        |r| r.check_new_exception_discarded = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "new.exceptionNotThrown"),
        "got: {d:?}"
    );
}

#[test]
fn an_actually_thrown_exception_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                throw new Exception("oops");
            }
        "#,
        |r| r.check_new_exception_discarded = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "new.exceptionNotThrown"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_new_of_a_non_exception_class_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {}
            function f(): void {
                new Foo();
            }
        "#,
        |r| r.check_new_exception_discarded = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "new.exceptionNotThrown"),
        "got: {d:?}"
    );
}

#[test]
fn an_exception_assigned_to_a_variable_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                $e = new Exception("oops");
                throw $e;
            }
        "#,
        |r| r.check_new_exception_discarded = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "new.exceptionNotThrown"),
        "got: {d:?}"
    );
}
