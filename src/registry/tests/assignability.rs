//! Type assignability checks driven by the class hierarchy.

use super::super::*;
use super::helpers::*;

#[test]
fn is_assignable_uses_class_hierarchy() {
    let r = build_registry(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
        "#,
    );
    assert!(r.is_assignable(
        &Type::Object("Circle".to_string(), Vec::new()),
        &Type::Object("Shape".to_string(), Vec::new())
    ));
    assert!(!r.is_assignable(
        &Type::Object("Shape".to_string(), Vec::new()),
        &Type::Object("Circle".to_string(), Vec::new())
    ));
}

#[test]
fn class_string_is_assignable_follows_class_hierarchy_not_name_equality() {
    let r = build_registry(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            class Square {}
        "#,
    );
    let cs = |name: &str| Type::ClassString(Box::new(Type::Object(name.to_string(), Vec::new())));
    assert!(r.is_assignable(&cs("Circle"), &cs("Shape")));
    assert!(!r.is_assignable(&cs("Square"), &cs("Shape")));
}

#[test]
fn any_class_instance_is_assignable_to_the_native_object_type() {
    let r = build_registry("<?php class Foo {}");
    assert!(r.is_assignable(
        &Type::Object("Foo".to_string(), Vec::new()),
        &Type::Object("object".to_string(), Vec::new())
    ));
    // Not the other way around - a plain `object` isn't known to be
    // any particular class.
    assert!(!r.is_assignable(
        &Type::Object("object".to_string(), Vec::new()),
        &Type::Object("Foo".to_string(), Vec::new())
    ));
}

#[test]
fn is_assignable_treats_unknown_array_element_type_as_compatible() {
    let r = Registry::default();
    let unknown_elements = Type::Array(Box::new(Type::Mixed));
    let ints = Type::Array(Box::new(Type::Int));
    // The lenient direction that types::is_subtype deliberately does
    // NOT take (see its own array-covariance test) - is_assignable is
    // the practical, checker-facing layer where "we don't know" must
    // not read as "definitely incompatible".
    assert!(r.is_assignable(&unknown_elements, &ints));
    // A genuinely known, different element type is still caught.
    let strings = Type::Array(Box::new(Type::String));
    assert!(!r.is_assignable(&strings, &ints));
}

#[test]
fn is_assignable_treats_unknown_array_as_compatible_with_a_shape() {
    let r = Registry::default();
    let unknown = Type::Array(Box::new(Type::Mixed));
    let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
    assert!(r.is_assignable(&unknown, &shape));
    // A genuinely known, homogeneous array still doesn't satisfy an
    // arbitrary shape.
    let ints = Type::Array(Box::new(Type::Int));
    assert!(!r.is_assignable(&ints, &shape));
}

#[test]
fn strict_mixed_arrays_opts_out_of_the_unknown_array_element_leniency() {
    let mut r = Registry {
        strict_mixed_arrays: true,
        ..Registry::default()
    };
    let unknown_elements = Type::Array(Box::new(Type::Mixed));
    let ints = Type::Array(Box::new(Type::Int));
    assert!(!r.is_assignable(&unknown_elements, &ints));
    // Still correctly permissive when the element types genuinely
    // match - strictness only removes the "we don't know" free pass,
    // it doesn't break real assignability.
    assert!(r.is_assignable(&Type::Array(Box::new(Type::Int)), &ints));

    r.strict_mixed_arrays = false;
    assert!(r.is_assignable(&unknown_elements, &ints));
}

#[test]
fn strict_mixed_arrays_opts_out_of_the_unknown_array_shape_leniency() {
    let r = Registry {
        strict_mixed_arrays: true,
        ..Registry::default()
    };
    let unknown = Type::Array(Box::new(Type::Mixed));
    let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
    assert!(!r.is_assignable(&unknown, &shape));
}

#[test]
fn is_assignable_accepts_a_value_matching_any_union_member_on_the_supertype_side() {
    // Coverage measurement found this branch (sup is a Union - not to
    // be confused with the sub-is-a-Union branch just above, which is
    // well covered by ordinary nullable-param tests) had zero
    // executions. Manually verified correct against real code first.
    let r = Registry::default();
    let int_or_string = Type::Union(vec![Type::Int, Type::String]);
    assert!(r.is_assignable(&Type::Int, &int_or_string));
    assert!(r.is_assignable(&Type::String, &int_or_string));
    assert!(!r.is_assignable(&Type::Bool, &int_or_string));
}

#[test]
fn is_assignable_handles_intersection_types_on_either_side() {
    // Coverage measurement found both Intersection branches had zero
    // executions - PHP 8.1+ intersection types (`Countable&Iterator`)
    // are real, if uncommon, syntax. Manually verified correct
    // against real code (an intersection-typed value satisfies a
    // supertype matching any one of its members; a plain single-
    // interface value does NOT satisfy an intersection requiring
    // more than it has) before locking in here.
    let r = build_registry(
        r#"<?php
            interface Countable2 {}
            interface Iterator2 {}
        "#,
    );
    let both = Type::Intersection(vec![
        Type::Object("Countable2".to_string(), Vec::new()),
        Type::Object("Iterator2".to_string(), Vec::new()),
    ]);
    let countable_only = Type::Object("Countable2".to_string(), Vec::new());

    // sub is the intersection: satisfies a supertype matching any one
    // of its members.
    assert!(r.is_assignable(&both, &countable_only));
    // sup is the intersection: a value only known to be one of the
    // two doesn't satisfy a requirement for both.
    assert!(!r.is_assignable(&countable_only, &both));
}
