//! Opt-in "boolean trap" detection (`argument.booleanTrap`,
//! ROADMAP Phase 96 D2) - a bare `true`/`false` literal passed
//! positionally into a bare-`bool`-typed parameter.

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function setVisible(bool $visible): void {}
            setVisible(true);
        "#,
    );
    assert!(d.is_empty(), "got: {d:?}");
}

#[test]
fn a_positional_bare_bool_literal_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function setVisible(bool $visible): void {}
            setVisible(true);
        "#,
        |r| r.check_boolean_trap = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.booleanTrap"),
        "got: {d:?}"
    );
}

#[test]
fn a_named_bool_argument_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function setVisible(bool $visible): void {}
            setVisible(visible: true);
        "#,
        |r| r.check_boolean_trap = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.booleanTrap"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_bool_parameter_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): void {}
            f(1);
        "#,
        |r| r.check_boolean_trap = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.booleanTrap"),
        "got: {d:?}"
    );
}

#[test]
fn a_bool_variable_argument_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function setVisible(bool $visible): void {}
            function test(bool $x): void {
                setVisible($x);
            }
        "#,
        |r| r.check_boolean_trap = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.booleanTrap"),
        "got: {d:?}"
    );
}
