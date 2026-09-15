//! `regexp.pattern` - malformed PCRE pattern literals passed to the 8
//! `preg_*` builtins that take one.

use super::helpers::*;

#[test]
fn an_unknown_modifier_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            preg_match('/foo/y', 'x');
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "regexp.pattern"),
        "got: {d:?}"
    );
}

#[test]
fn an_unbalanced_group_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            preg_match('/(unclosed/', 'x');
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "regexp.pattern"),
        "got: {d:?}"
    );
}

#[test]
fn an_alphanumeric_delimiter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            preg_match('1foo1', 'x');
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "regexp.pattern"),
        "got: {d:?}"
    );
}

#[test]
fn a_valid_pattern_produces_no_diagnostic() {
    let d = diagnostics_for(
        r#"<?php
            preg_match('/foo/i', 'x');
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "regexp.pattern"),
        "got: {d:?}"
    );
}

#[test]
fn preg_match_all_is_checked() {
    let d = diagnostics_for(
        r#"<?php
            preg_match_all('/foo/y', 'x', $m);
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "regexp.pattern"));
}

#[test]
fn preg_split_is_checked() {
    let d = diagnostics_for(
        r#"<?php
            preg_split('/foo/y', 'x');
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "regexp.pattern"));
}

#[test]
fn preg_grep_is_checked_and_never_treats_an_array_argument_as_multiple_patterns() {
    let d = diagnostics_for(
        r#"<?php
            preg_grep('/foo/y', ['x']);
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "regexp.pattern"));
}

#[test]
fn preg_replace_checks_a_plain_string_pattern() {
    let d = diagnostics_for(
        r#"<?php
            preg_replace('/foo/y', 'bar', 'x');
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "regexp.pattern"));
}

#[test]
fn preg_replace_checks_every_pattern_in_an_array_argument() {
    let d = diagnostics_for(
        r#"<?php
            preg_replace(['/ok/i', '/foo/y'], ['a', 'b'], 'x');
        "#,
    );
    assert_eq!(
        d.iter()
            .filter(|x| x.identifier == "regexp.pattern")
            .count(),
        1,
        "got: {d:?}"
    );
}

#[test]
fn preg_replace_callback_checks_a_plain_string_pattern() {
    let d = diagnostics_for(
        r#"<?php
            preg_replace_callback('/foo/y', fn ($m) => $m[0], 'x');
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "regexp.pattern"));
}

#[test]
fn preg_filter_checks_an_array_of_patterns() {
    let d = diagnostics_for(
        r#"<?php
            preg_filter(['/foo/y'], ['a'], 'x');
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "regexp.pattern"));
}

#[test]
fn preg_replace_callback_array_checks_the_array_keys_not_the_values() {
    let d = diagnostics_for(
        r#"<?php
            preg_replace_callback_array(['/foo/y' => fn ($m) => $m[0]], 'x');
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "regexp.pattern"));
}

// Independent-path confirmation: a pattern shape `regex_groups.rs`'s own
// lenient capture-group scanner bails on entirely (a construct outside
// what it tries to understand) is still validated correctly by this
// separate, independent path - `pcre_validate` doesn't reuse that
// scanner at all, only its own delimiter-splitting helper.
#[test]
fn a_pattern_the_lenient_capture_group_scanner_bails_on_is_still_validated() {
    let d = diagnostics_for(
        r#"<?php
            preg_match('/(?<a>x)|(?<a>y)/', 'x', $m);
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "regexp.pattern"),
        "got: {d:?}"
    );
    let d2 = diagnostics_for(
        r#"<?php
            preg_match('/(?<a>x)|(?<a>y)/J', 'x', $m);
        "#,
    );
    assert!(
        !d2.iter().any(|x| x.identifier == "regexp.pattern"),
        "got: {d2:?}"
    );
}
