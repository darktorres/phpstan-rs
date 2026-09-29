//! `intersect_types`: which members fold together and which drop.

use crate::{AccessoryKind, Repr, Shape, ShapeField, ShapeKey, Type, intersect_types};

fn field(key: i64, ty: Type, optional: bool) -> ShapeField {
    ShapeField::new(ShapeKey::Int(key), ty, optional)
}

fn non_empty() -> Type {
    Type::accessory(AccessoryKind::NonEmptyArray)
}

#[test]
fn two_shapes_intersect_into_one() {
    let literals = Type::new(Repr::Union((0..4).map(Type::int_literal).collect()));
    let payload = Type::object("Payload".into(), Vec::new());
    let a = Type::array_shape(Shape::sealed(vec![
        field(0, Type::new(Repr::Int), false),
        field(1, payload.clone(), false),
    ]));
    let b = Type::array_shape(Shape::sealed(vec![
        field(0, literals.clone(), false),
        field(1, Type::new(Repr::Mixed(false, None)), false),
    ]));
    assert_eq!(
        intersect_types(vec![a, b]),
        Type::array_shape(Shape::sealed(vec![
            field(0, literals, false),
            field(1, payload, false),
        ]))
    );
}

#[test]
fn a_required_key_the_other_sealed_shape_lacks_is_never() {
    let a = Type::array_shape(Shape::sealed(vec![field(0, Type::new(Repr::Int), false)]));
    let b = Type::array_shape(Shape::sealed(vec![field(1, Type::new(Repr::Int), true)]));
    assert_eq!(intersect_types(vec![a, b]), Type::never());
}

#[test]
fn a_shape_within_an_array_takes_its_value_type() {
    let shape = Type::array_shape(Shape::sealed(vec![field(
        0,
        Type::new(Repr::Mixed(false, None)),
        false,
    )]));
    let array = Type::array(Type::new(Repr::Int), Type::new(Repr::String));
    assert_eq!(
        intersect_types(vec![shape, array]),
        Type::array_shape(Shape::sealed(vec![field(
            0,
            Type::new(Repr::String),
            false
        )]))
    );
}

#[test]
fn an_all_optional_shape_stays_a_shape_that_cant_be_empty() {
    let shape = Type::array_shape(Shape::sealed(vec![field(0, Type::new(Repr::String), true)]));
    let ty = intersect_types(vec![shape.clone(), non_empty()]);
    assert_eq!(ty, Type::new(Repr::Intersection(vec![shape, non_empty()])));
    assert_eq!(ty.to_string(), "non-empty-array{0?: string}");
}

#[test]
fn a_shape_with_a_required_key_absorbs_non_empty() {
    let shape = Type::array_shape(Shape::sealed(vec![field(
        0,
        Type::new(Repr::String),
        false,
    )]));
    assert_eq!(intersect_types(vec![shape.clone(), non_empty()]), shape);
}

#[test]
fn an_offset_makes_an_optional_key_required() {
    let shape = Type::array_shape(Shape::sealed(vec![field(0, Type::new(Repr::String), true)]));
    assert_eq!(
        intersect_types(vec![shape, Type::has_offset(ShapeKey::Int(0))]),
        Type::array_shape(Shape::sealed(vec![field(
            0,
            Type::new(Repr::String),
            false
        )]))
    );
}

#[test]
fn a_member_within_another_drops_the_wider_one() {
    assert_eq!(
        intersect_types(vec![Type::mixed(), Type::int()]),
        Type::int()
    );
    assert_eq!(
        intersect_types(vec![Type::int(), Type::int_literal(3)]),
        Type::int_literal(3)
    );
    assert_eq!(
        intersect_types(vec![Type::int(), Type::string()]),
        Type::never()
    );
}

#[test]
fn a_refinement_never_replaces_its_base() {
    let refined = vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonEmptyString),
    ];
    assert_eq!(
        intersect_types(refined.clone()),
        Type::new(Repr::Intersection(refined))
    );
}

#[test]
fn an_error_drops_out_of_an_intersection() {
    assert_eq!(
        intersect_types(vec![Type::error(), Type::int()]),
        Type::int()
    );
}

#[test]
fn a_list_intersected_with_a_shape_keeps_only_list_keys() {
    let list = Type::accessory(AccessoryKind::ArrayList);
    let keyed = Type::array_shape(Shape::from_named([("a", Type::int(), false)]));
    assert_eq!(intersect_types(vec![list.clone(), keyed]), Type::never());
    let gapped = Type::array_shape(Shape::from_named([
        ("0", Type::int(), false),
        ("2", Type::int(), false),
    ]));
    assert_eq!(intersect_types(vec![list.clone(), gapped]), Type::never());
    let tuple = Type::array_shape(Shape::from_named([
        ("0", Type::int(), false),
        ("1", Type::int(), false),
    ]));
    assert_eq!(intersect_types(vec![list, tuple.clone()]), tuple);
}
