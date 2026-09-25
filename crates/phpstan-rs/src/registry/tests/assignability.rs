//! Type assignability checks driven by the class hierarchy.

use super::super::*;
use super::helpers::*;
use types::Shape;

#[test]
fn is_assignable_uses_class_hierarchy() {
    let r = build_registry(
        r#"<?php
            interface Shape {}
            class Circle implements Shape {}
        "#,
    );
    assert!(
        r.accepts(
            &Type::Object("Shape".into(), Vec::new()),
            &Type::Object("Circle".into(), Vec::new())
        )
        .yes()
    );
    assert!(
        !r.accepts(
            &Type::Object("Circle".into(), Vec::new()),
            &Type::Object("Shape".into(), Vec::new())
        )
        .yes()
    );
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
    let cs = |name: &str| Type::ClassString(Box::new(Type::Object(name.into(), Vec::new())));
    assert!(r.accepts(&cs("Shape"), &cs("Circle")).yes());
    assert!(!r.accepts(&cs("Shape"), &cs("Square")).yes());
}

#[test]
fn any_class_instance_is_assignable_to_the_native_object_type() {
    let r = build_registry("<?php class Foo {}");
    assert!(
        r.accepts(
            &Type::ObjectWithoutClass,
            &Type::Object("Foo".into(), Vec::new())
        )
        .yes()
    );
    // Not the other way around - a plain `object` isn't known to be
    // any particular class.
    assert!(
        !r.accepts(
            &Type::Object("Foo".into(), Vec::new()),
            &Type::ObjectWithoutClass
        )
        .yes()
    );
}

#[test]
fn is_assignable_treats_unknown_array_element_type_as_compatible() {
    let r = Registry::default();
    let unknown_elements = Type::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type::Mixed(false)),
    );
    let ints = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int));
    // The lenient direction that Relation::subtype deliberately does
    // NOT take (see its own array-covariance test) - accepts is
    // the practical, checker-facing layer where "we don't know" must
    // not read as "definitely incompatible".
    assert!(r.accepts(&ints, &unknown_elements).yes());
    // A genuinely known, different element type is still caught.
    let strings = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::String));
    assert!(!r.accepts(&ints, &strings).yes());
}

#[test]
fn is_assignable_treats_unknown_array_as_compatible_with_a_shape() {
    let r = Registry::default();
    let unknown = Type::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type::Mixed(false)),
    );
    let shape = Type::ArrayShape(Shape::from_named(vec![(
        "name".to_string(),
        Type::String,
        false,
    )]));
    assert!(r.accepts(&shape, &unknown).yes());
    // A genuinely known, homogeneous array still doesn't satisfy an
    // arbitrary shape.
    let ints = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int));
    assert!(!r.accepts(&shape, &ints).yes());
}

// An `ArrayShape`'s aggregate key type (int-vs-string, mirroring
// PHPStan's `ConstantArrayType::getIterableKeyType()`) must actually
// agree with a `Type::Array`'s declared key type, in both directions -
// not just each field's/element's value type.

#[test]
fn a_string_keyed_array_type_is_not_assignable_to_an_int_keyed_shape() {
    let r = Registry::default();
    let strings = Type::Array(Box::new(Type::String), Box::new(Type::Mixed(false)));
    let shape = Type::ArrayShape(Shape::from_named(vec![("0".to_string(), Type::Int, false)]));
    assert!(!r.accepts(&shape, &strings).yes());
}

#[test]
fn an_int_keyed_array_type_is_not_assignable_to_a_string_keyed_shape() {
    let r = Registry::default();
    let ints = Type::Array(Box::new(Type::Int), Box::new(Type::Mixed(false)));
    let shape = Type::ArrayShape(Shape::from_named(vec![(
        "name".to_string(),
        Type::String,
        false,
    )]));
    assert!(!r.accepts(&shape, &ints).yes());
}

#[test]
fn a_string_keyed_shape_is_not_assignable_to_an_int_keyed_array_type() {
    let r = Registry::default();
    let shape = Type::ArrayShape(Shape::from_named(vec![(
        "name".to_string(),
        Type::String,
        false,
    )]));
    let ints = Type::Array(Box::new(Type::Int), Box::new(Type::String));
    assert!(!r.accepts(&ints, &shape).yes());
}

