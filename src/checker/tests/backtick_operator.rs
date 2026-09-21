//! Real PHPStan's `BacktickRule` (`backtick.deprecated`, not
//! non-ignorable) - the backtick shell-exec operator (`` `cmd` ``) is
//! deprecated as of PHP 8.5 in favor of a plain `shell_exec()` call.

use super::helpers::*;

#[test]
fn a_backtick_shell_exec_on_php_8_5_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test() {
                $x = `echo hi`;
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "backtick.deprecated"),
        "got: {d:?}"
    );
}

#[test]
fn a_backtick_shell_exec_below_php_8_5_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function test() {
                $x = `echo hi`;
            }
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "backtick.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_backtick_shell_exec_with_no_configured_php_version_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test() {
                $x = `echo hi`;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "backtick.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_explicit_shell_exec_call_is_never_flagged_as_a_deprecated_backtick() {
    // Real PHPStan's own BacktickRule only listens for the `ShellExec`
    // node type - an author who already wrote shell_exec() themselves
    // is doing exactly what the deprecation recommends, not the thing
    // it warns about.
    let d = diagnostics_for_with(
        r#"<?php
            function test() {
                $x = shell_exec("echo hi");
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "backtick.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_backtick_shell_exec_still_type_checks_its_interpolated_contents() {
    // The ShellExec wrapper delegates straight through to the inner
    // Call for every other purpose - an undefined variable inside the
    // interpolated command must still be caught.
    let d = diagnostics_for(
        r#"<?php
            function test() {
                $x = `echo $undefinedInBacktick`;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "variable.undefined"),
        "got: {d:?}"
    );
}
