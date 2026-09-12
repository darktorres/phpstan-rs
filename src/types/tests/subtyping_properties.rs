//! Property-based tests for `types::is_subtype`, complementing the
//! hand-written examples in `subtyping.rs` (ROADMAP.md phase 19). Only
//! algebraic invariants that hold unconditionally for *any* `Type` are
//! asserted here - each one verified directly against `is_subtype`'s own
//! match arms (`subtype.rs`) rather than assumed.
use super::super::*;
use proptest::prelude::*;

/// Depth-0 alternatives only - no `Union`/`Intersection`/`Array`/etc.
/// nesting. Used both as `arbitrary_type`'s recursion base case and, on
/// its own, wherever a property specifically needs a member that can't
/// itself match an earlier, unrelated `is_subtype` arm (see
/// `a_unions_own_member_is_always_its_subtype` below).
///
/// `f64`/`FloatLiteral` are drawn from a bounded finite range rather than
/// `any::<f64>()`: a generated `NaN` would make `Type`'s derived
/// `PartialEq` (and so `is_subtype`'s own `(a, b) if a == b` reflexivity
/// arm) non-reflexive for reasons that are an IEEE-754/derived-`PartialEq`
/// artifact, not anything `is_subtype` itself gets wrong - excluded so
/// the reflexivity property below tests real subtyping behavior only.
fn leaf_type() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::Mixed),
        Just(Type::Never),
        Just(Type::Void),
        Just(Type::Null),
        Just(Type::Bool),
        Just(Type::True),
        Just(Type::False),
        Just(Type::Int),
        Just(Type::Float),
        Just(Type::String),
        Just(Type::SelfRef),
        Just(Type::ParentRef),
        Just(Type::StaticRef),
        any::<i64>().prop_map(Type::IntLiteral),
        "[a-zA-Z0-9_]{0,8}".prop_map(Type::StringLiteral),
        (-1e10_f64..1e10_f64).prop_map(Type::FloatLiteral),
        prop_oneof![
            Just(AccessoryKind::NonEmptyString),
            Just(AccessoryKind::NonFalsyString),
            Just(AccessoryKind::LiteralString),
            Just(AccessoryKind::NumericString),
            Just(AccessoryKind::ArrayList),
            Just(AccessoryKind::NonEmptyArray),
        ]
        .prop_map(Type::Accessory),
        "(Foo|Bar|Baz)".prop_map(|name| Type::Object(name, Vec::new())),
    ]
}

/// A small, explicitly depth-bounded `Type` generator built on top of
/// `leaf_type`. `Type` is a deeply recursive enum (`Union`,
/// `Intersection`, `Array(Box<Type>, Box<Type>)`, ...) - `prop_recursive`
/// caps depth (3), overall node count (32), and how many items a
/// collection-shaped variant (`Union`/`Intersection` members) gets (8)
/// explicitly, rather than risking a naive derived `Arbitrary`'s unbounded
/// nesting.
///
/// Deliberately excludes `Conditional`, `TemplateParam`, and `Callable`
/// from this first pass - each needs its own extra structural invariant
/// (a real parameter name, a real `CallableSig`) to generate anything
/// other than vacuous cases, which is a bigger, separate undertaking than
/// this pass's scope.
fn arbitrary_type() -> impl Strategy<Value = Type> {
    leaf_type().prop_recursive(3, 32, 8, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 1..4).prop_map(Type::Union),
            proptest::collection::vec(inner.clone(), 1..4).prop_map(Type::Intersection),
            (inner.clone(), inner.clone()).prop_map(|(k, v)| Type::Array(Box::new(k), Box::new(v))),
            inner.prop_map(|t| Type::ClassString(Box::new(t))),
        ]
    })
}

proptest! {
    /// `subtype.rs:124` - `(_, Type::Mixed) => true` unconditionally,
    /// before any other arm runs.
    #[test]
    fn mixed_is_always_a_supertype(t in arbitrary_type()) {
        prop_assert!(is_subtype(&t, &Type::Mixed));
    }

    /// `subtype.rs:125` - `(Type::Never, _) => true` unconditionally.
    #[test]
    fn never_is_always_a_subtype(t in arbitrary_type()) {
        prop_assert!(is_subtype(&Type::Never, &t));
    }

    /// `subtype.rs:126` - `(a, b) if a == b => true`.
    #[test]
    fn every_type_is_a_subtype_of_itself(t in arbitrary_type()) {
        let t2 = t.clone();
        prop_assert!(is_subtype(&t, &t2));
    }

    /// `subtype.rs`'s `(_, Type::Union(members)) => members.iter().any(|c|
    /// is_subtype(sub, c))` finds a literal member via the reflexivity arm
    /// above. Built from `leaf_type` rather than `arbitrary_type`: were a
    /// member itself a `Union`, `is_subtype(member, outer_union)` would
    /// instead match the *other* Union arm first (match arms are tried in
    /// source order, and `(Type::Union(_), _) => ...all(...)` sits above
    /// the membership arm this property means to exercise) - not
    /// unconditionally true, and a different property entirely.
    #[test]
    fn a_unions_own_member_is_always_its_subtype(
        (members, chosen) in proptest::collection::vec(leaf_type(), 1..6)
            .prop_flat_map(|members| {
                let chosen = proptest::sample::select(members.clone());
                (Just(members), chosen)
            })
    ) {
        let union = Type::Union(members);
        prop_assert!(is_subtype(&chosen, &union));
    }
}