#[test]
fn an_int_keyed_shape_is_still_assignable_to_a_matching_int_keyed_array_type() {
    let r = Registry::default();
    let shape = Type::ArrayShape(Shape::from_named(vec![(
        "0".to_string(),
        Type::String,
        false,
    )]));
    let ints = Type::Array(Box::new(Type::Int), Box::new(Type::String));
    assert!(r.accepts(&ints, &shape).yes());
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
    assert!(!r.accepts(&ints, &unknown_elements).yes());
    // Still correctly permissive when the element types genuinely
    // match - strictness only removes the "we don't know" free pass,
    // it doesn't break real assignability.
    assert!(
        r.accepts(
            &ints,
            &Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int))
        )
        .yes()
    );

    r.strict_mixed_arrays = false;
    assert!(r.accepts(&ints, &unknown_elements).yes());
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
    let shape = Type::ArrayShape(Shape::from_named(vec![(
        "name".to_string(),
        Type::String,
        false,
    )]));
    assert!(!r.accepts(&shape, &unknown).yes());
}

#[test]
fn is_assignable_accepts_a_value_matching_any_union_member_on_the_supertype_side() {
    // Coverage measurement found this branch (sup is a Union - not to
    // be confused with the sub-is-a-Union branch just above, which is
    // well covered by ordinary nullable-param tests) had zero
    // executions. Manually verified correct against real code first.
    let r = Registry::default();
    let int_or_string = Type::Union(vec![Type::Int, Type::String]);
    assert!(r.accepts(&int_or_string, &Type::Int).yes());
    assert!(r.accepts(&int_or_string, &Type::String).yes());
    assert!(!r.accepts(&int_or_string, &Type::Bool).yes());
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
        Type::Object("Countable2".into(), Vec::new()),
        Type::Object("Iterator2".into(), Vec::new()),
    ]);
    let countable_only = Type::Object("Countable2".into(), Vec::new());

    // sub is the intersection: satisfies a supertype matching any one
    // of its members.
    assert!(r.accepts(&countable_only, &both).yes());
    // sup is the intersection: a value only known to be one of the
    // two doesn't satisfy a requirement for both.
    assert!(!r.accepts(&both, &countable_only).yes());
}

#[test]
fn a_real_enum_class_string_satisfies_enum_string() {
    let r = build_registry(
        r#"<?php
            enum Suit { case Hearts; }
            class Foo {}
        "#,
    );
    let enum_string = Type::ClassString(Box::new(Type::Object("UnitEnum".into(), vec![])));
    assert!(
        r.accepts(
            &enum_string,
            &Type::ClassString(Box::new(Type::Object("Suit".into(), vec![])))
        )
        .yes()
    );
    assert!(
        !r.accepts(
            &enum_string,
            &Type::ClassString(Box::new(Type::Object("Foo".into(), vec![])))
        )
        .yes()
    );
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
    let interface_string = Type::ClassKindString(types::ClassKind::Interface);
    let trait_string = Type::ClassKindString(types::ClassKind::Trait);

    assert!(
        r.accepts(
            &interface_string,
            &Type::ClassString(Box::new(Type::Object("Shape2".into(), vec![])))
        )
        .yes()
    );
    assert!(
        !r.accepts(
            &interface_string,
            &Type::ClassString(Box::new(Type::Object("Plain2".into(), vec![])))
        )
        .yes()
    );
    assert!(
        r.accepts(
            &trait_string,
            &Type::ClassString(Box::new(Type::Object("Helper2".into(), vec![])))
        )
        .yes()
    );
    assert!(
        !r.accepts(
            &trait_string,
            &Type::ClassString(Box::new(Type::Object("Plain2".into(), vec![])))
        )
        .yes()
    );
}

// `rule_level_accepts`'s level-aware leniency.

