//! `RuleLevelHelper`'s level-dependent acceptance.

// Expected types are built without a class hierarchy.
#![allow(clippy::disallowed_methods)]

use types::{NoClasses, Shape, Type};

use super::RuleLevelHelper;

#[test]
fn strict_mixed_arrays_opts_out_of_the_unknown_array_element_leniency() {
    let mut levels = RuleLevelHelper {
        strict_mixed_arrays: true,
        ..RuleLevelHelper::default()
    };
    let unknown_elements = Type::array(Type::unknown_array_key(), Type::mixed());
    let ints = Type::array(Type::unknown_array_key(), Type::int());
    assert!(
        !levels
            .relation(&NoClasses)
            .accepts(&ints, &unknown_elements)
            .yes()
    );
    // Still correctly permissive when the element types genuinely
    // match - strictness only removes the "we don't know" free pass,
    // it doesn't break real assignability.
    assert!(
        levels
            .relation(&NoClasses)
            .accepts(&ints, &Type::array(Type::unknown_array_key(), Type::int()))
            .yes()
    );

    levels.strict_mixed_arrays = false;
    assert!(
        levels
            .relation(&NoClasses)
            .accepts(&ints, &unknown_elements)
            .yes()
    );
}

#[test]
fn strict_mixed_arrays_opts_out_of_the_unknown_array_shape_leniency() {
    let levels = RuleLevelHelper {
        strict_mixed_arrays: true,
        ..RuleLevelHelper::default()
    };
    let unknown = Type::array(Type::unknown_array_key(), Type::mixed());
    let shape = Type::array_shape(Shape::from_named(vec![(
        "name".to_string(),
        Type::string(),
        false,
    )]));
    assert!(!levels.relation(&NoClasses).accepts(&shape, &unknown).yes());
}

// `rejected_from`: the level from which a rule rejects a value.
fn rejected_from(levels: RuleLevelHelper, accepting: &Type, accepted: &Type) -> Option<u8> {
    levels
        .relation(&NoClasses)
        .rejected_from(accepting, accepted)
}

// The load-bearing case: a nullable-typed actual value (`?string`, the
// single most common PHP nullability idiom) must be accepted by a
// plain, non-nullable declared type below level 8, where plain
// `accepts` (and a naive "bail to accepts whenever `sup` is
// a Union too" fast path) both reject it.
#[test]
fn a_nullable_actual_type_is_rejected_by_a_non_nullable_declared_type_from_level_eight() {
    let levels = RuleLevelHelper::default();
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    assert!(
        !levels
            .relation(&NoClasses)
            .accepts(&Type::string(), &nullable_string)
            .yes()
    );
    assert_eq!(
        rejected_from(levels, &Type::string(), &nullable_string),
        Some(8)
    );
}

// The real, common shape every one of this phase's 8 call sites actually
// sees: a nullable *declared* type (`sup = Union([String, Null])`, e.g. a
// `?string`-typed parameter) accepting a plain non-nullable actual
// argument - already true via plain `accepts`'s own unconditional
// `.any()` arm, but must keep working once routed through the new
// recursive Union-vs-Union-aware method instead of silently regressing.
#[test]
fn a_plain_actual_value_still_satisfies_a_nullable_declared_type() {
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    assert_eq!(
        rejected_from(
            RuleLevelHelper::default(),
            &nullable_string,
            &Type::string()
        ),
        None
    );
}

#[test]
fn a_definite_null_actual_value_is_rejected_by_a_non_nullable_declared_type_at_every_level() {
    assert_eq!(
        rejected_from(RuleLevelHelper::default(), &Type::string(), &Type::null()),
        Some(0)
    );
}

#[test]
fn a_nullable_actual_type_is_rejected_by_a_definite_null_declared_type_from_level_seven() {
    // Stripping Null out of `sub` must never happen when `sup` is itself
    // bare `null` - a `?string` actual value still doesn't definitely
    // satisfy a target that only ever accepts `null` once level 7 checks
    // union members; below it any member of a union given is enough.
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    assert_eq!(
        rejected_from(RuleLevelHelper::default(), &Type::null(), &nullable_string),
        Some(7)
    );
}

#[test]
fn a_union_vs_union_pair_recurses_correctly_below_the_strict_threshold() {
    let levels = RuleLevelHelper::default();
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    assert_eq!(
        rejected_from(levels, &nullable_string, &nullable_string),
        None
    );
    let nullable_int = Type::union_of(vec![Type::int(), Type::null()]);
    // A nullable string's non-null member (`string`) doesn't satisfy a
    // nullable int's declared type at all, even once Null is stripped
    // from the actual side.
    assert_eq!(
        rejected_from(levels, &nullable_int, &nullable_string),
        Some(0)
    );
}

