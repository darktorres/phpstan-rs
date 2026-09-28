//! `intersect_types`: which members fold together and which drop.

use crate::{AccessoryKind, Repr, Shape, ShapeField, ShapeKey, Type, intersect_types};

fn field(key: i64, ty: Type, optional: bool) -> ShapeField {
    ShapeField::new(ShapeKey::Int(key), ty, optional)
}

fn non_empty() -> Type {
    Type(Repr::Accessory(AccessoryKind::NonEmptyArray))
}

#[test]
fn two_shapes_intersect_into_one() {
    let literals = Type(Repr::Union((0..4).map(Type::int_literal).collect()));
    let payload = Type(Repr::Object("Payload".into(), Vec::new()));
    let a = Type(Repr::ArrayShape(Shape::sealed(vec![
        field(0, Type(Repr::Int), false),
        field(1, payload.clone(), false),
    ])));
    let b = Type(Repr::ArrayShape(Shape::sealed(vec![
        field(0, literals.clone(), false),
        field(1, Type(Repr::Mixed(false)), false),
    ])));
    assert_eq!(
        intersect_types(vec![a, b]),
        Type(Repr::ArrayShape(Shape::sealed(vec![
            field(0, literals, false),
            field(1, payload, false),
        ])))
    );
}

#[test]
fn a_required_key_the_other_sealed_shape_lacks_is_never() {
    let a = Type(Repr::ArrayShape(Shape::sealed(vec![field(
        0,
        Type(Repr::Int),
        false,
    )])));
    let b = Type(Repr::ArrayShape(Shape::sealed(vec![field(
        1,
        Type(Repr::Int),
        true,
    )])));
    assert_eq!(intersect_types(vec![a, b]), Type(Repr::Never));
}

#[test]
fn a_shape_within_an_array_takes_its_value_type() {
    let shape = Type(Repr::ArrayShape(Shape::sealed(vec![field(
        0,
        Type(Repr::Mixed(false)),
        false,
    )])));
    let array = Type(Repr::Array(
        Box::new(Type(Repr::Int)),
        Box::new(Type(Repr::String)),
    ));
    assert_eq!(
        intersect_types(vec![shape, array]),
        Type(Repr::ArrayShape(Shape::sealed(vec![field(
            0,
            Type(Repr::String),
            false
        )])))
    );
}

#[test]
fn an_all_optional_shape_stays_a_shape_that_cant_be_empty() {
    let shape = Type(Repr::ArrayShape(Shape::sealed(vec![field(
        0,
        Type(Repr::String),
        true,
    )])));
    let ty = intersect_types(vec![shape.clone(), non_empty()]);
    assert_eq!(ty, Type(Repr::Intersection(vec![shape, non_empty()])));
    assert_eq!(ty.to_string(), "non-empty-array{0?: string}");
}

#[test]
fn a_shape_with_a_required_key_absorbs_non_empty() {
    let shape = Type(Repr::ArrayShape(Shape::sealed(vec![field(
        0,
        Type(Repr::String),
        false,
    )])));
    assert_eq!(intersect_types(vec![shape.clone(), non_empty()]), shape);
}

#[test]
fn an_offset_makes_an_optional_key_required() {
    let shape = Type(Repr::ArrayShape(Shape::sealed(vec![field(
        0,
        Type(Repr::String),
        true,
    )])));
    assert_eq!(
        intersect_types(vec![shape, Type(Repr::HasOffset(ShapeKey::Int(0)))]),
        Type(Repr::ArrayShape(Shape::sealed(vec![field(
            0,
            Type(Repr::String),
            false
        )])))
    );
}

#[test]
fn a_member_within_another_drops_the_wider_one() {
    assert_eq!(
        intersect_types(vec![Type(Repr::Mixed(false)), Type(Repr::Int)]),
        Type(Repr::Int)
    );
    assert_eq!(
        intersect_types(vec![Type(Repr::Int), Type(Repr::IntLiteral(3))]),
        Type(Repr::IntLiteral(3))
    );
    assert_eq!(
        intersect_types(vec![Type(Repr::Int), Type(Repr::String)]),
        Type(Repr::Never)
    );
}

#[test]
fn a_refinement_never_replaces_its_base() {
    let refined = vec![
        Type(Repr::String),
        Type(Repr::Accessory(AccessoryKind::NonEmptyString)),
    ];
    assert_eq!(
        intersect_types(refined.clone()),
        Type(Repr::Intersection(refined))
    );
}

#[test]
fn an_error_drops_out_of_an_intersection() {
    assert_eq!(
        intersect_types(vec![Type(Repr::Error), Type(Repr::Int)]),
        Type(Repr::Int)
    );
}
