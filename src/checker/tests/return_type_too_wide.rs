//! `TooWideFunctionReturnTypehintRule`/`TooWideMethodReturnTypehintRule`
//! - a declared return-type union member never actually produced.

use super::helpers::*;

#[test]
fn a_declared_union_member_never_returned_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(bool $b): int|string {
                if ($b) {
                    return 1;
                }
                return 2;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "return.unusedType" && x.message.contains("string")),
        "got: {d:?}"
    );
}

#[test]
fn every_declared_member_actually_returned_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(bool $b): int|string {
                if ($b) {
                    return 1;
                }
                return 'x';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_return_with_no_value_contributes_void_not_null() {
    // A function declared `int|null` that only ever does a bare
    // `return;` (never an explicit `return null;`/falls off the end)
    // still shouldn't flag `null` as unused - real PHPStan's own
    // `VoidType` push for a bare `return;` is separate from a `Null`
    // contribution, but `null` is still satisfied by the fact that PHP's
    // own bare `return;` really does produce a null value at runtime.
    // This test instead pins the more directly observable half: `int`
    // actually being produced is not spuriously flagged even though the
    // function also has a bare `return;` path.
    let d = all_diagnostics_for(
        r#"<?php
            function f(bool $b): int|null {
                if ($b) {
                    return 1;
                }
                return;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "return.unusedType" && x.message.contains("int")),
        "got: {d:?}"
    );
}

#[test]
fn a_function_with_no_explicit_return_at_all_is_not_flagged() {
    // The real, load-bearing correction: the falls-off-the-end path
    // alone must not make `current_function_return_points` look
    // "non-empty enough" to flag - that's `return.missing`'s own job.
    // A real `Union` declared type is used here specifically so this
    // exercises that gate - a non-Union declared type (a bare `int`)
    // bails even earlier, for an unrelated reason (this check never
    // proceeds at all for a non-Union declared type - see this
    // function's own doc comment), and wouldn't actually exercise the
    // explicit-return gate this test means to pin.
    let d = all_diagnostics_for(
        r#"<?php
            function f(): int|string {
                $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_single_non_union_declared_return_type_is_never_checked_here_at_all() {
    // Real PHPStan's own `TooWideTypeCheck::narrowType` only ever acts
    // on a literal `UnionType` (or, unbuilt here, a boolean-family type)
    // - for any other single declared type (a bare class, a
    // `list<int>`-shaped `Intersection`, ...) it leaves the declared
    // type completely unchanged, producing no diagnostic at all, even
    // when the actual return value doesn't satisfy it (that's `return.
    // type`'s own separate, already-existing job - this check must not
    // duplicate it).
    let d = all_diagnostics_for(
        r#"<?php
            /** @return list<int> */
            function f(): array {
                return ['a', 'b'];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_generator_is_exempt_entirely() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @return \Generator<int, int|string> */
            function f(bool $b): \Generator {
                if ($b) {
                    yield 1;
                    return;
                }
                yield 2;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_first_declaration_non_private_method_returning_only_null_is_exempt() {
    // A `final class` bypasses `should_check_too_wide_method_return_
    // type`'s own opt-in-flag gate unconditionally (mirroring the
    // throw-type check's identical precedent), so this exercises the
    // descendant-null exemption itself without needing the separate
    // `check_too_wide_return_type_in_protected_and_public_methods`
    // opt-in flag turned on.
    let d = all_diagnostics_for(
        r#"<?php
            interface HasFoo {
                /** @return int|null */
                public function foo(): ?int;
            }
            final class Impl implements HasFoo {
                public function foo(): ?int {
                    return null;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.unusedType"),
        "got: {d:?}"
    );
}

// `parameterByRef.type`, direct/compound assignment to a by-ref
// parameter.

#[test]
fn a_by_ref_parameter_assigned_a_mismatched_value_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int &$x): void {
                $x = 'not an int';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameterByRef.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_by_ref_parameter_assigned_a_matching_value_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int &$x): void {
                $x = 42;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_param_out_tag_widens_the_accepted_type_beyond_the_native_hint() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @param-out int|string $x */
            function f(int &$x): void {
                $x = 'now a string';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_compound_assignment_to_a_by_ref_parameter_is_checked_against_its_real_result() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int &$x): void {
                $x .= 'suffix';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameterByRef.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_by_ref_parameter_is_never_checked() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int $x): void {
                $x = 'not an int';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.type"),
        "got: {d:?}"
    );
}

// `parameterByRef.executionEnd` (45C) - unlike `parameterByRef.type`
// above, only ever fires for a by-ref parameter that actually declares
// a real `@param-out` tag.

#[test]
fn an_unnarrowed_by_ref_parameter_at_the_only_exit_point_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @param-out int $x */
            function f(int|string &$x): void {
                $unrelated = 1;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameterByRef.executionEnd"),
        "got: {d:?}"
    );
}

#[test]
fn a_by_ref_parameter_narrowed_before_every_exit_is_never_flagged_execution_end() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @param-out int $x */
            function f(int|string &$x): void {
                $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameterByRef.executionEnd"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_by_ref_parameter_with_no_param_out_tag_is_never_flagged_execution_end() {
    // Unlike `parameterByRef.type` (45A/45B), real `ParameterOutExecutionEndTypeRule`
    // bails entirely when there's no `@param-out` tag - the native type
    // alone never triggers this specific check.
    let d = all_diagnostics_for(
        r#"<?php
            function f(int &$x): void {
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameterByRef.executionEnd"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_by_ref_parameter_narrowed_on_every_branch_before_exit_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /** @param-out int $x */
            function f(int|string &$x, bool $cond): void {
                if ($cond) {
                    $x = 1;
                } else {
                    $x = 2;
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameterByRef.executionEnd"),
        "unexpected diagnostics: {d:?}"
    );
}

// `parameterByRef.tooWide` (45D) - the by-ref-parameter analogue of
// `return.unusedType`, gated the same way `check_too_wide_return_type`
// is (private/final bypass the opt-in flag; a protected/public method
// needs it).

#[test]
fn a_declared_by_ref_union_member_never_actually_assigned_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int|string &$x): void {
                $x = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameterByRef.tooWide"),
        "got: {d:?}"
    );
}

#[test]
fn every_declared_by_ref_union_member_actually_reachable_at_exit_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int|string &$x, bool $cond): void {
                if ($cond) {
                    $x = 1;
                } else {
                    $x = 'a string';
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.tooWide"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_single_non_union_declared_by_ref_type_is_never_checked_here_at_all() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int &$x): void {
                $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.tooWide"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_too_wide_by_ref_type_on_a_protected_method_needs_the_opt_in_flag() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                protected function f(int|string &$x): void {
                    $x = 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.tooWide"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_too_wide_by_ref_type_on_a_protected_method_is_flagged_with_the_opt_in_flag() {
    let d = diagnostics_for_with(
        r#"<?php
            abstract class Base {
                abstract protected function f(int|string &$x): void;
            }
            class Foo extends Base {
                protected function f(int|string &$x): void {
                    $x = 1;
                }
            }
        "#,
        |r| r.check_too_wide_by_ref_type_in_protected_and_public_methods = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameterByRef.tooWide"),
        "got: {d:?}"
    );
}

#[test]
fn a_too_wide_by_ref_type_on_a_private_method_bypasses_the_opt_in_flag() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private function f(int|string &$x): void {
                    $x = 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameterByRef.tooWide"),
        "got: {d:?}"
    );
}

// Closure/arrow-fn boundary exemption - real PHPStan's own
// `isInAnonymousFunction()` bail, for both `parameterByRef.type` and
// the two new checks above.

#[test]
fn a_closure_reassigning_a_captured_by_ref_parameter_is_never_checked_against_it() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int &$x): void {
                $cb = function () use (&$x) {
                    $x = 'not an int';
                };
                $cb();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.type"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_arrow_fn_reassigning_a_captured_by_ref_parameter_is_never_checked_against_it() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(int &$x): void {
                $cb = fn() => $x = 'not an int';
                $cb();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameterByRef.type"),
        "unexpected diagnostics: {d:?}"
    );
}