#[test]
fn accepts_at_configured_level_matches_is_assignable_when_never_configured() {
    let r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    assert_eq!(r.configured_level, None);
    assert_eq!(
        r.rule_level_accepts(&Type::String, &nullable_string).yes(),
        r.accepts(&Type::String, &nullable_string).yes()
    );
    assert!(!r.rule_level_accepts(&Type::String, &nullable_string).yes());
}

#[test]
fn accepts_at_configured_level_matches_is_assignable_at_the_strict_threshold() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(8);
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    assert!(!r.rule_level_accepts(&Type::String, &nullable_string).yes());
}

// The load-bearing case: a nullable-typed actual value (`?string`, the
// single most common PHP nullability idiom) must be accepted by a
// plain, non-nullable declared type below level 8, where plain
// `accepts` (and a naive "bail to accepts whenever `sup` is
// a Union too" fast path) both reject it.
#[test]
fn a_nullable_actual_type_is_accepted_by_a_non_nullable_declared_type_below_level_eight() {
    let mut r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    for level in 0..8 {
        r.configured_level = Some(level);
        assert!(
            r.rule_level_accepts(&Type::String, &nullable_string).yes(),
            "level {level} should accept a nullable actual type"
        );
    }
}

// The real, common shape every one of this phase's 8 call sites actually
// sees: a nullable *declared* type (`sup = Union([String, Null])`, e.g. a
// `?string`-typed parameter) accepting a plain non-nullable actual
// argument - already true via plain `accepts`'s own unconditional
// `.any()` arm, but must keep working once routed through the new
// recursive Union-vs-Union-aware method instead of silently regressing.
#[test]
fn a_plain_actual_value_still_satisfies_a_nullable_declared_type() {
    let mut r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    r.configured_level = Some(5);
    assert!(r.rule_level_accepts(&nullable_string, &Type::String).yes());
}

#[test]
fn a_definite_null_actual_value_is_still_rejected_by_a_non_nullable_declared_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(5);
    assert!(!r.rule_level_accepts(&Type::String, &Type::Null).yes());
}

#[test]
fn a_nullable_actual_type_is_not_accepted_by_a_definite_null_declared_type() {
    let mut r = build_registry("<?php");
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    r.configured_level = Some(5);
    // Stripping Null out of `sub` must never happen when `sup` is itself
    // bare `Type::Null` - a `?string` actual value still doesn't
    // definitely satisfy a target that only ever accepts `null`.
    assert!(!r.rule_level_accepts(&Type::Null, &nullable_string).yes());
}

#[test]
fn a_union_vs_union_pair_recurses_correctly_below_the_strict_threshold() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(5);
    let nullable_string = Type::Union(vec![Type::String, Type::Null]);
    assert!(
        r.rule_level_accepts(&nullable_string, &nullable_string)
            .yes()
    );
    let nullable_int = Type::Union(vec![Type::Int, Type::Null]);
    // A nullable string's non-null member (`string`) doesn't satisfy a
    // nullable int's declared type at all, even once Null is stripped
    // from the actual side.
    assert!(!r.rule_level_accepts(&nullable_int, &nullable_string).yes());
}

// `checkExplicitMixed`/`checkImplicitMixed` - real PHPStan's own level
// 9/10 (`rule_level_accepts`'s own `accepts_at_level`).

#[test]
fn below_level_nine_an_explicit_mixed_value_is_accepted_by_a_concrete_declared_type() {
    let mut r = build_registry("<?php");
    for level in 0..9 {
        r.configured_level = Some(level);
        assert!(
            r.rule_level_accepts(&Type::String, &Type::Mixed(true))
                .yes(),
            "level {level} should still accept an explicit mixed value"
        );
    }
}

#[test]
fn at_level_nine_an_explicit_mixed_value_is_rejected_by_a_concrete_declared_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(9);
    assert!(
        !r.rule_level_accepts(&Type::String, &Type::Mixed(true))
            .yes()
    );
}

#[test]
fn at_level_nine_an_implicit_mixed_value_is_still_accepted() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(9);
    // Level 9 is `checkExplicitMixed` only - `checkImplicitMixed` needs
    // level 10, real PHPStan's own cumulative-but-distinct pair.
    assert!(
        r.rule_level_accepts(&Type::String, &Type::Mixed(false))
            .yes()
    );
}

