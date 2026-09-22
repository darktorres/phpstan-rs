//! Opt-in swapped-adjacent-argument-name detection
//! (`arguments.possiblySwapped`, ROADMAP Phase 96 D6).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function resize(int $width, int $height): void {}
            function f(int $height, int $width): void {
                resize($height, $width);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "arguments.possiblySwapped"),
        "got: {d:?}"
    );
}

#[test]
fn crossed_variable_names_are_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function resize(int $width, int $height): void {}
            function f(int $height, int $width): void {
                resize($height, $width);
            }
        "#,
        |r| r.check_swapped_arguments = true,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "arguments.possiblySwapped"),
        "got: {d:?}"
    );
}

#[test]
fn correctly_ordered_variable_names_are_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function resize(int $width, int $height): void {}
            function f(int $width, int $height): void {
                resize($width, $height);
            }
        "#,
        |r| r.check_swapped_arguments = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "arguments.possiblySwapped"),
        "got: {d:?}"
    );
}

#[test]
fn unrelated_variable_names_are_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function resize(int $width, int $height): void {}
            function f(int $a, int $b): void {
                resize($a, $b);
            }
        "#,
        |r| r.check_swapped_arguments = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "arguments.possiblySwapped"),
        "got: {d:?}"
    );
}

#[test]
fn different_typed_parameters_are_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $width, string $height): void {}
            function g(string $height, int $width): void {
                f($height, $width);
            }
        "#,
        |r| r.check_swapped_arguments = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "arguments.possiblySwapped"),
        "got: {d:?}"
    );
}

#[test]
fn non_adjacent_crossed_names_are_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $width, int $mid, int $height): void {}
            function g(int $height, int $mid, int $width): void {
                f($height, $mid, $width);
            }
        "#,
        |r| r.check_swapped_arguments = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "arguments.possiblySwapped"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_variable_argument_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function resize(int $width, int $height): void {}
            function f(int $height): void {
                resize(1 + 1, $height);
            }
        "#,
        |r| r.check_swapped_arguments = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "arguments.possiblySwapped"),
        "got: {d:?}"
    );
}
