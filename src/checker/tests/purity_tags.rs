//! Tests for `@phpstan-impure` tag validation
//! (`checker::noop::check_declared_purity_tag`).
use super::helpers::*;

#[test]
fn a_function_declared_impure_but_provably_pure_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-impure */
            function addOne(int $x): int {
                return $x + 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "impureFunction.pure"),
        "got: {d:?}"
    );
}

#[test]
fn a_method_declared_impure_but_provably_pure_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Calc {
                /** @phpstan-impure */
                public function addOne(int $x): int {
                    return $x + 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "impureMethod.pure"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_declared_impure_that_really_is_impure_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-impure */
            function logIt(string $msg): void {
                echo $msg;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "impureFunction.pure"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_provably_pure_function_with_no_purity_tag_at_all_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function addOne(int $x): int {
                return $x + 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "impureFunction.pure"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 41 follow-up: `check_purity_impure_points` -
// `impure.<id>`/`possiblyImpure.<id>`/`pure{Function,Method}.
// parameterByRef`/`pure{Function,Method}.void`, for a declaration real
// PHPStan considers pure (`@phpstan-pure`-tagged, or classifier-proven).

#[test]
fn a_function_declared_pure_but_echoing_is_flagged_certain_impure() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-pure */
            function logIt(string $msg): void {
                echo $msg;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "impure.echo"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_declared_pure_but_calling_an_unknown_function_is_flagged_possibly_impure() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-pure */
            function callIt(): void {
                someTotallyUnknownFunction();
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "possiblyImpure.functionCall"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_declared_pure_with_a_by_ref_parameter_is_flagged() {
    // `purity_gate_passes` already unconditionally disqualifies any
    // by-ref parameter from ever reaching `is_transitively_pure` at all
    // (a real, disclosed simplification of real PHPStan's own
    // `createsNewVariable()` distinction), so this diagnostic can only
    // ever fire for the tagged-but-classifier-disagrees case, never for
    // a classifier-proven-pure declaration.
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-pure */
            function sortIt(array &$xs): void {
                sort($xs);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "pureFunction.parameterByRef"),
        "got: {d:?}"
    );
}

#[test]
fn a_pure_void_function_with_no_side_effects_is_flagged_void() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-pure */
            function doNothing(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "pureFunction.void"),
        "got: {d:?}"
    );
}

#[test]
fn a_pure_constructor_is_never_flagged_void_even_though_it_has_no_return_value() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @phpstan-pure */
            final class Point {
                public function __construct(private int $x) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "pureMethod.void"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_provably_pure_function_with_no_purity_tag_never_gets_any_of_the_impure_point_diagnostics() {
    // Real, disclosed architectural fact: `registry::purity`'s own fixed
    // point is short-circuiting - `is_transitively_pure == true` already
    // implies zero disqualifying constructs were found in the body, so
    // none of `impure.*`/`possiblyImpure.*`/`pure*.parameterByRef`/
    // `pure*.void` can ever be non-vacuous for a classifier-*proven*-pure
    // declaration (only for the `@phpstan-pure`-tagged-but-actually-
    // impure case, covered by the tests just above).
    let d = all_diagnostics_for(
        r#"<?php
            function addOne(int $x): int {
                return $x + 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("impure.")
            || x.identifier.starts_with("possiblyImpure.")
            || x.identifier.ends_with(".parameterByRef")
            || x.identifier.ends_with(".void")),
        "unexpected diagnostics: {d:?}"
    );
}
