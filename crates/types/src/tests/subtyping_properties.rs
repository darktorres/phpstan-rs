//! Proptest-based subtyping properties for `Relation::is_super_type_of`. A
//! bounded-depth `Type` generator (`prop_recursive` - the correct tool
//! for a genuinely recursive ~20-variant enum, not a hand-rolled
//! recursive fn), plus 5 properties, each evidenced by real reasoning
//! about `is_super_type_of`'s own arms, not merely assumed to hold.
//!
//! Transitivity is deliberately NOT asserted as a blanket property: a
//! real, existing, documented non-transitive chain (`ClassString(Foo) <:
//! String <: ClassString(Bar)` while `ClassString(Foo) <: ClassString
//! (Bar)` is false - a deliberate false-negative-avoiding design choice,
//! see `is_super_type_of`'s own `ClassString`/`String` interchangeability
//! comment) would make a blanket transitivity property either fail on a
//! case this file's own author already reasoned through on purpose, or
//! need ad hoc generator exclusions purpose-built to dodge this
//! project's own documented design decisions.

use crate::{AccessoryKind, Repr, Type};
use proptest::prelude::*;

fn leaf_type() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::mixed()),
        Just(Type::never()),
        Just(Type::void()),
        Just(Type::null()),
        Just(Type::bool()),
        Just(Type::bool_literal(true)),
        Just(Type::bool_literal(false)),
        Just(Type::int()),
        Just(Type::float()),
        Just(Type::string()),
        any::<i64>().prop_map(Type::int_literal),
        // `f64::NAN` is generated too (`any::<f64>()` includes it) -
        // exactly the case the reflexivity fix in `is_super_type_of` exists
        // for.
        any::<f64>().prop_map(Type::float_literal),
        "[a-z]{0,5}".prop_map(|s| Type::string_literal(s.into())),
        "[A-Z][a-zA-Z]{0,5}".prop_map(|n| Type::object(n.into(), Vec::new())),
    ]
}

fn any_type() -> impl Strategy<Value = Type> {
    leaf_type().prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(k, v)| Type::array(k, v)),
            inner.clone().prop_map(Type::class_string),
            prop::collection::vec(inner.clone(), 2..4).prop_map(Type::union_of),
            prop_oneof![
                Just(AccessoryKind::NonEmptyString),
                Just(AccessoryKind::NonFalsyString),
                Just(AccessoryKind::LiteralString),
                Just(AccessoryKind::NumericString),
                Just(AccessoryKind::LowercaseString),
                Just(AccessoryKind::UppercaseString),
                Just(AccessoryKind::DecimalIntegerString),
            ]
            .prop_map(|kind| Type(Repr::Intersection(vec![
                Type::string(),
                Type::accessory(kind)
            ]))),
        ]
    })
}

proptest! {
    /// Every generated type is its own subtype - `is_super_type_of` needs a
    /// dedicated arm for this, since `FloatLiteral(NAN)` would otherwise
    /// fail under `Type`'s plain structural `PartialEq`, where
    /// `NAN != NAN`.
    #[test]
    fn reflexivity(t in any_type()) {
        prop_assert!(super::is_subtype(&t, &t));
    }

    /// `mixed` is a super type of everything.
    #[test]
    fn mixed_is_top(t in any_type()) {
        prop_assert!(super::is_subtype(&t, &Type::mixed()));
    }

    /// `never` is a subtype of everything.
    #[test]
    fn never_is_bottom(t in any_type()) {
        prop_assert!(super::is_subtype(&Type::never(), &t));
    }

    /// A member is a subtype of the union containing it.
    #[test]
    fn union_injection(members in prop::collection::vec(any_type(), 2..4)) {
        let union = Type(Repr::Union(members.clone()));
        for m in &members {
            prop_assert!(super::is_subtype(m, &union));
        }
    }

    /// An intersection is a subtype of each of its members.
    #[test]
    fn intersection_projection(members in prop::collection::vec(any_type(), 2..4)) {
        let intersection = Type(Repr::Intersection(members.clone()));
        for m in &members {
            prop_assert!(super::is_subtype(&intersection, m));
        }
    }
}

/// A deterministic, pinned regression - independent of the proptest run
/// above, so the fix stays covered even if a future edit to `leaf_type`/
/// `any_type` stopped generating a NaN case.
#[test]
fn nan_float_literal_is_reflexive() {
    assert!(super::is_subtype(
        &Type::float_literal(f64::NAN),
        &Type::float_literal(f64::NAN)
    ));
}

/// A second deterministic, pinned regression - the exact minimal
/// counterexample `proptest-regressions/types/tests/subtyping_
/// properties.txt` records: `A & B` must always be a subtype of `B`,
/// even when `B` is itself a `Union` that also happens to be one of
/// `A & B`'s own members - see `is_super_type_of`'s own new arm for the full
/// reasoning.
#[test]
fn an_intersection_is_a_subtype_of_its_own_union_shaped_member() {
    let union = Type(Repr::Union(vec![Type::null(), Type::bool()]));
    let intersection = Type(Repr::Intersection(vec![Type::void(), union.clone()]));
    assert!(super::is_subtype(&intersection, &union));
    assert!(super::is_subtype(&intersection, &Type::void()));
}
