//! Phase 71 Item 3: `preg_quote()` delimiter cross-checking against the
//! actual pattern delimiter of a concatenated regex pattern.

use super::helpers::*;

#[test]
fn preg_quote_missing_delimiter_arg_reports() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $needle): void {
                preg_match('/' . preg_quote($needle) . '/', "subject");
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.invalidPregQuote"
            && x.message.contains("missing delimiter")),
        "got: {d:?}"
    );
}

#[test]
fn preg_quote_wrong_delimiter_reports() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $needle): void {
                preg_match('/' . preg_quote($needle, '#') . '/', "subject");
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.invalidPregQuote"
            && x.message.contains("invalid delimiter")),
        "got: {d:?}"
    );
}

#[test]
fn preg_quote_correct_delimiter_no_report() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $needle): void {
                preg_match('/' . preg_quote($needle, '/') . '/', "subject");
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.invalidPregQuote"),
        "got: {d:?}"
    );
}

#[test]
fn preg_quote_default_escaped_delimiter_no_report() {
    // `.` is one of preg_quote()'s own always-escaped characters, so
    // giving it as the delimiter is inconsequential (preg_quote escapes
    // it either way) even though the pattern's own real delimiter is
    // `#`, not `.`.
    let d = diagnostics_for(
        r#"<?php
            function test(string $needle): void {
                preg_match('#' . preg_quote($needle, '.') . '#', "subject");
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.invalidPregQuote"),
        "got: {d:?}"
    );
}

#[test]
fn preg_quote_non_literal_pattern_prefix_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $prefix, string $needle): void {
                preg_match($prefix . preg_quote($needle) . '/', "subject");
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.invalidPregQuote"),
        "got: {d:?}"
    );
}

#[test]
fn preg_quote_call_outside_pattern_arg_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $needle, string $subject): void {
                preg_match('/foo/', preg_quote($needle) . $subject);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.invalidPregQuote"),
        "got: {d:?}"
    );
}

#[test]
fn preg_quote_delimiter_check_covers_preg_replace() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $needle): void {
                preg_replace('/' . preg_quote($needle) . '/', "x", "subject");
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.invalidPregQuote"),
        "got: {d:?}"
    );
}

#[test]
fn preg_quote_delimiter_check_covers_preg_split() {
    let d = diagnostics_for(
        r#"<?php
            function test(string $needle): void {
                preg_split('/' . preg_quote($needle) . '/', "subject");
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.invalidPregQuote"),
        "got: {d:?}"
    );
}
