//! PHPDoc conditional return types (`@return ($param is X ? A : B)`) -
//! `Type::Conditional`, `types::resolve_conditional` (Phase 80 follow-up).

use super::helpers::*;

#[test]
fn a_conditional_return_type_picks_the_true_branch_for_a_matching_argument() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return $x; }
            assertType('string', f(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_conditional_return_type_picks_the_false_branch_for_a_non_matching_argument() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return $x; }
            assertType('bool', f('hello'));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_negated_conditional_return_type_inverts_the_branch_choice() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is not int ? string : bool)
             */
            function f($x) { return $x; }
            assertType('bool', f(5));
            assertType('string', f('hello'));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_conditional_return_type_with_an_untyped_argument_falls_back_to_the_union_of_both_branches() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param mixed $x
             * @return ($x is int ? string : bool)
             */
            function f($x) { return $x; }
            /** @var mixed $unknown */
            $unknown = null;
            assertType('string|bool', f($unknown));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_template_referenced_inside_a_conditional_branch_is_substituted_first() {
    // `infer_call_return_type` runs `substitute_template` before
    // `resolve_conditional` - a template used inside one of a
    // conditional's own branches (a real shape, Doctrine DBAL's own
    // `DriverManager::getConnection()`) must already be bound by the
    // time the branch is picked.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T
             * @param T $x
             * @return ($x is int ? T : string)
             */
            function f($x) { return $x; }
            assertType('5', f(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_class_reference_inside_a_conditional_branch_resolves_correctly_in_a_namespaced_file() {
    // Doctrine DBAL's own `DriverManager::getConnection()` real shape -
    // exercises `resolve_names_in_type`'s new `Conditional` arm (`Connection`
    // must resolve to its real, namespace-qualified FQN, not stay bare).
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            use function PHPStan\Testing\assertType;
            final class Connection {}
            /**
             * @param mixed $x
             * @return ($x is int ? Connection : string)
             */
            function f($x) { return $x; }
            assertType('App\Connection', f(5));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
