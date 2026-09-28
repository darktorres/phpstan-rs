//! `RuleLevelHelper`'s level-dependent acceptance.

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

// `rule_level_accepts`'s level-aware leniency.
#[test]
fn accepts_at_configured_level_matches_is_assignable_when_never_configured() {
    let levels = RuleLevelHelper::default();
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    assert_eq!(levels.configured_level, None);
    assert_eq!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &nullable_string)
            .yes(),
        levels
            .relation(&NoClasses)
            .accepts(&Type::string(), &nullable_string)
            .yes()
    );
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &nullable_string)
            .yes()
    );
}

#[test]
fn accepts_at_configured_level_matches_is_assignable_at_the_strict_threshold() {
    let levels = RuleLevelHelper {
        configured_level: Some(8),
        ..RuleLevelHelper::default()
    };
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &nullable_string)
            .yes()
    );
}

// The load-bearing case: a nullable-typed actual value (`?string`, the
// single most common PHP nullability idiom) must be accepted by a
// plain, non-nullable declared type below level 8, where plain
// `accepts` (and a naive "bail to accepts whenever `sup` is
// a Union too" fast path) both reject it.
#[test]
fn a_nullable_actual_type_is_accepted_by_a_non_nullable_declared_type_below_level_eight() {
    let mut levels = RuleLevelHelper::default();
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    for level in 0..8 {
        levels.configured_level = Some(level);
        assert!(
            levels
                .relation(&NoClasses)
                .rule_level_accepts(&Type::string(), &nullable_string)
                .yes(),
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
    let mut levels = RuleLevelHelper::default();
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    levels.configured_level = Some(5);
    assert!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&nullable_string, &Type::string())
            .yes()
    );
}

#[test]
fn a_definite_null_actual_value_is_still_rejected_by_a_non_nullable_declared_type() {
    let levels = RuleLevelHelper {
        configured_level: Some(5),
        ..RuleLevelHelper::default()
    };
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::null())
            .yes()
    );
}

#[test]
fn a_nullable_actual_type_is_not_accepted_by_a_definite_null_declared_type() {
    let mut levels = RuleLevelHelper::default();
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    levels.configured_level = Some(5);
    // Stripping Null out of `sub` must never happen when `sup` is itself
    // bare `Type::Null` - a `?string` actual value still doesn't
    // definitely satisfy a target that only ever accepts `null`.
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::null(), &nullable_string)
            .yes()
    );
}

#[test]
fn a_union_vs_union_pair_recurses_correctly_below_the_strict_threshold() {
    let levels = RuleLevelHelper {
        configured_level: Some(5),
        ..RuleLevelHelper::default()
    };
    let nullable_string = Type::union_of(vec![Type::string(), Type::null()]);
    assert!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&nullable_string, &nullable_string)
            .yes()
    );
    let nullable_int = Type::union_of(vec![Type::int(), Type::null()]);
    // A nullable string's non-null member (`string`) doesn't satisfy a
    // nullable int's declared type at all, even once Null is stripped
    // from the actual side.
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&nullable_int, &nullable_string)
            .yes()
    );
}

// `checkExplicitMixed`/`checkImplicitMixed` - real PHPStan's own level
// 9/10 (`rule_level_accepts`'s own `accepts_at_level`).
#[test]
fn below_level_nine_an_explicit_mixed_value_is_accepted_by_a_concrete_declared_type() {
    let mut levels = RuleLevelHelper::default();
    for level in 0..9 {
        levels.configured_level = Some(level);
        assert!(
            levels
                .relation(&NoClasses)
                .rule_level_accepts(&Type::string(), &Type::explicit_mixed())
                .yes(),
            "level {level} should still accept an explicit mixed value"
        );
    }
}

#[test]
fn at_level_nine_an_explicit_mixed_value_is_rejected_by_a_concrete_declared_type() {
    let levels = RuleLevelHelper {
        configured_level: Some(9),
        ..RuleLevelHelper::default()
    };
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::explicit_mixed())
            .yes()
    );
}

#[test]
fn at_level_nine_an_implicit_mixed_value_is_still_accepted() {
    let levels = RuleLevelHelper {
        configured_level: Some(9),
        ..RuleLevelHelper::default()
    };
    // Level 9 is `checkExplicitMixed` only - `checkImplicitMixed` needs
    // level 10, real PHPStan's own cumulative-but-distinct pair.
    assert!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::mixed())
            .yes()
    );
}

#[test]
fn at_level_ten_both_explicit_and_implicit_mixed_values_are_rejected() {
    let levels = RuleLevelHelper {
        configured_level: Some(10),
        ..RuleLevelHelper::default()
    };
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::explicit_mixed())
            .yes()
    );
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::mixed())
            .yes()
    );
}

#[test]
fn at_level_ten_a_mixed_value_is_still_accepted_by_a_mixed_declared_type() {
    let levels = RuleLevelHelper {
        configured_level: Some(10),
        ..RuleLevelHelper::default()
    };
    assert!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::mixed(), &Type::explicit_mixed())
            .yes()
    );
    assert!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::explicit_mixed(), &Type::mixed())
            .yes()
    );
}

#[test]
fn at_level_ten_a_mixed_value_inside_a_union_is_still_rejected_by_a_concrete_type() {
    let levels = RuleLevelHelper {
        configured_level: Some(10),
        ..RuleLevelHelper::default()
    };
    let string_or_mixed = Type::union_of(vec![Type::string(), Type::mixed()]);
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &string_or_mixed)
            .yes()
    );
}

#[test]
fn at_level_ten_ordinary_rejection_is_unaffected() {
    let levels = RuleLevelHelper {
        configured_level: Some(10),
        ..RuleLevelHelper::default()
    };
    // A real mismatch (not mixed-driven at all) must still be rejected -
    // this mechanism only ever turns an acceptance into a rejection,
    // never masks a genuine one.
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::int())
            .yes()
    );
}

// `check_benevolent_union_types`: PHPStan's standalone
// `checkBenevolentUnionTypes` parameter.
#[test]
fn a_benevolent_union_is_accepted_leniently_by_default() {
    let levels = RuleLevelHelper::default();
    assert!(!levels.check_benevolent_union_types);
    // Only `Int` actually matches `Type::String` here - the default,
    // unconfigured leniency accepts because *any* member does.
    assert!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::benevolent_array_key())
            .yes()
    );
}

#[test]
fn a_benevolent_union_is_rejected_strictly_once_opted_in() {
    let levels = RuleLevelHelper {
        check_benevolent_union_types: true,
        ..RuleLevelHelper::default()
    };
    // Once opted in, *every* member of the wrapped union must satisfy
    // `sup` - `Int` doesn't satisfy `Type::String`, so this now fails.
    assert!(
        !levels
            .relation(&NoClasses)
            .rule_level_accepts(&Type::string(), &Type::benevolent_array_key())
            .yes()
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
    assert!(
        levels
            .relation(&NoClasses)
            .rule_level_accepts(&int_or_string_target, &int_or_string)
            .yes()
    );
}

#[test]
fn opting_in_never_affects_is_assignable_directly() {
    // Real PHPStan's own `checkBenevolentUnionTypes` only ever changes
    // `RuleLevelHelper::accepts()` - never a bare `isSuperTypeOf()`/
    // `accepts()` call outside it (narrowing, generics bound-checking,
    // ...). Mirrored here: opting in must never touch this project's
    // own general-purpose `accepts`, only `rule_level_accepts`.
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
