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
