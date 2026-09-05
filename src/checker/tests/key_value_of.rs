//! Phase 80 (partial slice): `value-of<T>`/`key-of<T>` - real PHPStan's
//! own `ValueOfType`/`KeyOfType`, both `LateResolvableType`s this
//! project has no lazy/deferred type-resolution mechanism to fully
//! replicate. Only the case answerable immediately with no registry
//! access - `T` itself already a real `array`/`array{...}` type - is
//! resolved precisely; anything else (a class/enum reference needing a
//! registry lookup) degrades to `Mixed` rather than the bogus,
//! namespace-qualified "class name" it used to mangle into (a real,
//! active `class.notFound` false positive on every use of this syntax).

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
fn value_of_on_a_class_or_enum_reference_no_longer_produces_a_bogus_class_not_found() {
    // The registry-dependent case (T naming a real enum) can't be
    // resolved with no registry access at parse time - degrades to
    // Mixed rather than a nonsense class reference.
    let d = diagnostics_for(
        r#"<?php
            enum Suit: string {
                case Hearts = 'H';
            }
            /** @param value-of<Suit> $x */
            function f($x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn key_of_on_a_class_reference_no_longer_produces_a_bogus_class_not_found() {
    let d = diagnostics_for(
        r#"<?php
            class C {}
            /** @param key-of<C> $x */
            function f($x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}
