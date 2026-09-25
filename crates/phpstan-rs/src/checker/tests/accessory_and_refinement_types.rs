//! Union collapsing, readonly/property hooks, literal/accessory refinement types.

use super::super::*;
use super::helpers::*;
use types::AccessoryKind;
use types::Shape;

// Exact type equality is the wrong tool for "is this the kind of thing
// being narrowed" - both `narrow_type_to`/`narrow_type_excluding`
// (member filtering) and `union_types` (member dedup at construction
// time) must not use plain `==`/`!=`.

#[test]
fn union_types_collapses_a_string_literal_into_an_already_present_string() {
    assert_eq!(
        union_types(Type::String, Type::StringLiteral("".to_string())),
        Type::String
    );
}

#[test]
fn union_types_collapses_an_int_literal_into_an_already_present_int() {
    assert_eq!(union_types(Type::Int, Type::IntLiteral(0)), Type::Int);
}

#[test]
fn union_types_collapses_a_mixed_member_reached_only_via_flattening() {
    // The pre-existing fast path only catches a *bare* `Mixed` operand
    // (`a == Type::Mixed(false) || b == Type::Mixed(false)`) - a `Mixed`
    // that only shows up once a multi-member `Union` operand gets
    // flattened into `members` (real shape: `?mixed`'s own
    // `Union([Mixed, Null])` construction, `types/parse.rs`, before its
    // own fix) sailed straight through into the final assembled union
    // unexamined, instead of collapsing the whole thing to `mixed` the
    // way it's vacuously equal to.
    assert_eq!(
        union_types(
            Type::Union(vec![Type::String, Type::Mixed(false)]),
            Type::Null
        ),
        Type::Mixed(false)
    );
    assert_eq!(
        union_types(
            Type::Null,
            Type::Union(vec![Type::String, Type::Mixed(false)])
        ),
        Type::Mixed(false)
    );
}

// The float-widening pitfall this fix deliberately avoids: `is_subtype
// (IntLiteral(0), Float)` is `true` (PHP's int-to-float argument
// coercion), but that's an *assignability* fact, not a "this value's
// representation is already covered" one - an `IntLiteral` sitting
// next to an unrelated `Float` member must not be silently dropped.
#[test]
fn union_types_does_not_collapse_an_int_literal_into_an_unrelated_float() {
    assert_eq!(
        union_types(Type::Float, Type::IntLiteral(0)),
        Type::Union(vec![Type::Float, Type::IntLiteral(0)])
    );
}

// The same absorption idea, for an accessory-refined string/array and an
// IntRange - an unknown branch genuinely might be empty/out of range, so
// the refined member can't survive next to its own separately-present,
// unrefined base.
#[test]
fn union_types_collapses_a_non_empty_string_into_an_already_present_string() {
    let non_empty = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::NonEmptyString),
    ]);
    assert_eq!(union_types(Type::String, non_empty), Type::String);
}

#[test]
fn union_types_collapses_a_list_into_an_already_present_array() {
    let list_ty = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    assert_eq!(
        union_types(
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
            list_ty
        ),
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int))
    );
}

#[test]
fn union_types_collapses_an_int_range_into_an_already_present_int() {
    assert_eq!(
        union_types(Type::Int, Type::IntRange(Some(1), None)),
        Type::Int
    );
}

#[test]
fn union_types_collapses_a_float_literal_into_an_already_present_float() {
    assert_eq!(
        union_types(Type::Float, Type::FloatLiteral(1.5)),
        Type::Float
    );
}

#[test]
fn union_types_does_not_collapse_a_real_class_intersection() {
    // Guards the absorption fix above: a genuine, unrelated multi-class
    // intersection (no Accessory member at all) must never be treated
    // as "more specific than its own base type" - Countable&Iterator
    // sitting next to bare Countable in the same union are both real,
    // independent possibilities.
    let intersection = Type::Intersection(vec![
        Type::Object("Countable".into(), Vec::new()),
        Type::Object("Iterator".into(), Vec::new()),
    ]);
    assert_eq!(
        union_types(
            Type::Object("Countable".into(), Vec::new()),
            intersection.clone()
        ),
        Type::Union(vec![
            Type::Object("Countable".into(), Vec::new()),
            intersection
        ])
    );
}

