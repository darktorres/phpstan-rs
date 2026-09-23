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
    let unknown_elements = Type::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type::Mixed(false)),
    );
    let ints = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int));
    // The lenient direction that types::is_subtype deliberately does
    // NOT take (see its own array-covariance test) - is_assignable is
    // the practical, checker-facing layer where "we don't know" must
    // not read as "definitely incompatible".
    assert!(r.is_assignable(&unknown_elements, &ints));
    // A genuinely known, different element type is still caught.
    let strings = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::String));
    assert!(!r.is_assignable(&strings, &ints));
}

#[test]
fn is_assignable_treats_unknown_array_as_compatible_with_a_shape() {
    let r = Registry::default();
    let unknown = Type::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type::Mixed(false)),
    );
    let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
    assert!(r.is_assignable(&unknown, &shape));
    // A genuinely known, homogeneous array still doesn't satisfy an
    // arbitrary shape.
    let ints = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int));
    assert!(!r.is_assignable(&ints, &shape));
}

// An `ArrayShape`'s aggregate key type (int-vs-string, mirroring
// PHPStan's `ConstantArrayType::getIterableKeyType()`) must actually
// agree with a `Type::Array`'s declared key type, in both directions -
// not just each field's/element's value type.

#[test]
fn a_string_keyed_array_type_is_not_assignable_to_an_int_keyed_shape() {
    let r = Registry::default();
    let strings = Type::Array(Box::new(Type::String), Box::new(Type::Mixed(false)));
    let shape = Type::ArrayShape(vec![("0".to_string(), Type::Int, false)]);
    assert!(!r.is_assignable(&strings, &shape));
}

#[test]
fn an_int_keyed_array_type_is_not_assignable_to_a_string_keyed_shape() {
    let r = Registry::default();
    let ints = Type::Array(Box::new(Type::Int), Box::new(Type::Mixed(false)));
    let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
    assert!(!r.is_assignable(&ints, &shape));
}

#[test]
fn a_string_keyed_shape_is_not_assignable_to_an_int_keyed_array_type() {
    let r = Registry::default();
    let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
    let ints = Type::Array(Box::new(Type::Int), Box::new(Type::String));
    assert!(!r.is_assignable(&shape, &ints));
}

#[test]
fn an_int_keyed_shape_is_still_assignable_to_a_matching_int_keyed_array_type() {
    let r = Registry::default();
    let shape = Type::ArrayShape(vec![("0".to_string(), Type::String, false)]);
    let ints = Type::Array(Box::new(Type::Int), Box::new(Type::String));
    assert!(r.is_assignable(&shape, &ints));
}

#[test]
fn strict_mixed_arrays_opts_out_of_the_unknown_array_element_leniency() {
    let mut r = Registry {
        strict_mixed_arrays: true,
        ..Registry::default()
    };
    let unknown_elements = Type::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type::Mixed(false)),
    );
    let ints = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int));
    assert!(!r.is_assignable(&unknown_elements, &ints));
    // Still correctly permissive when the element types genuinely
    // match - strictness only removes the "we don't know" free pass,
    // it doesn't break real assignability.
    assert!(r.is_assignable(
        &Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        &ints
    ));

    r.strict_mixed_arrays = false;
    assert!(r.is_assignable(&unknown_elements, &ints));
}

#[test]
fn strict_mixed_arrays_opts_out_of_the_unknown_array_shape_leniency() {
    let r = Registry {
        strict_mixed_arrays: true,
        ..Registry::default()
    };
    let unknown = Type::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type::Mixed(false)),
    );
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

#[test]
fn a_real_enum_class_string_satisfies_enum_string() {
    let r = build_registry(
        r#"<?php
            enum Suit { case Hearts; }
            class Foo {}
        "#,
    );
    let enum_string = Type::ClassString(Box::new(Type::Object("UnitEnum".to_string(), vec![])));
    assert!(r.is_assignable(
        &Type::ClassString(Box::new(Type::Object("Suit".to_string(), vec![]))),
        &enum_string
    ));
    assert!(!r.is_assignable(
        &Type::ClassString(Box::new(Type::Object("Foo".to_string(), vec![]))),
        &enum_string
    ));
}

