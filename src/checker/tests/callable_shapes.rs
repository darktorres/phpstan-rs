//! Structural `Closure(...)`-shaped PHPDoc callable types - parsing,
//! contextual closure-body checking against a shaped `Closure(...)`-
//! typed parameter, and invoking a callable-typed value directly.

use super::helpers::*;

// Two *different* shapes against each other - real contravariant-
// parameter/covariant-return subtyping, not the old unconditional
// leniency.

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