// `merge_scopes` (one `union_types` call per `if`/`else`) and
// `write_index_into`'s own `Union` arm both feed into `union_types`,
// which must cap how many distinct members it accumulates - a function
// with many sequential branches assigning genuinely distinct array
// shapes to the same variable can otherwise grow that variable's union
// member count combinatorially. Simulates that shape directly: 20
// pairwise-distinct one-field `ArrayShape`s folded through
// `union_types` one at a time, exactly the access pattern `merge_scopes`
// uses across many sequential branches.
#[test]
fn union_types_caps_runaway_member_growth_instead_of_hanging() {
    let result = (0..20)
        .map(|i| Type::ArrayShape(Shape::from_named(vec![(i.to_string(), Type::Int, false)])))
        .reduce(union_types)
        .unwrap();
    assert_eq!(
        result,
        Type::Mixed(false),
        "expected the cap to collapse a >12-member union to Mixed, got {result:?}"
    );
}

// The cap must not misfire on the common, small case real code
// actually produces - a handful of genuinely distinct branch types
// stays a precise `Union`, not `Mixed`.
#[test]
fn union_types_below_the_cap_still_builds_a_precise_union() {
    let result = [Type::Int, Type::String, Type::Bool, Type::Null]
        .into_iter()
        .reduce(union_types)
        .unwrap();
    assert_eq!(
        result,
        Type::Union(vec![Type::Int, Type::String, Type::Bool, Type::Null])
    );
}

// The two tests above exercise `generalize_by_key_set` directly;
// this one confirms `union_types` itself actually calls it - built as
// a single `union_types(pre_built_union_of_12, one_more_shape)` call
// (not a `.reduce()` chain) specifically so only *one* generalization
// pass happens, matching what `generalize_by_key_set`'s own
// standalone tests already prove that pass does.
#[test]
fn union_types_itself_generalizes_before_collapsing_to_mixed() {
    let first_twelve = Type::Union(
        (0..12)
            .map(|i| {
                let value = if i % 2 == 0 { Type::True } else { Type::False };
                Type::ArrayShape(Shape::from_named(vec![("x".to_string(), value, false)]))
            })
            .collect(),
    );
    // A genuinely new value, distinct from every one of the first 12
    // (which only alternate True/False) - `union_types`'s own
    // existing dedup (`!members.contains(&other)`) would otherwise
    // silently no-op adding a 13th member that already exists,
    // masking whether generalization ran at all.
    let thirteenth = Type::ArrayShape(Shape::from_named(vec![(
        "x".to_string(),
        Type::Null,
        false,
    )]));
    let result = union_types(first_twelve, thirteenth);
    match result {
        Type::ArrayShape(fields) => {
            let fields = fields.named_fields();
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "x");
        }
        other => panic!(
            "expected union_types to generalize down to one merged ArrayShape, got {other:?}"
        ),
    }
}

// String accessory case-tracking, full concat algorithm.

#[test]
fn a_float_literal_concatenated_with_a_string_literal_narrows_via_the_composed_accessories() {
    // This project's own inference has never given a bare float literal
    // expression a precise `Type::FloatLiteral` (only `Literal::Int`
    // does, `checker/expr.rs`'s own `ExprKind::Literal` arm collapses
    // `Literal::Float` straight to plain `Type::Float`) - so `1.5` here
    // reaches `resolve_concat_type` as `Type::Float`, not a literal, and
    // the result is the composed-accessory answer for that shape
    // (`Float`'s own `toString()` projection carries no non-empty/non-
    // falsy accessory on its own, so only the literal `'-suffix'`
    // operand's own non-falsiness survives the AND/OR composition).
    // `format_php_float`'s own real algorithm is covered directly by
    // `checker::narrowing::accessory_range`'s own unit tests instead,
    // where a real `Type::FloatLiteral` value is easy to construct by
    // hand.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            assertType('non-falsy-string', 1.5 . '-suffix');
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
