//! `property.defaultValue` - real PHPStan's own `Rules/Properties/
//! DefaultValueTypesAssignedToPropertiesRule.php`: a property's own
//! default value expression's type must be assignable to its declared
//! (writable) type.

use super::helpers::*;

#[test]
fn an_incompatible_scalar_default_on_a_typed_property_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x = "hello";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_compatible_scalar_default_on_a_typed_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x = 5;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_null_default_on_a_nullable_typed_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public ?int $x = null;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_null_default_on_an_untyped_property_is_never_flagged_even_with_a_narrow_var_tag() {
    // Real PHP quietly defaults every untyped property to `null`
    // regardless of its own `@var` tag - explicit `= null` is always
    // legal there.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @var int */
                public $x = null;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_null_incompatible_default_on_an_untyped_property_is_still_flagged() {
    // Unlike `null`, any other default on an untyped property is still
    // checked normally against its own `@var` tag.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @var int */
                public $x = "still bad";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_static_property_gets_the_static_wording() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public static string $x = 5;
            }
        "#,
    );
    let hit = d
        .iter()
        .find(|x| x.identifier == "property.defaultValue")
        .unwrap_or_else(|| panic!("got: {d:?}"));
    assert!(
        hit.message.starts_with("Static property"),
        "got: {}",
        hit.message
    );
}

#[test]
fn a_matching_enum_case_default_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status { case A; case B; }
            class Foo {
                public Status $s = Status::A;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_property_with_no_default_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.defaultValue"),
        "got: {d:?}"
    );
}