#[test]
fn interface_string_and_trait_string_check_the_real_class_kind() {
    let r = build_registry(
        r#"<?php
            interface Shape2 {}
            trait Helper2 {}
            class Plain2 {}
        "#,
    );
    let interface_string =
        Type::ClassString(Box::new(Type::Object("$$interface".to_string(), vec![])));
    let trait_string = Type::ClassString(Box::new(Type::Object("$$trait".to_string(), vec![])));

    assert!(r.is_assignable(
        &Type::ClassString(Box::new(Type::Object("Shape2".to_string(), vec![]))),
        &interface_string
    ));
    assert!(!r.is_assignable(
        &Type::ClassString(Box::new(Type::Object("Plain2".to_string(), vec![]))),
        &interface_string
    ));
    assert!(r.is_assignable(
        &Type::ClassString(Box::new(Type::Object("Helper2".to_string(), vec![]))),
        &trait_string
    ));
    assert!(!r.is_assignable(
        &Type::ClassString(Box::new(Type::Object("Plain2".to_string(), vec![]))),
        &trait_string
    ));
}

// `accepts_at_configured_level`'s level-aware leniency.

#[test]
fn accepts_at_configured_level_matches_is_assignable_when_never_configured() {
    let r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    assert_eq!(r.configured_level, None);
    assert_eq!(
        r.accepts_at_configured_level(&nullable_string, &Type::String),
        r.is_assignable(&nullable_string, &Type::String)
    );
    assert!(!r.accepts_at_configured_level(&nullable_string, &Type::String));
}

#[test]
fn accepts_at_configured_level_matches_is_assignable_at_the_strict_threshold() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(8);
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    assert!(!r.accepts_at_configured_level(&nullable_string, &Type::String));
}

// The load-bearing case: a nullable-typed actual value (`?string`, the
// single most common PHP nullability idiom) must be accepted by a
// plain, non-nullable declared type below level 8, where plain
// `is_assignable` (and a naive "bail to is_assignable whenever `sup` is
// a Union too" fast path) both reject it.
#[test]
fn a_nullable_actual_type_is_accepted_by_a_non_nullable_declared_type_below_level_eight() {
    let mut r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    for level in 0..8 {
        r.configured_level = Some(level);
        assert!(
            r.accepts_at_configured_level(&nullable_string, &Type::String),
            "level {level} should accept a nullable actual type"
        );
    }
}

// The real, common shape every one of this phase's 8 call sites actually
// sees: a nullable *declared* type (`sup = Union([String, Null])`, e.g. a
// `?string`-typed parameter) accepting a plain non-nullable actual
// argument - already true via plain `is_assignable`'s own unconditional
// `.any()` arm, but must keep working once routed through the new
// recursive Union-vs-Union-aware method instead of silently regressing.
#[test]
fn a_plain_actual_value_still_satisfies_a_nullable_declared_type() {
    let mut r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    r.configured_level = Some(5);
    assert!(r.accepts_at_configured_level(&Type::String, &nullable_string));
}

#[test]
fn a_definite_null_actual_value_is_still_rejected_by_a_non_nullable_declared_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(5);
    assert!(!r.accepts_at_configured_level(&Type::Null, &Type::String));
}

#[test]
fn a_nullable_actual_type_is_not_accepted_by_a_definite_null_declared_type() {
    let mut r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    r.configured_level = Some(5);
    // Stripping Null out of `sub` must never happen when `sup` is itself
    // bare `Type::Null` - a `?string` actual value still doesn't
    // definitely satisfy a target that only ever accepts `null`.
    assert!(!r.accepts_at_configured_level(&nullable_string, &Type::Null));
}

#[test]
fn a_union_vs_union_pair_recurses_correctly_below_the_strict_threshold() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(5);
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    assert!(r.accepts_at_configured_level(&nullable_string, &nullable_string));
    let nullable_int = Type::Union(vec![Type::Int, Type::Null]);
    // A nullable string's non-null member (`string`) doesn't satisfy a
    // nullable int's declared type at all, even once Null is stripped
    // from the actual side.
    assert!(!r.accepts_at_configured_level(&nullable_string, &nullable_int));
}

