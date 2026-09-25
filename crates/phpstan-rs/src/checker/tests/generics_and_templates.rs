//! @template binding, variance, bounds, and class-level generics.

use super::helpers::*;

// `Registry::accepts` applies real variance to a generic class's
// own type arguments, instead of ignoring them entirely (`Collection<Cat>`
// and `Collection<Dog>` are not mutually assignable). These exercise it
// through ordinary argument-type checking (`checker/calls.rs`), the
// same real call site every one of these comparisons goes through in
// practice.

#[test]
fn a_bare_erased_generic_reference_is_never_flagged_either_direction() {
    // The overwhelmingly common real shape: no `<T>` at all. Padding a
    // missing slot with plain `Mixed` (not the class's own declared
    // bound - see `Registry::accepts`'s own doc comment for why
    // that distinction matters) keeps this silent, matching this
    // project's established "erased generic = don't know = don't flag"
    // leniency everywhere else - even though `Collection` here declares
    // a real, non-`Mixed` bound on its own template.
    let d = diagnostics_for(
        r#"<?php
            class Shape {}
            class Circle extends Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<Circle> $c */
            function needsCircleCollection($c) {}
            /** @param Collection $c */
            function f($c) {
                needsCircleCollection($c);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `generics.callSiteVarianceConflict`/`.callSiteVarianceRedundant` -
// `check_call_site_variance`'s own port of `GenericObjectTypeCheck`'s
// call-site-variance half.

#[test]
fn a_call_site_variance_marker_inside_a_declared_parameter_overrides_declaration_soundness_checking()
 {
    // `checker::generics::walk_variance` (declaration-soundness, not
    // assignability) must consult the exact same override -
    // `Box`'s own `@template-covariant T` may only appear in an output
    // (return) position, but a call-site `contravariant` marker on the
    // nested `Collection<contravariant T>` flips the composed position
    // from input back to output, making this sound - proof
    // `Registry::effective_slot_variance` is really wired into
    // `walk_variance`, not just `accepts`.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template U
             */
            class Collection {}
            /**
             * @template-covariant T
             */
            class Good {
                /** @param Collection<contravariant T> $c */
                public function consume($c): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `ReflectionMethod`/`ReflectionProperty` carry a synthesized `TClass`
// template (not real in phpstorm-stubs) so the 4 receiver-propagating
// `ReflectionClass<T>` methods below bind it to `T`, and
// `getDeclaringClass()` hands it straight back.
#[test]
fn reflection_class_get_method_binds_its_receiver_s_own_template() {
    // Also carries Phase 77's own reflected-member-binding fact
    // (`Type::ReflectedMember`) since `'bar'` is a genuine string
    // literal - see the dedicated `reflected_member_binding` tests below
    // for `invoke()`/`getValue()` actually consuming it.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            final class Foo {}
            function f(): void {
                $reflClass = new ReflectionClass(Foo::class);
                assertType(
                    "ReflectionMethod<Foo>&reflectedMember(Foo, 'bar')",
                    $reflClass->getMethod('bar'),
                );
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reflection_class_get_method_leaves_t_erased_when_the_receiver_s_own_t_is_erased() {
    // Negative control: an erased receiver `T` (a plain `string`-typed
    // variable, not `Foo::class`) must not wrongly bind the returned
    // `ReflectionMethod`'s own `TClass` to anything concrete either.
    // Renders as `ReflectionMethod<mixed>`, not bare `ReflectionMethod` -
    // `substitute_class_templates`'s own generic `substitute_scoped`
    // fallback (used pervasively for every other generic method-return
    // substitution project-wide) always fills an unbound `TemplateParam`
    // with a literal `Type::Mixed(false)` argument, unlike `ExprKind::New`'s own
    // *separate*, special-cased "trim trailing unbound template
    // arguments" logic for a freshly-constructed object's own type
    // (which is what makes `new ReflectionClass($erased)` itself render
    // as bare `ReflectionClass` - a different code path, not something
    // an ordinary method-return substitution reuses).
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $className): void {
                $reflClass = new ReflectionClass($className);
                assertType('ReflectionMethod<mixed>', $reflClass->getMethod('bar'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
