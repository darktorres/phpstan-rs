//! Union collapsing, readonly/property hooks, literal/accessory refinement types.

// Expected types are built without a class hierarchy.
#![allow(clippy::disallowed_methods)]

use super::super::*;
use types::AccessoryKind;
use types::Shape;

// Exact type equality is the wrong tool for "is this the kind of thing
// being narrowed" - both `Relation::narrow`/`Type::without`
// (member filtering) and `Type::union` (member dedup at construction
// time) must not use plain `==`/`!=`.

#[test]
fn union_types_collapses_a_string_literal_into_an_already_present_string() {
    assert_eq!(
        Type::union(Type::string(), Type::string_literal("".into())),
        Type::string()
    );
}

#[test]
fn union_types_collapses_an_int_literal_into_an_already_present_int() {
    assert_eq!(Type::union(Type::int(), Type::int_literal(0)), Type::int());
}

#[test]
fn union_types_collapses_a_mixed_member_reached_only_via_flattening() {
    // The pre-existing fast path only catches a *bare* `Mixed` operand
    // (`a.is_mixed() || b.is_mixed()`) - a `Mixed`
    // that only shows up once a multi-member `Union` operand gets
    // flattened into `members` (real shape: `?mixed`'s own
    // `Union([Mixed, Null])` construction, `types/parse.rs`, before its
    // own fix) sailed straight through into the final assembled union
    // unexamined, instead of collapsing the whole thing to `mixed` the
    // way it's vacuously equal to.
    assert_eq!(
        Type::union(
            Type::union_of(vec![Type::string(), Type::mixed()]),
            Type::null()
        ),
        Type::mixed()
    );
    assert_eq!(
        Type::union(
            Type::null(),
            Type::union_of(vec![Type::string(), Type::mixed()])
        ),
        Type::mixed()
    );
}

// The float-widening pitfall this fix deliberately avoids: `subtype
// (IntLiteral(0), Float)` is `true` (PHP's int-to-float argument
// coercion), but that's an *assignability* fact, not a "this value's
// representation is already covered" one - an `IntLiteral` sitting
// next to an unrelated `Float` member must not be silently dropped.
#[test]
fn union_types_does_not_collapse_an_int_literal_into_an_unrelated_float() {
    assert_eq!(
        Type::union(Type::float(), Type::int_literal(0)),
        Type::union_of(vec![Type::float(), Type::int_literal(0)])
    );
}

// The same absorption idea, for an accessory-refined string/array and an
// IntRange - an unknown branch genuinely might be empty/out of range, so
// the refined member can't survive next to its own separately-present,
// unrefined base.
#[test]
fn union_types_collapses_a_non_empty_string_into_an_already_present_string() {
    let non_empty = Type::intersection_of(vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonEmptyString),
    ]);
    assert_eq!(Type::union(Type::string(), non_empty), Type::string());
}

#[test]
fn union_types_collapses_a_list_into_an_already_present_array() {
    let list_ty = Type::intersection_of(vec![
        Type::array(Type::unknown_array_key(), Type::int()),
        Type::accessory(AccessoryKind::ArrayList),
    ]);
    assert_eq!(
        Type::union(Type::array(Type::unknown_array_key(), Type::int()), list_ty),
        Type::array(Type::unknown_array_key(), Type::int())
    );
}

#[test]
fn union_types_collapses_an_int_range_into_an_already_present_int() {
    assert_eq!(
        Type::union(Type::int(), Type::int_range(Some(1), None)),
        Type::int()
    );
}

#[test]
fn union_types_collapses_a_float_literal_into_an_already_present_float() {
    assert_eq!(
        Type::union(Type::float(), Type::float_literal(1.5)),
        Type::float()
    );
}

#[test]
fn union_types_does_not_collapse_a_real_class_intersection() {
    // Guards the absorption fix above: a genuine, unrelated multi-class
    // intersection (no Accessory member at all) must never be treated
    // as "more specific than its own base type" - Countable&Iterator
    // sitting next to bare Countable in the same union are both real,
    // independent possibilities.
    let intersection = Type::intersection_of(vec![
        Type::object("Countable".into(), Vec::new()),
        Type::object("Iterator".into(), Vec::new()),
    ]);
    assert_eq!(
        Type::union(
            Type::object("Countable".into(), Vec::new()),
            intersection.clone()
        ),
        Type::union_of(vec![
            Type::object("Countable".into(), Vec::new()),
            intersection
        ])
    );
}

// `merge_scopes` (one `Type::union` call per `if`/`else`) and
// `write_index_into`'s own `Union` arm both feed into `Type::union`,
// which must cap how many distinct members it accumulates - a function
// with many sequential branches assigning genuinely distinct array
// shapes to the same variable can otherwise grow that variable's union
// member count combinatorially. Simulates that shape directly: more
// pairwise-distinct one-field `ArrayShape`s than the cap, folded through
// `Type::union` one at a time, exactly the access pattern `merge_scopes`
// uses across many sequential branches. Past the cap the shapes
// generalize into one array rather than giving up to `mixed`.
#[test]
fn union_types_caps_runaway_member_growth_instead_of_hanging() {
    let result = (0..types::MAX_UNION_MEMBERS + 8)
        .map(|i| Type::array_shape(Shape::from_named(vec![(i.to_string(), Type::int(), false)])))
        .reduce(Type::union)
        .unwrap();
    assert_eq!(
        result.array_value_type(),
        Some(Type::int()),
        "got {result:?}"
    );
    assert!(
        result.union_members().len() <= types::MAX_UNION_MEMBERS,
        "got {result:?}"
    );
}

// The cap must not misfire on the common, small case real code
// actually produces - a handful of genuinely distinct branch types
// stays a precise `Union`, not `Mixed`.
#[test]
fn union_types_below_the_cap_still_builds_a_precise_union() {
    let result = [Type::int(), Type::string(), Type::bool(), Type::null()]
        .into_iter()
        .reduce(Type::union)
        .unwrap();
    assert_eq!(
        result,
        Type::union_of(vec![
            Type::int(),
            Type::string(),
            Type::bool(),
            Type::null()
        ])
    );
}

// The two tests above exercise `generalize_by_key_set` directly;
// this one confirms `Type::union` itself actually calls it - built as
// a single `Type::union(pre_built_full_union, one_more_shape)` call
// (not a `.reduce()` chain) specifically so only *one* generalization
// pass happens, matching what `generalize_by_key_set`'s own
// standalone tests already prove that pass does.
#[test]
fn union_types_itself_generalizes_before_collapsing_to_mixed() {
    let cap = types::MAX_UNION_MEMBERS as i64;
    let full = Type::union_of(
        (0..cap)
            .map(|i| {
                Type::array_shape(Shape::from_named(vec![(
                    "x".to_string(),
                    Type::int_literal(i),
                    false,
                )]))
            })
            .collect(),
    );
    // One more distinct tag than the cap: no two merge without loss, so
    // only the member cap's generalization makes them one shape.
    let one_more = Type::array_shape(Shape::from_named(vec![(
        "x".to_string(),
        Type::int_literal(cap),
        false,
    )]));
    let result = Type::union(full, one_more);
    match result.as_array_shape() {
        Some(fields) => {
            let fields = fields.named_fields();
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "x");
        }
        None => panic!(
            "expected union_types to generalize down to one merged ArrayShape, got {result:?}"
        ),
    }
}

// String accessory case-tracking, full concat algorithm.