// `checkExplicitMixed`/`checkImplicitMixed` - real PHPStan's own level
// 9/10 (`accepts_at_configured_level`'s own `is_assignable_at_level`).

#[test]
fn below_level_nine_an_explicit_mixed_value_is_accepted_by_a_concrete_declared_type() {
    let mut r = build_registry("<?php");
    for level in 0..9 {
        r.configured_level = Some(level);
        assert!(
            r.accepts_at_configured_level(&Type::Mixed(true), &Type::String),
            "level {level} should still accept an explicit mixed value"
        );
    }
}

#[test]
fn at_level_nine_an_explicit_mixed_value_is_rejected_by_a_concrete_declared_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(9);
    assert!(!r.accepts_at_configured_level(&Type::Mixed(true), &Type::String));
}

#[test]
fn at_level_nine_an_implicit_mixed_value_is_still_accepted() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(9);
    // Level 9 is `checkExplicitMixed` only - `checkImplicitMixed` needs
    // level 10, real PHPStan's own cumulative-but-distinct pair.
    assert!(r.accepts_at_configured_level(&Type::Mixed(false), &Type::String));
}

#[test]
fn at_level_ten_both_explicit_and_implicit_mixed_values_are_rejected() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    assert!(!r.accepts_at_configured_level(&Type::Mixed(true), &Type::String));
    assert!(!r.accepts_at_configured_level(&Type::Mixed(false), &Type::String));
}

#[test]
fn at_level_ten_a_mixed_value_is_still_accepted_by_a_mixed_declared_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    assert!(r.accepts_at_configured_level(&Type::Mixed(true), &Type::Mixed(false)));
    assert!(r.accepts_at_configured_level(&Type::Mixed(false), &Type::Mixed(true)));
}

#[test]
fn at_level_ten_a_mixed_value_inside_a_union_is_still_rejected_by_a_concrete_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    let string_or_mixed = Type::Union(vec![Type::String, Type::Mixed(false)]);
    assert!(!r.accepts_at_configured_level(&string_or_mixed, &Type::String));
}

#[test]
fn at_level_ten_ordinary_rejection_is_unaffected() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    // A real mismatch (not mixed-driven at all) must still be rejected -
    // this mechanism only ever turns an acceptance into a rejection,
    // never masks a genuine one.
    assert!(!r.accepts_at_configured_level(&Type::Int, &Type::String));
}

// `check_benevolent_union_types` - real PHPStan's own standalone
// `checkBenevolentUnionTypes` parameter (`registry::Registry::
// check_benevolent_union_types`'s own doc comment).

#[test]
fn a_benevolent_union_is_accepted_leniently_by_default() {
    let r = build_registry("<?php");
    assert!(!r.check_benevolent_union_types);
    // Only `Int` actually matches `Type::String` here - the default,
    // unconfigured leniency accepts because *any* member does.
    assert!(r.accepts_at_configured_level(&Type::benevolent_array_key(), &Type::String));
}

#[test]
fn a_benevolent_union_is_rejected_strictly_once_opted_in() {
    let mut r = build_registry("<?php");
    r.check_benevolent_union_types = true;
    // Once opted in, *every* member of the wrapped union must satisfy
    // `sup` - `Int` doesn't satisfy `Type::String`, so this now fails.
    assert!(!r.accepts_at_configured_level(&Type::benevolent_array_key(), &Type::String));
}

#[test]
fn a_benevolent_union_still_accepted_once_opted_in_when_every_member_matches() {
    let mut r = build_registry("<?php");
    r.check_benevolent_union_types = true;
    let int_or_string = Type::Intersection(vec![
        Type::Union(vec![Type::Int, Type::String]),
        Type::Accessory(crate::types::AccessoryKind::Benevolent),
    ]);
    let int_or_string_target = Type::Union(vec![Type::Int, Type::String]);
    assert!(r.accepts_at_configured_level(&int_or_string, &int_or_string_target));
}

