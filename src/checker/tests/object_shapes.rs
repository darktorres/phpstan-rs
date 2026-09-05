//! Phase 80 (partial slice): `object{name: string, age?: int}` - PHPDoc's
//! structural object shape. Before `Type::ObjectShape` existed, the raw
//! `object{...}` text was mangled into a bogus, namespace-qualified
//! "class name," producing a real, active `class.notFound` false
//! positive on every use of this syntax - not just a missing-precision
//! gap.

use super::helpers::*;

#[test]
fn an_object_shape_no_longer_produces_a_bogus_class_not_found() {
    let d = diagnostics_for(
        r#"<?php
            /** @param object{name: string, age: int} $x */
            function f($x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_object_shape_field_is_resolved_by_property_access() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param object{name: string, age: int} $x */
            function f($x): void {
                assertType('string', $x->name);
                assertType('int', $x->age);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unlisted_property_on_an_object_shape_falls_back_to_mixed_not_an_error() {
    // Same "don't guess a shape is exhaustive" caution `ArrayShape`'s own
    // unlisted keys already get - no new "undefined property" diagnostic.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param object{name: string} $x */
            function f($x): void {
                assertType('mixed', $x->somethingElse);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_real_class_with_matching_public_properties_satisfies_an_object_shape() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public string $label = '';
                public int $x = 0;
                public int $y = 0;
            }
            /** @param object{x: int, y: int} $p */
            function sink($p): void {}
            function f(Point $p): void {
                sink($p);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_real_class_missing_a_required_property_does_not_satisfy_an_object_shape() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public int $x = 0;
            }
            /** @param object{x: int, y: int} $p */
            function sink($p): void {}
            function f(Point $p): void {
                sink($p);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_missing_optional_field_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public int $x = 0;
            }
            /** @param object{x: int, y?: int} $p */
            function sink($p): void {}
            function f(Point $p): void {
                sink($p);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_mismatched_property_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public string $x = '';
            }
            /** @param object{x: int} $p */
            function sink($p): void {}
            function f(Point $p): void {
                sink($p);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn stdclass_and_bare_object_always_satisfy_an_object_shape() {
    // Neither of these has real, statically-known properties this
    // project tracks - don't guess wrong, matching Array(Mixed)'s own
    // established leniency for the identical situation.
    let d = diagnostics_for(
        r#"<?php
            /** @param object{x: int, y: int} $p */
            function sink($p): void {}
            function f(stdClass $s, object $o): void {
                sink($s);
                sink($o);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_object_shape_satisfies_the_bare_native_object_hint() {
    let d = diagnostics_for(
        r#"<?php
            function sink(object $o): void {}
            /** @param object{x: int} $p */
            function f($p): void {
                sink($p);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_richer_object_shape_satisfies_a_looser_one() {
    // sink() only requires `x` - f()'s own `$p`, typed with an extra `y`
    // field, still satisfies that (extra fields are ignored, the same
    // "structural, not exact" spirit `ArrayShape`'s own identical rule
    // documents).
    let d = diagnostics_for(
        r#"<?php
            /** @param object{x: int} $p */
            function sink($p): void {}
            /** @param object{x: int, y: int} $p */
            function f($p): void {
                sink($p);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_looser_object_shape_does_not_satisfy_a_stricter_one() {
    let d = diagnostics_for(
        r#"<?php
            /** @param object{x: int, y: int} $p */
            function sink($p): void {}
            /** @param object{x: int} $p */
            function f($p): void {
                sink($p);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}
