//! The kind predicates: every value (`Yes`), none (`No`), some (`Maybe`).

use crate::Repr;
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
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString)
        ]))
        .is_string(),
        TrinaryLogic::Yes
    );
    assert_eq!(Type::list_of(Type::int()).is_array(), TrinaryLogic::Yes);
    assert_eq!(Type::int_range(Some(0), None).is_int(), TrinaryLogic::Yes);
    assert_eq!(Type::this().is_object(), TrinaryLogic::Yes);
}

#[test]
fn a_union_is_a_kind_only_when_every_member_is() {
    let nullable = Type::new(Repr::Union(vec![foo(), Type::null()]));
    assert_eq!(nullable.is_object(), TrinaryLogic::Maybe);
    assert_eq!(nullable.is_null(), TrinaryLogic::Maybe);
    let object_or_null = Type::new(Repr::Union(vec![
        Type::object_without_class(),
        Type::null(),
    ]));
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

#[test]
fn an_array_answers_its_value_and_key_types() {
    use crate::{Shape, ShapeField, ShapeKey};
    let shape = Type::array_shape(Shape::sealed(vec![
        ShapeField::new(ShapeKey::Int(0), Type::int(), false),
        ShapeField::new(ShapeKey::Int(1), Type::string(), false),
    ]));
    assert_eq!(
        shape.array_value_type(),
        Some(Type::new(Repr::Union(vec![Type::int(), Type::string()])))
    );
    let empty = Type::array_shape(Shape::sealed(Vec::new()));
    assert_eq!(empty.array_value_type(), Some(Type::never()));
    assert_eq!(
        Type::new(Repr::Union(vec![empty, Type::list_of(Type::int())])).array_value_type(),
        Some(Type::int())
    );
    assert_eq!(
        Type::new(Repr::Union(vec![Type::list_of(Type::int()), Type::null()])).array_value_type(),
        None
    );
    assert_eq!(
        Type::list_of(Type::int()).array_key_type(),
        Some(Type::int_range(Some(0), None))
    );
}

#[test]
fn to_boolean_follows_each_kind() {
    assert_eq!(
        Type::string_literal("0".into()).to_boolean(),
        Type::bool_literal(false)
    );
    assert_eq!(
        Type::int_range(Some(1), None).to_boolean(),
        Type::bool_literal(true)
    );
    assert_eq!(
        Type::class_string(Type::mixed()).to_boolean(),
        Type::bool_literal(true)
    );
    assert_eq!(
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonFalsyString)
        ]))
        .to_boolean(),
        Type::bool_literal(true)
    );
    assert_eq!(
        Type::new(Repr::Union(vec![Type::null(), Type::int_literal(1)])).to_boolean(),
        Type::bool()
    );
}

#[test]
fn an_all_optional_shape_isnt_surely_non_empty() {
    use crate::{Shape, ShapeField, ShapeKey};
    let optional = Type::array_shape(Shape::sealed(vec![ShapeField::new(
        ShapeKey::Int(0),
        Type::int(),
        true,
    )]));
    assert_eq!(optional.is_non_empty_array(), TrinaryLogic::Maybe);
    assert_eq!(
        Type::array_shape(Shape::sealed(Vec::new())).is_empty_array(),
        TrinaryLogic::Yes
    );
}

#[test]
fn generalize_widens_literals_cases_and_shapes() {
    use crate::{Shape, ShapeField, ShapeKey};
    assert_eq!(Type::int_range(Some(1), Some(3)).generalize(), Type::int());
    assert_eq!(
        Type::enum_case("E".into(), "A".into()).generalize(),
        Type::object("E".into(), Vec::new())
    );
    let tuple = Type::array_shape(Shape::sealed(vec![
        ShapeField::new(ShapeKey::Int(0), Type::string_literal("a".into()), false),
        ShapeField::new(ShapeKey::Int(1), Type::int_literal(1), false),
    ]));
    assert_eq!(tuple.generalize().to_string(), "non-empty-list<string|int>");
}

#[test]
fn finite_types_lists_a_bool_and_a_small_range() {
    assert_eq!(
        Type::bool().finite_types(),
        vec![Type::bool_literal(true), Type::bool_literal(false)]
    );
    assert_eq!(Type::int_range(Some(1), Some(2)).finite_types().len(), 2);
    assert!(Type::int().finite_types().is_empty());
}
