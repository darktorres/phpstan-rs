//! Opt-in strict `checkDynamicProperties` behavior inside `isset()`/
//! `empty()`/`??`/`??=` (PHASE_BACKLOG Phase 102 W1).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            class C {}
            function f(C $c): bool {
                return isset($c->missing);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn isset_on_an_unknown_property_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            class C {}
            function f(C $c): bool {
                return isset($c->missing);
            }
        "#,
        |r| r.check_dynamic_properties = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn coalesce_on_an_unknown_property_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            class C {}
            function f(C $c): mixed {
                return $c->missing ?? 'default';
            }
        "#,
        |r| r.check_dynamic_properties = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn isset_on_a_real_property_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class C { public int $x = 1; }
            function f(C $c): bool {
                return isset($c->x);
            }
        "#,
        |r| r.check_dynamic_properties = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn isset_on_a_bare_undefined_variable_is_still_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): bool {
                return isset($undefined);
            }
        "#,
        |r| r.check_dynamic_properties = true,
    );
    assert!(d.is_empty(), "got: {d:?}");
}

#[test]
fn isset_on_stdclass_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(\stdClass $c): bool {
                return isset($c->anything);
            }
        "#,
        |r| r.check_dynamic_properties = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.notFound"),
        "got: {d:?}"
    );
}