// A template its bound makes acceptable stays accepted where a union is
// declared, though no member of the union accepts it alone.
#[test]
fn a_template_bounded_by_a_declared_union_is_accepted_at_every_level() {
    let int_or_string = Type::union_of(vec![Type::int(), Type::string()]);
    let t = Type::template_param(types::TemplateParam {
        bound: Box::new(int_or_string.clone()),
        ..types::TemplateParam::unbounded("T".to_string())
    });
    assert_eq!(
        rejected_from(RuleLevelHelper::default(), &int_or_string, &t),
        None
    );
}

// `checkExplicitMixed`/`checkImplicitMixed` - real PHPStan's own level
// 9/10 (`rejected_from`'s own `accepts_at_level`).
#[test]
fn an_explicit_mixed_value_is_rejected_by_a_concrete_declared_type_from_level_nine() {
    assert_eq!(
        rejected_from(
            RuleLevelHelper::default(),
            &Type::string(),
            &Type::explicit_mixed()
        ),
        Some(9)
    );
}

#[test]
fn an_implicit_mixed_value_is_rejected_by_a_concrete_declared_type_from_level_ten() {
    // Level 9 is `checkExplicitMixed` only - `checkImplicitMixed` needs
    // level 10, real PHPStan's own cumulative-but-distinct pair.
    assert_eq!(
        rejected_from(RuleLevelHelper::default(), &Type::string(), &Type::mixed()),
        Some(10)
    );
}

#[test]
fn a_mixed_value_is_accepted_by_a_mixed_declared_type_at_every_level() {
    let levels = RuleLevelHelper::default();
    assert_eq!(
        rejected_from(levels, &Type::mixed(), &Type::explicit_mixed()),
        None
    );
    assert_eq!(
        rejected_from(levels, &Type::explicit_mixed(), &Type::mixed()),
        None
    );
}

#[test]
fn a_mixed_value_inside_a_union_is_rejected_by_a_concrete_type_from_level_ten() {
    let string_or_mixed = Type::union_of(vec![Type::string(), Type::mixed()]);
    assert_eq!(
        rejected_from(
            RuleLevelHelper::default(),
            &Type::string(),
            &string_or_mixed
        ),
        Some(10)
    );
}

#[test]
fn ordinary_rejection_is_made_at_every_level() {
    // A real mismatch (not mixed-driven at all) must still be rejected -
    // this mechanism only ever turns an acceptance into a rejection,
    // never masks a genuine one.
    assert_eq!(
        rejected_from(RuleLevelHelper::default(), &Type::string(), &Type::int()),
        Some(0)
    );
}

// `check_benevolent_union_types`: PHPStan's standalone
// `checkBenevolentUnionTypes` parameter.
#[test]
fn a_benevolent_union_is_accepted_leniently_by_default() {
    let levels = RuleLevelHelper::default();
    assert!(!levels.check_benevolent_union_types);
    // Only `Int` actually matches `string` here - the default,
    // unconfigured leniency accepts because *any* member does.
    assert_eq!(
        rejected_from(levels, &Type::string(), &Type::benevolent_array_key()),
        None
    );
}

#[test]
fn a_benevolent_union_is_rejected_strictly_once_opted_in() {
    let levels = RuleLevelHelper {
        check_benevolent_union_types: true,
        ..RuleLevelHelper::default()
    };
    // Once opted in, *every* member of the wrapped union must satisfy
    // `sup` - `Int` doesn't satisfy `string`, so this now fails.
    assert_eq!(
        rejected_from(levels, &Type::string(), &Type::benevolent_array_key()),
        Some(0)
    );
}

#[test]
fn a_benevolent_union_still_accepted_once_opted_in_when_every_member_matches() {
    let levels = RuleLevelHelper {
        check_benevolent_union_types: true,
        ..RuleLevelHelper::default()
    };
    let int_or_string = Type::benevolent_union_of(vec![Type::int(), Type::string()]);
    let int_or_string_target = Type::union_of(vec![Type::int(), Type::string()]);
    assert_eq!(
        rejected_from(levels, &int_or_string_target, &int_or_string),
        None
    );
}

#[test]
fn opting_in_never_affects_is_assignable_directly() {
    // Real PHPStan's own `checkBenevolentUnionTypes` only ever changes
    // `RuleLevelHelper::accepts()` - never a bare `isSuperTypeOf()`/
    // `accepts()` call outside it (narrowing, generics bound-checking,
    // ...). Mirrored here: opting in must never touch this project's
    // own general-purpose `accepts`, only `rejected_from`.
    let levels = RuleLevelHelper {
        check_benevolent_union_types: true,
        ..RuleLevelHelper::default()
    };
    assert!(
        levels
            .relation(&NoClasses)
            .accepts(&Type::string(), &Type::benevolent_array_key())
            .yes()
    );
}
