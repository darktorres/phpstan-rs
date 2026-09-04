//! Phase 71: a batch of small, independent builtin-specific checks with
//! no shared architectural blocker.

use super::helpers::*;

// `argument.unused` - `define()`'s own third `$case_insensitive`
// argument stopped doing anything as of PHP 8.0. Gated on a configured
// `php_version` (>= 8.0) - confirmed directly via `php` that this is a
// real runtime `E_WARNING`, not just a PHPStan-only observation.

#[test]
fn defines_third_argument_is_flagged_on_php_8_and_later() {
    let d = diagnostics_for_with(r#"<?php define("FOO", 1, true);"#, |r| {
        r.php_version = Some((8, 0));
    });
    assert!(
        d.iter().any(|x| x.identifier == "argument.unused"),
        "got: {d:?}"
    );
}

#[test]
fn defines_third_argument_is_flagged_case_insensitively() {
    let d = diagnostics_for_with(r#"<?php DEFINE("FOO", 1, true);"#, |r| {
        r.php_version = Some((8, 5));
    });
    assert!(
        d.iter().any(|x| x.identifier == "argument.unused"),
        "got: {d:?}"
    );
}

#[test]
fn defines_third_argument_is_never_flagged_on_php_7() {
    // Real, legitimate PHP < 8.0 - case-insensitive constants still
    // worked there.
    let d = diagnostics_for_with(r#"<?php define("FOO", 1, true);"#, |r| {
        r.php_version = Some((7, 4));
    });
    assert!(
        !d.iter().any(|x| x.identifier == "argument.unused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn defines_third_argument_is_never_flagged_with_no_configured_php_version() {
    // No configured version means nothing to check against yet, matching
    // every other PhpFeature-gated rule's own convention.
    let d = diagnostics_for(r#"<?php define("FOO", 1, true);"#);
    assert!(
        !d.iter().any(|x| x.identifier == "argument.unused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_plain_two_argument_define_is_never_flagged() {
    let d = diagnostics_for_with(r#"<?php define("FOO", 1);"#, |r| {
        r.php_version = Some((8, 5));
    });
    assert!(
        !d.iter().any(|x| x.identifier == "argument.unused"),
        "unexpected diagnostics: {d:?}"
    );
}

// `argument.type` - random_int()'s $min must be <= $max, confirmed
// directly via php as a real, unconditional ValueError at call time.

#[test]
fn random_int_with_min_greater_than_max_is_flagged() {
    let d = diagnostics_for(r#"<?php random_int(10, 1);"#);
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("random_int")),
        "got: {d:?}"
    );
}

#[test]
fn random_int_case_insensitive_call_name_is_still_checked() {
    let d = diagnostics_for(r#"<?php RANDOM_INT(10, 1);"#);
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("random_int")),
        "got: {d:?}"
    );
}

#[test]
fn random_int_with_min_less_than_max_is_never_flagged() {
    let d = diagnostics_for(r#"<?php random_int(1, 10);"#);
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("random_int")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn random_int_with_equal_min_and_max_is_never_flagged() {
    // A degenerate but legal single-value range.
    let d = diagnostics_for(r#"<?php random_int(5, 5);"#);
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("random_int")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn random_int_with_a_non_literal_argument_is_never_flagged() {
    // Don't guess - no IntegerRangeType in this project's own type
    // system to reason about a non-literal argument's own bounds.
    let d = diagnostics_for(
        r#"<?php
            function f(int $max) {
                random_int(10, $max);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("random_int")),
        "unexpected diagnostics: {d:?}"
    );
}
