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

// `function.uselessReturnValue` - var_export()/print_r()/
// highlight_string() all print directly and return a useless null/true
// unless their own `$return` flag is passed `true` - only a problem
// once something tries to actually *use* that useless return value.

#[test]
fn a_bare_var_export_statement_is_never_flagged() {
    // The normal, intended way to use it - printing, with the return
    // value going nowhere by construction.
    let d = diagnostics_for(r#"<?php var_export([1, 2]);"#);
    assert!(
        !d.iter()
            .any(|x| x.identifier == "function.uselessReturnValue"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn assigning_var_exports_return_value_with_no_flag_is_flagged() {
    let d = diagnostics_for(r#"<?php $x = var_export([1, 2]);"#);
    assert!(
        d.iter()
            .any(|x| x.identifier == "function.uselessReturnValue"),
        "got: {d:?}"
    );
}

#[test]
fn assigning_var_exports_return_value_with_an_explicit_false_flag_is_flagged() {
    let d = diagnostics_for(r#"<?php $x = var_export([1, 2], false);"#);
    assert!(
        d.iter()
            .any(|x| x.identifier == "function.uselessReturnValue"),
        "got: {d:?}"
    );
}

#[test]
fn assigning_var_exports_return_value_with_true_is_never_flagged() {
    let d = diagnostics_for(r#"<?php $x = var_export([1, 2], true);"#);
    assert!(
        !d.iter()
            .any(|x| x.identifier == "function.uselessReturnValue"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn assigning_var_exports_return_value_with_an_unknown_flag_is_never_flagged() {
    // Don't guess - a genuinely unknown (non-literal) flag could be
    // true at runtime.
    let d = diagnostics_for(
        r#"<?php
            function f(bool $flag) {
                $x = var_export([1, 2], $flag);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "function.uselessReturnValue"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn print_r_and_highlight_string_are_also_covered() {
    let d = diagnostics_for(
        r#"<?php
            $a = print_r("x");
            $b = highlight_string("<?php echo 1;");
        "#,
    );
    assert_eq!(
        d.iter()
            .filter(|x| x.identifier == "function.uselessReturnValue")
            .count(),
        2,
        "got: {d:?}"
    );
}

#[test]
fn a_var_export_call_nested_inside_another_call_is_flagged() {
    // Not a bare statement itself, even though it's the outer call's
    // own argument, not an assignment.
    let d = diagnostics_for(r#"<?php strlen(var_export([1, 2]));"#);
    assert!(
        d.iter()
            .any(|x| x.identifier == "function.uselessReturnValue"),
        "got: {d:?}"
    );
}

// Phase 76: filter_var()'s own FILTER_VALIDATE_* return-type precision,
// confirmed directly via php.

#[test]
fn filter_var_validate_int_returns_int_or_false() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('int|false', filter_var($x, FILTER_VALIDATE_INT));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_validate_boolean_returns_bool_or_false() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                // `bool|false` normalizes to plain `bool` (`false`
                // is already a subtype of `bool`) - matches real
                // PHPStan too.
                assertType('bool', filter_var($x, FILTER_VALIDATE_BOOLEAN));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_validate_ip_returns_a_non_falsy_string_or_false() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('non-falsy-string|false', filter_var($x, FILTER_VALIDATE_IP));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_null_on_failure_returns_null_instead_of_false() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'non-falsy-string|null',
                    filter_var($x, FILTER_VALIDATE_IP, FILTER_NULL_ON_FAILURE)
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_recognizes_null_on_failure_inside_an_or_chain() {
    // A real corpus idiom: ORing FILTER_NULL_ON_FAILURE together with an
    // unrelated flag - contains_flag_name's own lenient bitwise-or-chain
    // scan (already established for preg_split's flags argument) covers
    // this the same way.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'non-falsy-string|null',
                    filter_var($x, FILTER_VALIDATE_IP, FILTER_NULL_ON_FAILURE | FILTER_FLAG_NO_PRIV_RANGE)
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_require_array_flag_falls_back_to_the_generic_stub() {
    // Genuinely more complex than this narrow slice attempts - don't
    // guess, just leave the generic stub type in place.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('mixed', filter_var($x, FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_an_options_array_falls_back_to_the_generic_stub() {
    // Real corpus find: filter_var($v, FILTER_VALIDATE_INT, ['options'
    // => ['min_range' => 1]]) narrows the successful result to a real
    // range this project doesn't attempt to compute - guessing plain
    // int|false here wrongly widened past a declared positive-int sink.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'mixed',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_a_computed_filter_falls_back_to_the_generic_stub() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x, int $filter): void {
                assertType('mixed', filter_var($x, $filter));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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
