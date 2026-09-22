//! Structural `Closure(...)`-shaped PHPDoc callable types - parsing,
//! contextual closure-body checking against a shaped `Closure(...)`-
//! typed parameter, and invoking a callable-typed value directly.

use super::helpers::*;

#[test]
fn a_closure_shaped_phpdoc_type_is_displayed_with_its_own_signature() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param Closure(int): string $fn */
            function f($fn): void {
                assertType('callable(int): string', $fn);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn a_bare_callable_type_and_a_shaped_closure_type_are_mutually_assignable() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): string $fn */
            function needsShaped($fn): void {}
            /** @param callable $fn */
            function needsBare($fn): void {}
            /** @param Closure(int): string $fn */
            function test($fn): void {
                needsBare($fn);
            }
            function test2(callable $fn): void {
                needsShaped($fn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Two *different* shapes against each other - real contravariant-
// parameter/covariant-return subtyping, not the old unconditional
// leniency.

#[test]
fn a_narrower_return_type_is_assignable_to_a_wider_one_covariantly() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): (string|int) $fn */
            function needsWiderReturn($fn): void {}
            /** @param Closure(int): string $fn */
            function test($fn): void {
                needsWiderReturn($fn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_wider_return_type_is_not_assignable_to_a_narrower_one() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): int $fn */
            function needsIntReturn($fn): void {}
            /** @param Closure(int): string $fn */
            function test($fn): void {
                needsIntReturn($fn);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "a Closure returning string must not satisfy a Closure(int): int position, got: {d:?}"
    );
}

#[test]
fn a_closure_accepting_a_wider_parameter_type_is_assignable_to_a_narrower_position() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): void $fn */
            function needsIntParam($fn): void {}
            /** @param Closure(int|string): void $fn */
            function test($fn): void {
                needsIntParam($fn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_closure_accepting_a_narrower_parameter_type_is_not_assignable_to_a_wider_position() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int|string): void $fn */
            function needsWiderParam($fn): void {}
            /** @param Closure(int): void $fn */
            function test($fn): void {
                needsWiderParam($fn);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "a Closure(int) only handles int, so it can't stand in for a position whose \
         own callers might pass a string too - got: {d:?}"
    );
}

#[test]
fn a_closure_requiring_more_arguments_than_the_expected_shape_provides_is_rejected() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): void $fn */
            function needsOneArg($fn): void {}
            /** @param Closure(int, int): void $fn */
            function test($fn): void {
                needsOneArg($fn);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "a Closure requiring 2 arguments can't stand in where only 1 is ever \
         supplied - got: {d:?}"
    );
}

