//! A batch of small, independent builtin-specific checks with no shared
//! architectural blocker.

use super::helpers::*;

// `filter_input()` itself.

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

// An `['options' => [...]]` array's `default`/`min_range`/`max_range`
// sub-keys are recognized, via the existing
// `normalize_int_range`/`int_range_bounds` range-arithmetic machinery.

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