#[test]
fn at_level_ten_both_explicit_and_implicit_mixed_values_are_rejected() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    assert!(
        !r.rule_level_accepts(&Type::String, &Type::Mixed(true))
            .yes()
    );
    assert!(
        !r.rule_level_accepts(&Type::String, &Type::Mixed(false))
            .yes()
    );
}

#[test]
fn at_level_ten_a_mixed_value_is_still_accepted_by_a_mixed_declared_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    assert!(
        r.rule_level_accepts(&Type::Mixed(false), &Type::Mixed(true))
            .yes()
    );
    assert!(
        r.rule_level_accepts(&Type::Mixed(true), &Type::Mixed(false))
            .yes()
    );
}

#[test]
fn at_level_ten_a_mixed_value_inside_a_union_is_still_rejected_by_a_concrete_type() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    let string_or_mixed = Type::Union(vec![Type::String, Type::Mixed(false)]);
    assert!(!r.rule_level_accepts(&Type::String, &string_or_mixed).yes());
}

#[test]
fn at_level_ten_ordinary_rejection_is_unaffected() {
    let mut r = build_registry("<?php");
    r.configured_level = Some(10);
    // A real mismatch (not mixed-driven at all) must still be rejected -
    // this mechanism only ever turns an acceptance into a rejection,
    // never masks a genuine one.
    assert!(!r.rule_level_accepts(&Type::String, &Type::Int).yes());
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
    assert!(
        r.rule_level_accepts(&Type::String, &Type::benevolent_array_key())
            .yes()
    );
}

#[test]
fn a_benevolent_union_is_rejected_strictly_once_opted_in() {
    let mut r = build_registry("<?php");
    r.check_benevolent_union_types = true;
    // Once opted in, *every* member of the wrapped union must satisfy
    // `sup` - `Int` doesn't satisfy `Type::String`, so this now fails.
    assert!(
        !r.rule_level_accepts(&Type::String, &Type::benevolent_array_key())
            .yes()
    );
}

#[test]
fn a_benevolent_union_still_accepted_once_opted_in_when_every_member_matches() {
    let mut r = build_registry("<?php");
    r.check_benevolent_union_types = true;
    let int_or_string = Type::BenevolentUnion(vec![Type::Int, Type::String]);
    let int_or_string_target = Type::Union(vec![Type::Int, Type::String]);
    assert!(
        r.rule_level_accepts(&int_or_string_target, &int_or_string)
            .yes()
    );
}

#[test]
fn opting_in_never_affects_is_assignable_directly() {
    // Real PHPStan's own `checkBenevolentUnionTypes` only ever changes
    // `RuleLevelHelper::accepts()` - never a bare `isSuperTypeOf()`/
    // `accepts()` call outside it (narrowing, generics bound-checking,
    // ...). Mirrored here: opting in must never touch this project's
    // own general-purpose `accepts`, only `accepts_at_configured_
    // level`.
    let mut r = build_registry("<?php");
    r.check_benevolent_union_types = true;
    assert!(
        r.accepts(&Type::String, &Type::benevolent_array_key())
            .yes()
    );
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
    let producer_of = |name: &str| Type::Object("Producer".into(), vec![obj(name)]);
    let container_of = |t: Type| Type::Object("Container".into(), vec![t]);
    assert!(
        r.accepts(
            &container_of(producer_of("Animal")),
            &container_of(producer_of("Cat"))
        )
        .yes()
    );
    assert!(
        !r.accepts(
            &container_of(producer_of("Cat")),
            &container_of(producer_of("Animal"))
        )
        .yes()
    );
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
    let consumer_of = |name: &str| Type::Object("Consumer".into(), vec![obj(name)]);
    let container_of = |t: Type| Type::Object("Container".into(), vec![t]);
    // Consumer<Animal> <: Consumer<Cat> (contravariant), so
    // Container<Consumer<Animal>> <: Container<Consumer<Cat>> too, once
    // the inner slot's own contravariance is composed through the outer
    // covariant one rather than inherited as covariant by default.
    assert!(
        r.accepts(
            &container_of(consumer_of("Cat")),
            &container_of(consumer_of("Animal"))
        )
        .yes()
    );
    assert!(
        !r.accepts(
            &container_of(consumer_of("Animal")),
            &container_of(consumer_of("Cat"))
        )
        .yes()
    );
}

