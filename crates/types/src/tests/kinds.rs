//! The kind predicates: every value (`Yes`), none (`No`), some (`Maybe`).

use crate::{AccessoryKind, TrinaryLogic, Type};

fn foo() -> Type {
    Type::object("Foo".into(), Vec::new())
}

#[test]
fn a_kind_holds_for_its_refinements() {
    assert_eq!(
        Type::string_literal("a".into()).is_string(),
        TrinaryLogic::Yes
    );
    assert_eq!(
        Type::intersection_of(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString)
        ])
        .is_string(),
        TrinaryLogic::Yes
    );
    assert_eq!(Type::list_of(Type::int()).is_array(), TrinaryLogic::Yes);
    assert_eq!(Type::int_range(Some(0), None).is_int(), TrinaryLogic::Yes);
    assert_eq!(Type::this().is_object(), TrinaryLogic::Yes);
}

#[test]
fn a_union_is_a_kind_only_when_every_member_is() {
    let nullable = Type::union_of(vec![foo(), Type::null()]);
    assert_eq!(nullable.is_object(), TrinaryLogic::Maybe);
    assert_eq!(nullable.is_null(), TrinaryLogic::Maybe);
    let object_or_null = Type::union_of(vec![Type::object_without_class(), Type::null()]);
    assert_eq!(nullable.is_within(&object_or_null), TrinaryLogic::Yes);
}

#[test]
fn unrelated_kinds_are_no_and_mixed_is_maybe() {
    assert_eq!(Type::int().is_string(), TrinaryLogic::No);
    assert_eq!(Type::mixed().is_string(), TrinaryLogic::Maybe);
    assert_eq!(Type::bool_literal(true).is_scalar(), TrinaryLogic::Yes);
}

#[test]
fn never_is_no_kind() {
    assert_eq!(Type::never().is_null(), TrinaryLogic::No);
    assert_eq!(Type::never().is_string(), TrinaryLogic::No);
}