#[test]
fn a_closure_with_an_extra_optional_parameter_is_still_assignable() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): void $fn */
            function needsOneArg($fn): void {}
            /** @param Closure(int, int=): void $fn */
            function test($fn): void {
                needsOneArg($fn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_closure_taking_fewer_parameters_than_the_expected_shape_is_assignable() {
    // PHP silently drops extra arguments passed to a user-defined
    // callable, so a zero-argument closure stands in for a one-argument
    // shape - real PHPStan's `CallableTypeHelper` only walks the passed
    // callable's own parameters.
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int, string): void $fn */
            function needsTwoArgs($fn): void {}
            /** @param Closure(int): void $fn */
            function test($fn): void {
                needsTwoArgs($fn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_required_parameter_where_the_expected_shape_is_optional_is_rejected() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int=): void $fn */
            function mayOmitArg($fn): void {}
            /** @param Closure(int): void $fn */
            function test($fn): void {
                mayOmitArg($fn);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "the accepting side may call it with no argument - got: {d:?}"
    );
}

#[test]
fn a_mixed_return_or_parameter_type_is_treated_as_any() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): string $fn */
            function needsString($fn): void {}
            /** @param Closure(mixed): mixed $fn */
            function test($fn): void {
                needsString($fn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_template_param_inside_a_closure_shape_is_marked_not_left_as_a_bogus_class_reference() {
    // Not full call-site template *substitution* - `check_expr`'s arm
    // reads the callee's own raw, unsubstituted
    // `FunctionSig.params[i].ty` as the override list, with no per-call
    // generic-argument inference of its own. What this verifies is
    // `mark_template_params`: `T` inside `Closure(T): string` is a real
    // `Type::TemplateParam`, not an unresolved `Object("T", [])` a real
    // call could never satisfy. An already-hinted closure parameter
    // (`int $x` here) keeps its own native hint regardless either way
    // (`check_closure_literal_with_param_overrides`'s own "declared
    // type always wins over an inferred override" rule), so this
    // doesn't by itself prove template substitution happens for an
    // *unhinted* one.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T
             * @param T $value
             * @param Closure(T): string $fn
             */
            function apply($value, $fn): void {
                assertType('T', $value);
            }
            apply(5, function (int $x): string {
                assertType('int', $x);
                return (string) $x;
            });
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn a_closure_literal_argument_at_a_closure_shaped_parameter_gets_its_unhinted_params_typed() {
    // `$x` has no native/arrow-fn type hint of its own, so it should
    // pick up `int` from the declared `Closure(int): string` shape -
    // calling an int-only method on it should be clean, and calling a
    // string-only method should be flagged.
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): string $fn */
            function apply($fn): void {}
            apply(function ($x) {
                return $x + 1;
            });
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_closure_literal_argument_body_bug_at_a_closure_shaped_parameter_is_flagged() {
    // A scalar receiver (`int`) deliberately isn't usable here: this
    // project's own `MethodCall` handling only ever reports `method.
    // notFound` for a receiver type it can extract a real class from
    // (`Type::Object`/`EnumCase`/`Union`/`Intersection` - see `recv_
    // class`'s own match in `checker/expr.rs`) - a scalar-typed receiver
    // is silently treated the same as `Mixed` (nothing checkable), so
    // it can never distinguish "the injected type is really `int`" from
    // "the injection silently failed and `$x` fell back to `Mixed`."
    // `Foo` (a real, known class with no `paint()` method) gives a
    // signal that only fires when the injection actually worked.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param Closure(Foo): void $fn */
            function apply($fn): void {}
            apply(function ($x) {
                $x->paint();
            });
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.notFound" || x.message.contains("paint")),
        "expected an undefined-method diagnostic on the injected Foo param, got: {d:?}"
    );
}

#[test]
fn an_already_hinted_closure_literal_parameter_keeps_its_own_hint_over_the_shape() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param Closure(int): void $fn */
            function apply($fn): void {}
            apply(function (string $x) {
                assertType('string', $x);
            });
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn invoking_a_shaped_callable_value_infers_its_declared_return_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param Closure(int): string $fn */
            function test($fn): void {
                assertType('string', $fn(1));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn invoking_a_shaped_callable_value_with_too_few_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int, int): string $fn */
            function test($fn): void {
                $fn(1);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "expected arguments.count, got: {d:?}"
    );
}

#[test]
fn invoking_a_shaped_callable_value_with_too_many_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): string $fn */
            function test($fn): void {
                $fn(1, 2);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "expected arguments.count, got: {d:?}"
    );
}

#[test]
fn invoking_a_shaped_callable_value_with_a_mismatched_argument_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int): string $fn */
            function test($fn): void {
                $fn('not an int');
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "expected argument.type, got: {d:?}"
    );
}

#[test]
fn invoking_a_shaped_callable_value_through_an_array_element_still_checks_its_callee() {
    // The callee sub-expression of a dynamic call (here, an
    // array-element access, not a plain variable) must be independently
    // `check_expr`'d - an undefined variable used as the array itself
    // should still be flagged.
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $fns['x']();
            }
        "#,
    );
    assert!(
        !d.is_empty(),
        "expected the undefined-variable callee to be flagged, got none: {d:?}"
    );
}

#[test]
fn a_named_argument_to_a_dynamic_callable_call_is_not_type_checked() {
    // `CallableParam` carries no parameter name at all - a named argument
    // at a dynamic callable call site can't be resolved, so this project
    // deliberately stays silent rather than guessing.
    let d = diagnostics_for(
        r#"<?php
            /** @param Closure(int $x): string $fn */
            function test($fn): void {
                $fn(x: 'not an int');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