fn obj(name: &str) -> Type {
    Type::Object(name.into(), Vec::new())
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
    assert!(r.accepts(&case("ARRAY"), &case("ARRAY")).yes());
    assert!(!r.accepts(&case("ITERABLE"), &case("ARRAY")).yes());
    assert!(
        r.accepts(
            &r.classes["ProbeTb\\Generic"].template_bounds["T"],
            &args[0]
        )
        .yes()
    );
}

#[test]
fn has_method_and_has_property_are_read_off_the_class() {
    let r = build_registry("<?php class Foo { public int $x = 0; public function bar(): void {} }");
    let foo = Type::Object("Foo".into(), vec![]);
    assert!(r.accepts(&Type::HasMethod("bar".into()), &foo).yes());
    assert!(r.accepts(&Type::HasMethod("BAR".into()), &foo).yes());
    assert!(!r.accepts(&Type::HasMethod("baz".into()), &foo).yes());
    assert!(r.accepts(&Type::HasProperty("x".into()), &foo).yes());
    assert!(!r.accepts(&Type::HasProperty("y".into()), &foo).yes());
}

#[test]
fn a_shape_has_the_offsets_it_requires() {
    let r = build_registry("<?php");
    let shape = types::parse_type_string("array{a: int, b?: string}");
    let has = |key: &str| Type::HasOffset(types::ShapeKey::from_name(key));
    assert!(r.accepts(&has("a"), &shape).yes());
    assert!(!r.accepts(&has("b"), &shape).yes());
    assert!(!r.accepts(&has("c"), &shape).yes());
}

/// A toy extension payload: a `Builder` that knows how many joins it has.
#[derive(Debug)]
struct Joins(u32);

impl types::ExtensionTypeData for Joins {
    fn base(&self) -> Type {
        Type::Object("Builder".into(), vec![])
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn eq_data(&self, other: &dyn types::ExtensionTypeData) -> bool {
        other
            .as_any()
            .downcast_ref::<Joins>()
            .is_some_and(|o| o.0 == self.0)
    }
    fn hash_data(&self, state: &mut dyn std::hash::Hasher) {
        state.write_u32(self.0);
    }
    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Builder<joins={}>", self.0)
    }
    fn is_super_type_of(
        &self,
        other: &dyn types::ExtensionTypeData,
        _: &dyn types::ClassHierarchy,
    ) -> Option<types::TrinaryLogic> {
        let other = other.as_any().downcast_ref::<Joins>()?;
        Some(types::TrinaryLogic::from_bool(other.0 == self.0))
    }
    fn widen(&self) -> Option<Type> {
        Some(self.base())
    }
}

#[test]
fn an_extension_payload_relates_through_its_hooks_and_its_base() {
    use types::TrinaryLogic;
    let r = build_registry("<?php class Builder {}");
    let joins = |n| Type::Extension(std::sync::Arc::new(Joins(n)));
    let builder = Type::Object("Builder".into(), vec![]);
    assert_eq!(joins(1), joins(1));
    assert_ne!(joins(1), joins(2));
    assert_eq!(joins(2).to_string(), "Builder<joins=2>");
    assert!(r.accepts(&joins(1), &joins(1)).yes());
    assert!(!r.accepts(&joins(2), &joins(1)).yes());
    assert!(r.accepts(&builder, &joins(1)).yes());
    assert_eq!(r.is_super_type_of(&builder, &joins(3)), TrinaryLogic::Yes);
    assert_eq!(r.is_super_type_of(&joins(3), &builder), TrinaryLogic::Maybe);
    // Distinct payloads stay distinct until the union cap, then widen.
    let two = types::union_types(joins(1), joins(2));
    assert!(matches!(&two, Type::Union(members) if members.len() == 2));
    let many = (0..20).map(joins).reduce(types::union_types).unwrap();
    assert_eq!(many, builder);
}
