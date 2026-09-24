//! `value-of<T>`/`key-of<T>` - mirrors PHPStan's own `ValueOfType`/
//! `KeyOfType`, both `LateResolvableType`s this project has no general
//! lazy/deferred type-resolution mechanism to fully replicate. `T` itself
//! already a real `array`/`array{...}` type needs no registry access at
//! all and is resolved precisely at parse time.
//!
//! `T` naming a real, registered enum is also resolved precisely -
//! `value-of<T>` only (matching PHPStan's `KeyOfType::getResult()`,
//! which has no such special case at all) via a one-time
//! `Registry::resolve_deferred_value_of` pass that runs once every
//! class/enum is indexed (this project's lower-risk substitute for
//! PHPStan's per-use laziness - see that function's own doc comment in
//! `registry.rs`). Everything else genuinely unresolvable (`key-of<T>`
//! naming a class/enum - PHPStan has no special case there either;
//! `value-of<T>`/`key-of<T>` naming an ordinary, non-enum class)
//! degrades to `Mixed` rather than a bogus, namespace-qualified "class
//! name".

use super::helpers::*;

// `value-of<T>` naming a real, registered enum - resolved by
// `Registry::resolve_deferred_value_of` (`registry.rs`) once every
// class/enum is indexed, reusing the enum's already-built `->value`
// property type rather than recomputing it.

#[test]
fn value_of_on_an_unresolvable_template_falls_back_to_the_native_return_type() {
    // `@return value-of<T>` where `T` is a `@template` placeholder (never
    // resolvable to a concrete enum, so `Registry::
    // resolve_deferred_value_of` degrades it to `Mixed`) must not
    // silently discard a genuinely useful native return type in favor of
    // a bare `Mixed` - `resolve_type`'s `reconcile_with_native` must
    // fall back to the native type for this exact shape.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of \UnitEnum
             */
            class Coercer {
                /**
                 * @return value-of<T>
                 */
                public function coerce(mixed $value): string|int {
                    return $value;
                }
            }
            function f(Coercer $c): void {
                assertType('string|int', $c->coerce('x'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn value_of_on_a_backed_enum_resolves_in_a_property_type_too() {
    // Exercises a different walked location than a function param/return -
    // `Registry::resolve_deferred_value_of` also rewrites property types.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            class Holder {
                /** @var value-of<Suit> */
                public $x;
            }
            function f(Holder $h): void {
                assertType("'H'|'S'", $h->x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
