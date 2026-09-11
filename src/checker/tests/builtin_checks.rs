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

// `min`/`max` - a Piwigo corpus false positive found both functions had
// zero return-type handling at all (the vendored stub declares `mixed`
// params/return, matching real PHP's own native signature - real
// PHPStan's own precision here comes entirely from a bundled
// `MinMaxFunctionReturnTypeExtension` this project has no equivalent of).
// Scoped to the fallback path that extension itself uses whenever it
// can't constant-fold (this project has no constant-value tracking as
// elaborate as real PHPStan's own): the union of every argument's own
// type, only when none of them could be an array (a single array
// argument is real PHP's *other*, differently-behaved `min`/`max`
// overload, deliberately not modeled here).

#[test]
fn min_of_two_ints_infers_int() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string $x */
            function needsString($x): void {}
            function f(int $a, int $b): void {
                needsString(min($a, $b));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn max_of_an_int_and_a_string_infers_their_union() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int $x */
            function needsInt($x): void {}
            function f(int $a, string $b): void {
                needsInt(max($a, $b));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"
            && x.message.contains("expects int, int|string given")),
        "got: {d:?}"
    );
}

#[test]
fn min_with_a_possible_array_argument_stays_mixed() {
    // Deliberately not modeled (real `min(array $value)` is a distinct,
    // differently-behaved overload) - must not be over-widened into a
    // false claim about what a possibly-array argument's own min could be.
    let d = diagnostics_for(
        r#"<?php
            /** @param string $x */
            function needsString($x): void {}
            function f(array $a, int $b): void {
                needsString(min($a, $b));
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
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
    // guess, just leave the generic stub type in place. (A non-provably-
    // array input - see the Phase 76 sub-item A tests below for the
    // provably-array case, which *is* now handled.)
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

// Phase 76 sub-item A: `FILTER_REQUIRE_ARRAY`/`FILTER_FORCE_ARRAY` per-
// element return type.

#[test]
fn filter_var_with_require_array_flag_and_a_known_array_input_wraps_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string> $values */
            function f(array $values): void {
                assertType('array<int|false>', filter_var($values, FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_var_with_force_array_flag_wraps_regardless_of_input_shape() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('array<int|false>', filter_var($x, FILTER_VALIDATE_INT, FILTER_FORCE_ARRAY));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_var_require_array_takes_priority_over_force_array_when_both_are_set() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string> $values */
            function f(array $values): void {
                assertType(
                    'array<int|false>',
                    filter_var($values, FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY | FILTER_FORCE_ARRAY)
                );
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_var_min_range_option_survives_require_array_wrap() {
    // The `|false` fallback is still unioned into the per-element type
    // even with a `min_range` option present - confirmed directly
    // against real PHPStan's own `getType()`: `$exactType === null` (a
    // non-literal array element's own value is never "exact") triggers
    // the fallback-union unconditionally, independent of whether a
    // range option also narrowed the base type.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string> $values */
            function f(array $values): void {
                assertType(
                    'array<int<1, max>|false>',
                    filter_var($values, FILTER_VALIDATE_INT, [
                        'flags' => FILTER_REQUIRE_ARRAY,
                        'options' => ['min_range' => 1],
                    ])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Phase 76 sub-item B: PHP 8.5's `FILTER_THROW_ON_FAILURE`.

#[test]
fn filter_var_with_throw_on_failure_removes_the_false_fallback() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('int', filter_var($x, FILTER_VALIDATE_INT, FILTER_THROW_ON_FAILURE));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_var_with_throw_on_failure_ored_with_another_flag_still_removes_the_fallback() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int',
                    filter_var($x, FILTER_VALIDATE_INT, FILTER_FLAG_ALLOW_OCTAL | FILTER_THROW_ON_FAILURE)
                );
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_var_with_throw_on_failure_and_a_literal_default_option_still_removes_the_default() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int',
                    filter_var($x, FILTER_VALIDATE_INT, [
                        'flags' => FILTER_THROW_ON_FAILURE,
                        'options' => ['default' => 42],
                    ])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Phase 76 sub-item C: `filter_input()` itself.

#[test]
fn filter_input_validate_int_returns_int_or_null_or_false() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('int|null|false', filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_input_with_null_on_failure_still_includes_both_null_and_false() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType(
                    'int|null|false',
                    filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE)
                );
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_input_with_a_literal_default_option_keeps_the_unconditional_null_and_false() {
    // `int|42` simplifies to plain `int` (this project's own `union_types`
    // already collapses a literal into its own already-present base type -
    // see `filter_var_with_a_default_option_uses_it_as_the_failure_
    // fallback_instead_of_false`'s identical, already-established note),
    // so the real thing this test demonstrates is that `null`/`false`
    // still both appear even with a *custom* default option given, not
    // that the literal default itself survives unsimplified.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType(
                    'int|null|false',
                    filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT, ['options' => ['default' => 42]])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn filter_input_with_require_array_flag_falls_back_to_the_generic_stub() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType(
                    'mixed',
                    filter_input(INPUT_GET, 'x', FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY)
                );
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Phase 76 follow-up: an `['options' => [...]]` array's own `default`/
// `min_range`/`max_range` sub-keys are now recognized (this phase's own
// prior write-up called this "needs Phase 51's IntegerRangeType
// completion" - re-checked rather than trusted, and turned out to be
// unrelated: `normalize_int_range`/`int_range_bounds` already existed,
// built this session for Phase 15's own range-arithmetic work).

#[test]
fn filter_var_with_a_min_and_max_range_option_narrows_to_a_real_int_range() {
    // Real corpus find: filter_var($v, FILTER_VALIDATE_INT, ['options'
    // => ['min_range' => 1]]) narrows the successful result to a real
    // range - guessing plain int|false here wrongly widened past a
    // declared positive-int sink.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int<1, 10>|false',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1, 'max_range' => 10]])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_only_a_min_range_option_leaves_the_upper_bound_open() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int<1, max>|false',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_a_default_option_uses_it_as_the_failure_fallback_instead_of_false() {
    // `int|42` simplifies to plain `int` (42 is already a member of it) -
    // this project's own `union_types` already collapses that generally,
    // not a precision loss specific to this check. A string default
    // (`filter_var_with_a_non_int_default_option_below`) demonstrates the
    // fallback actually being used, unsimplified.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['default' => 42]])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_a_non_int_default_option_widens_the_result_union() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'int|null',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => ['default' => null]])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_min_range_option_is_ignored_for_a_non_int_filter() {
    // `FILTER_VALIDATE_FLOAT`'s own `min_range`/`max_range` aren't
    // modeled (real PHPStan itself doesn't support FloatRangeType
    // either, confirmed directly against `FilterFunctionReturnTypeHelper
    // ::getFilterTypeOptions()`'s own commented-out entry) - the base
    // type is used unchanged rather than misapplying an int range to it.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType(
                    'float|false',
                    filter_var($x, FILTER_VALIDATE_FLOAT, ['options' => ['min_range' => 1]])
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn filter_var_with_an_unparseable_options_array_falls_back_to_the_generic_stub() {
    // A non-constant `options` value (`$dynamic` here) can't be
    // inspected at all - falls through unchanged, matching real
    // PHPStan's own "too complicated" leniency rather than guessing.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x, array $dynamic): void {
                assertType(
                    'mixed',
                    filter_var($x, FILTER_VALIDATE_INT, ['options' => $dynamic])
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

// Phase 71: `filter_var()`'s `FILTER_NULL_ON_FAILURE`/`FILTER_THROW_ON_FAILURE`
// flag conflict, PHP 8.5+.
#[test]
fn filter_var_null_and_throw_on_failure_bitwise_or_reports() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(string $x) {
                filter_var($x, FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE | FILTER_THROW_ON_FAILURE);
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "filterVar.nullOnFailureAndThrowOnFailure"),
        "got: {d:?}"
    );
}

#[test]
fn filter_var_null_and_throw_on_failure_options_array_flags_reports() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(string $x) {
                filter_var($x, FILTER_VALIDATE_INT, [
                    'flags' => FILTER_NULL_ON_FAILURE | FILTER_THROW_ON_FAILURE,
                ]);
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "filterVar.nullOnFailureAndThrowOnFailure"),
        "got: {d:?}"
    );
}

#[test]
fn filter_var_only_null_on_failure_no_report() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(string $x) {
                filter_var($x, FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE);
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "filterVar.nullOnFailureAndThrowOnFailure"),
        "got: {d:?}"
    );
}

#[test]
fn filter_var_conflict_silent_below_php85() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(string $x) {
                filter_var($x, FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE | FILTER_THROW_ON_FAILURE);
            }
        "#,
        |r| r.php_version = Some((8, 3)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "filterVar.nullOnFailureAndThrowOnFailure"),
        "got: {d:?}"
    );
}

#[test]
fn filter_var_conflict_silent_with_no_configured_version() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $x) {
                filter_var($x, FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE | FILTER_THROW_ON_FAILURE);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "filterVar.nullOnFailureAndThrowOnFailure"),
        "got: {d:?}"
    );
}