#[test]
fn opting_in_never_affects_is_assignable_directly() {
    // Real PHPStan's own `checkBenevolentUnionTypes` only ever changes
    // `RuleLevelHelper::accepts()` - never a bare `isSuperTypeOf()`/
    // `accepts()` call outside it (narrowing, generics bound-checking,
    // ...). Mirrored here: opting in must never touch this project's
    // own general-purpose `is_assignable`, only `accepts_at_configured_
    // level`.
    let mut r = build_registry("<?php");
    r.check_benevolent_union_types = true;
    assert!(r.is_assignable(&Type::benevolent_array_key(), &Type::String));
}

#[test]
fn variance_composes_through_a_nested_covariant_generic_argument() {
    let r = build_registry(
        r#"<?php
            /** @template-covariant T */
            class Producer {}
            /** @template-covariant T */
            class Container {}
            class Animal {}
            class Cat extends Animal {}
        "#,
    );
    let producer_of = |name: &str| Type::Object("Producer".to_string(), vec![obj(name)]);
    let container_of = |t: Type| Type::Object("Container".to_string(), vec![t]);
    assert!(r.is_assignable(
        &container_of(producer_of("Cat")),
        &container_of(producer_of("Animal"))
    ));
    assert!(!r.is_assignable(
        &container_of(producer_of("Animal")),
        &container_of(producer_of("Cat"))
    ));
}

#[test]
fn variance_flips_through_a_nested_contravariant_generic_argument() {
    let r = build_registry(
        r#"<?php
            /** @template-contravariant T */
            class Consumer {}
            /** @template-covariant T */
            class Container {}
            class Animal {}
            class Cat extends Animal {}
        "#,
    );
    let consumer_of = |name: &str| Type::Object("Consumer".to_string(), vec![obj(name)]);
    let container_of = |t: Type| Type::Object("Container".to_string(), vec![t]);
    // Consumer<Animal> <: Consumer<Cat> (contravariant), so
    // Container<Consumer<Animal>> <: Container<Consumer<Cat>> too, once
    // the inner slot's own contravariance is composed through the outer
    // covariant one rather than inherited as covariant by default.
    assert!(r.is_assignable(
        &container_of(consumer_of("Animal")),
        &container_of(consumer_of("Cat"))
    ));
    assert!(!r.is_assignable(
        &container_of(consumer_of("Cat")),
        &container_of(consumer_of("Animal"))
    ));
}

fn obj(name: &str) -> Type {
    Type::Object(name.to_string(), Vec::new())
}

#[test]
fn a_doc_class_the_registry_never_saw_is_kept_on_a_native_interface_hint() {
    let r = build_registry(
        r#"<?php
            namespace App;
            interface Node {}
            class Rule {
                /** @param \Vendor\CollectedDataNode $node */
                public function processNode(Node $node): void {}
            }
            "#,
    );
    let ty = &r.classes["App\\Rule"].methods["processNode"].params[0].ty;
    assert_eq!(ty.to_string(), "Vendor\\CollectedDataNode");
}

#[test]
fn a_generic_argument_naming_an_enum_case_satisfies_a_bound_listing_that_case() {
    let r = build_registry(
        r#"<?php
            namespace ProbeTb;
            enum Id: string { case ARRAY = 'array'; case ITERABLE = 'iterable'; }
            /** @template T of Id */
            class Builtin {}
            /** @template T of Builtin<Id::ARRAY>|Builtin<Id::ITERABLE> */
            class Generic {}
            /** @template T */
            class Coll {}
            /** @extends Coll<Generic<Builtin<Id::ARRAY>>> */
            final class Shape extends Coll {}
            "#,
    );
    let Type::Object(_, args) = &r.classes["ProbeTb\\Shape"].extends_args[0] else {
        panic!("expected a generic parent");
    };
    let case = |c: &str| Type::EnumCase("ProbeTb\\Id".into(), c.into());
    assert!(r.is_assignable(&case("ARRAY"), &case("ARRAY")));
    assert!(!r.is_assignable(&case("ARRAY"), &case("ITERABLE")));
    assert!(r.is_assignable(
        &args[0],
        &r.classes["ProbeTb\\Generic"].template_bounds["T"]
    ));
}
