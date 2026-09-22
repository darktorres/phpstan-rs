//! Real PHPStan's own level 9 `checkExplicitMixed`/level 10
//! `checkImplicitMixed` (Phase 84, Stage 3) - end-to-end, through a real
//! `Checker::check_program` pass, not just `Registry::
//! accepts_at_configured_level`'s own unit tests
//! (`registry/tests/assignability.rs`).

use super::helpers::*;

#[test]
fn below_level_nine_a_native_mixed_parameter_is_accepted_by_a_typed_one() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(mixed $m): void {
                g($m);
            }
            function g(string $s): void {}
        "#,
        |_| {},
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn at_level_nine_a_native_mixed_parameter_is_rejected_by_a_typed_one() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(mixed $m): void {
                g($m);
            }
            function g(string $s): void {}
        "#,
        |r| r.configured_level = Some(9),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("mixed given")),
        "got: {d:?}"
    );
}

#[test]
fn at_level_nine_an_implicit_mixed_value_is_still_accepted() {
    // No type at all (not even a native `mixed` hint) is *implicit*
    // mixed - `checkImplicitMixed` needs level 10, not 9.
    let d = diagnostics_for_with(
        r#"<?php
            function f($m): void {
                g($m);
            }
            function g(string $s): void {}
        "#,
        |r| r.configured_level = Some(9),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn at_level_ten_an_implicit_mixed_value_is_also_rejected() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($m): void {
                g($m);
            }
            function g(string $s): void {}
        "#,
        |r| r.configured_level = Some(10),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("mixed given")),
        "got: {d:?}"
    );
}

#[test]
fn at_level_ten_a_mixed_value_is_still_accepted_by_a_mixed_declared_parameter() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(mixed $m): void {
                g($m);
            }
            function g(mixed $x): void {}
        "#,
        |r| r.configured_level = Some(10),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn at_level_ten_an_explicit_mixed_property_assignment_is_rejected() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                public string $s = '';
                function set(mixed $m): void {
                    $this->s = $m;
                }
            }
        "#,
        |r| r.configured_level = Some(10),
    );
    assert!(
        d.iter().any(|x| x.identifier == "assign.propertyType"),
        "got: {d:?}"
    );
}

#[test]
fn at_level_ten_an_explicit_mixed_return_value_is_rejected() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(mixed $m): string {
                return $m;
            }
        "#,
        |r| r.configured_level = Some(10),
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "got: {d:?}"
    );
}

#[test]
fn at_level_ten_ordinary_type_mismatches_are_unaffected() {
    // A real, ordinary mismatch (nothing to do with `mixed` at all) must
    // keep firing the same as always once this opt-in level is on.
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                g(1);
            }
            function g(string $s): void {}
        "#,
        |r| r.configured_level = Some(10),
    );
    assert!(d.iter().any(|x| x.identifier == "argument.type"));
}

#[test]
fn at_level_nine_is_array_narrowed_value_is_explicit_mixed() {
    // Real PHPStan's own `IsArrayFunctionTypeSpecifyingExtension`
    // narrows to `ArrayType(MixedType(true), MixedType(true))` -
    // explicit, so this fires at level 9 already, not only at level 10.
    let d = diagnostics_for_with(
        r#"<?php
            function f($m): void {
                if (is_array($m)) {
                    g($m[0]);
                }
            }
            function g(string $s): void {}
        "#,
        |r| r.configured_level = Some(9),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("mixed given")),
        "got: {d:?}"
    );
}

#[test]
fn at_level_nine_a_superglobal_element_is_explicit_mixed() {
    // Real PHPStan's own `MutatingScope::getVariableType`'s
    // `isGlobalVariable` branch resolves an unset superglobal element to
    // `ArrayType(BenevolentUnionType([Int, String]), MixedType(true))` -
    // explicit, so this fires at level 9 already, not only at level 10.
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                g($_GET['id']);
            }
            function g(string $s): void {}
        "#,
        |r| r.configured_level = Some(9),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("mixed given")),
        "got: {d:?}"
    );
}
