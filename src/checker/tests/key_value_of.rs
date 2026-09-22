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

#[test]
fn value_of_on_a_plain_array_type_extracts_the_value_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param value-of<array<string, int>> $x */
            function f($x): void {
                assertType('int', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn key_of_on_a_plain_array_type_extracts_the_key_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param key-of<array<string, int>> $x */
            function f($x): void {
                assertType('string', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn value_of_on_an_array_shape_unions_every_field_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param value-of<array{a: int, b: string}> $x */
            function f($x): void {
                assertType('int|string', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn key_of_on_an_array_shape_unions_every_field_name_as_a_literal() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param key-of<array{a: int, 5: string}> $x */
            function f($x): void {
                assertType("'a'|5", $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn value_of_on_a_single_field_shape_is_the_bare_type_not_a_one_member_union() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param value-of<array{a: int}> $x */
            function f($x): void {
                assertType('int', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn key_of_on_a_class_reference_no_longer_produces_a_bogus_class_not_found() {
    // Mirrors PHPStan's `KeyOfType::getResult()` (`return
    // $this->type->getIterableKeyType();`) - a bare class/enum reference
    // has no special case there at all, so `Mixed` (an ordinary class
    // isn't iterable) is the correct, final answer here, not a
    // placeholder waiting on more work.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class C {}
            /** @param key-of<C> $x */
            function f($x): void {
                assertType('mixed', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `value-of<T>` naming a real, registered enum - resolved by
// `Registry::resolve_deferred_value_of` (`registry.rs`) once every
// class/enum is indexed, reusing the enum's already-built `->value`
// property type rather than recomputing it.

#[test]
fn value_of_on_a_backed_enum_resolves_to_the_real_union_of_case_values() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /** @param value-of<Suit> $x */
            function f($x): void {
                assertType("'H'|'S'", $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn value_of_on_a_backed_enum_resolves_correctly_inside_a_namespace() {
    // `NameContext::resolve_names_in_type`'s generic `Object` arm
    // namespace-qualifies every `Object` name it doesn't special-case - a
    // namespace-less fixture never actually exercises this at all
    // (`ctx.resolve` on a bare name with an empty namespace is a no-op),
    // so this needs a real `namespace` statement to catch it:
    // `resolve_names_in_type`'s `"$$valueof"` guard must fire before the
    // sentinel gets mangled into `App\$$valueof`, or resolution never
    // gets a chance to recognize it, producing a bogus `class.notFound`
    // instead of the real resolved type.
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /** @param value-of<Suit> $x */
            function f($x): void {
                assertType("'H'|'S'", $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn value_of_on_a_backed_enum_is_enforced_as_a_real_return_type() {
    // The same resolved type actually gates real diagnostics, not just
    // `assertType` - a wrong-typed `return` is caught the same way any
    // other declared return type mismatch would be.
    let d = diagnostics_for(
        r#"<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            /**
             * @return value-of<Suit>
             */
            function bad(): string {
                return 'not a real case';
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "return.type"));
}

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
fn value_of_on_an_unbacked_enum_resolves_to_never() {
    // Matches real PHPStan's own `ValueOfType::getResult()` exactly: a
    // non-backed enum's cases have no `getBackingValueType()` at all, so
    // the collected value-type list is empty, and it falls to `NeverType`.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum PureCase {
                case A;
                case B;
            }
            /** @param value-of<PureCase> $x */
            function f($x): void {
                assertType('never', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn value_of_on_a_non_enum_class_reference_still_degrades_to_mixed() {
    // Confirmed against real PHPStan's own `ValueOfType::getResult()`:
    // an ordinary (non-enum) class falls to `$this->type->getIterableValueType()`,
    // `Mixed` for a plain class - this project's own already-accepted
    // "can't verify, don't guess" leniency, not a new gap.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class NotAnEnum {}
            /** @param value-of<NotAnEnum> $x */
            function f($x): void {
                assertType('mixed', $x);
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

// A class-constant type reference (`Foo::BAR`, `Foo::PREFIX_*`, `Foo::*`)
// resolves to the union of the matching constants' own types, instead of
// collapsing the whole PHPDoc union to `mixed` (Monolog's
// `Logger::toMonologLevel` `@phpstan-param Level|LogLevel::* $level`).
#[test]
fn a_class_constant_wildcard_keeps_its_sibling_union_members() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Level: int { case Debug = 100; case Info = 200; }
            class LogLevel { const DEBUG = 'debug'; const INFO = 'info'; const OTHER = 1; }
            /** @phpstan-param Level|LogLevel::* $level */
            function b(string|Level $level): void {
                assertType("Level|'debug'|'info'|1", $level);
            }
            /** @param LogLevel::D* $level */
            function c($level): void { assertType("'debug'", $level); }
            /** @param Level::Debug|LogLevel::INFO $level */
            function d($level): void { assertType("Level::Debug|'info'", $level); }
            class Self_ {
                const RFC_A = 'a';
                const RFC_B = 'b';
                /** @param self::RFC_* $rfc */
                public function e($rfc): void { assertType("'a'|'b'", $rfc); }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn value_of_a_typed_class_constant_resolves_its_value_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Level {
                /** @var list<100|200> */
                const VALUES = [100, 200];
            }
            /** @param value-of<Level::VALUES> $level */
            function f(int $level): void { assertType('100|200', $level); }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// A class constant's own value is typed exactly (real PHPStan's
// `ConstantArrayType`), including references to other constants,
// resolved through chains; a reference cycle degrades to `mixed`.
#[test]
fn class_constant_values_are_typed_exactly_including_references() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Level: int {
                case Debug = 100;
                case Info = 200;
                const VALUES = [100, 200];
                const NAMES = ['debug' => self::Debug, 'info' => self::Info];
            }
            class Other { const C = 'c'; }
            class Consts {
                const A = self::B;
                const B = Other::C;
                const LIST = [self::A, 'x', 7 => true, false];
                const EMPTY = [];
                const LOOP_A = self::LOOP_B;
                const LOOP_B = self::LOOP_A;
            }
            /** @param value-of<Level::VALUES> $level */
            function f(int $level): void { assertType('100|200', $level); }
            function g(): void {
                assertType("'c'", Consts::A);
                assertType("array{0: 'c', 1: 'x', 7: true, 8: false}", Consts::LIST);
                assertType('array{}', Consts::EMPTY);
                assertType("array{debug: Level::Debug, info: Level::Info}", Level::NAMES);
                assertType('mixed', Consts::LOOP_A);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn a_class_constant_wildcard_sees_inherited_and_interface_constants() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            interface HasCodes { const CODE_I = 'i'; }
            class Base { const CODE_A = 'a'; const CODE_B = 'b'; }
            class Child extends Base implements HasCodes {
                const CODE_B = 'overridden';
                const CODE_C = 'c';
                /** @param self::CODE_* $code */
                public function f($code): void {
                    assertType("'a'|'overridden'|'c'|'i'", $code);
                }
            }
            /** @param Child::CODE_* $code */
            function g($code): void { assertType("'a'|'overridden'|'c'|'i'", $code); }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}
