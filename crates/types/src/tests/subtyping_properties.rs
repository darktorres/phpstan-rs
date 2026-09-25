//! Proptest-based subtyping properties for `crate::is_subtype`. A
//! bounded-depth `Type` generator (`prop_recursive` - the correct tool
//! for a genuinely recursive ~20-variant enum, not a hand-rolled
//! recursive fn), plus 5 properties, each evidenced by real reasoning
//! about `is_subtype`'s own arms, not merely assumed to hold.
//!
//! Transitivity is deliberately NOT asserted as a blanket property: a
//! real, existing, documented non-transitive chain (`ClassString(Foo) <:
//! String <: ClassString(Bar)` while `ClassString(Foo) <: ClassString
//! (Bar)` is false - a deliberate false-negative-avoiding design choice,
//! see `is_subtype`'s own `ClassString`/`String` interchangeability
//! comment) would make a blanket transitivity property either fail on a
//! case this file's own author already reasoned through on purpose, or
//! need ad hoc generator exclusions purpose-built to dodge this
//! project's own documented design decisions.

use crate::{AccessoryKind, Type};
use proptest::prelude::*;

fn leaf_type() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::Mixed(false)),
        Just(Type::Never),
        Just(Type::Void),
        Just(Type::Null),
        Just(Type::Bool),
        Just(Type::True),
        Just(Type::False),
        Just(Type::Int),
        Just(Type::Float),
        Just(Type::String),
        any::<i64>().prop_map(Type::IntLiteral),
        // `f64::NAN` is generated too (`any::<f64>()` includes it) -
        // exactly the case the reflexivity fix in `is_subtype` exists
        // for.
        any::<f64>().prop_map(Type::FloatLiteral),
        "[a-z]{0,5}".prop_map(Type::StringLiteral),
        "[A-Z][a-zA-Z]{0,5}".prop_map(|n| Type::Object(n.into(), Vec::new())),
    ]
}

fn any_type() -> impl Strategy<Value = Type> {
    leaf_type().prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(k, v)| Type::Array(Box::new(k), Box::new(v))),
            inner.clone().prop_map(|t| Type::ClassString(Box::new(t))),
            prop::collection::vec(inner.clone(), 2..4).prop_map(Type::Union),
            prop_oneof![
                Just(AccessoryKind::NonEmptyString),
                Just(AccessoryKind::NonFalsyString),
                Just(AccessoryKind::LiteralString),
                Just(AccessoryKind::NumericString),
                Just(AccessoryKind::LowercaseString),
                Just(AccessoryKind::UppercaseString),
                Just(AccessoryKind::DecimalIntegerString),
            ]
            .prop_map(|kind| Type::Intersection(vec![Type::String, Type::Accessory(kind)])),
        ]
    })
}

proptest! {
    /// Every generated type is its own subtype - `is_subtype` needs a
    /// dedicated arm for this, since `FloatLiteral(NAN)` would otherwise
    /// fail under `Type`'s plain structural `PartialEq`, where
    /// `NAN != NAN`.
    #[test]
    fn reflexivity(t in any_type()) {
        prop_assert!(crate::is_subtype(&t, &t));
    }

    /// `(_, Type::Mixed(false)) => true` is `is_subtype`'s own first arm -
    /// `Mixed` accepts everything.
    #[test]
    fn mixed_is_top(t in any_type()) {
        prop_assert!(crate::is_subtype(&t, &Type::Mixed(false)));
    }

    /// `(Type::Never, _) => true` is `is_subtype`'s own second arm -
    /// `Never` satisfies everything.
    #[test]
    fn never_is_bottom(t in any_type()) {
        prop_assert!(crate::is_subtype(&Type::Never, &t));
    }

    /// `(_, Type::Union(members)) => members.iter().any(|m| is_subtype
    /// (sub, m))` - a member is always a subtype of the union containing
    /// it (found via reflexivity: `is_subtype(m, m)` is what the `.any()`
    /// finds).
    #[test]
    fn union_injection(members in prop::collection::vec(any_type(), 2..4)) {
        let union = Type::Union(members.clone());
        for m in &members {
            prop_assert!(crate::is_subtype(m, &union));
        }
    }

    /// `(Type::Intersection(members), _) => members.iter().any(|m|
    /// is_subtype(m, sup))` - an intersection is always a subtype of
    /// each of its own members (same reflexivity-driven reasoning as
    /// union injection, the dual direction).
    #[test]
    fn intersection_projection(members in prop::collection::vec(any_type(), 2..4)) {
        let intersection = Type::Intersection(members.clone());
        for m in &members {
            prop_assert!(crate::is_subtype(&intersection, m));
        }
    }
}

/// A deterministic, pinned regression - independent of the proptest run
/// above, so the fix stays covered even if a future edit to `leaf_type`/
/// `any_type` stopped generating a NaN case.
#[test]
fn nan_float_literal_is_reflexive() {
    assert!(crate::is_subtype(
        &Type::FloatLiteral(f64::NAN),
        &Type::FloatLiteral(f64::NAN)
    ));
}

/// A second deterministic, pinned regression - the exact minimal
/// counterexample `proptest-regressions/types/tests/subtyping_
/// properties.txt` records: `A & B` must always be a subtype of `B`,
/// even when `B` is itself a `Union` that also happens to be one of
/// `A & B`'s own members - see `is_subtype`'s own new arm for the full
/// reasoning.
#[test]
fn an_intersection_is_a_subtype_of_its_own_union_shaped_member() {
    let union = Type::Union(vec![Type::Null, Type::Bool]);
    let intersection = Type::Intersection(vec![Type::Void, union.clone()]);
    assert!(crate::is_subtype(&intersection, &union));
    assert!(crate::is_subtype(&intersection, &Type::Void));
}
