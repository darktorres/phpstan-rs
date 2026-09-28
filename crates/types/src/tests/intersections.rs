//! `intersect_types`: which members fold together and which drop.

use crate::{AccessoryKind, Shape, ShapeField, ShapeKey, Type, intersect_types};

fn field(key: i64, ty: Type, optional: bool) -> ShapeField {
    ShapeField::new(ShapeKey::Int(key), ty, optional)
}

fn non_empty() -> Type {
    Type::Accessory(AccessoryKind::NonEmptyArray)
}

#[test]
fn two_shapes_intersect_into_one() {
    let literals = Type::Union((0..4).map(Type::IntLiteral).collect());
    let payload = Type::Object("Payload".into(), Vec::new());
    let a = Type::ArrayShape(Shape::sealed(vec![
        field(0, Type::Int, false),
        field(1, payload.clone(), false),
    ]));
    let b = Type::ArrayShape(Shape::sealed(vec![
        field(0, literals.clone(), false),
        field(1, Type::Mixed(false), false),
    ]));
    assert_eq!(
        intersect_types(vec![a, b]),
        Type::ArrayShape(Shape::sealed(vec![
            field(0, literals, false),
            field(1, payload, false),
        ]))
    );
}

#[test]
fn a_required_key_the_other_sealed_shape_lacks_is_never() {
    let a = Type::ArrayShape(Shape::sealed(vec![field(0, Type::Int, false)]));
    let b = Type::ArrayShape(Shape::sealed(vec![field(1, Type::Int, true)]));
    assert_eq!(intersect_types(vec![a, b]), Type::Never);
}

#[test]
fn a_shape_within_an_array_takes_its_value_type() {
    let shape = Type::ArrayShape(Shape::sealed(vec![field(0, Type::Mixed(false), false)]));
    let array = Type::Array(Box::new(Type::Int), Box::new(Type::String));
    assert_eq!(
        intersect_types(vec![shape, array]),
        Type::ArrayShape(Shape::sealed(vec![field(0, Type::String, false)]))
    );
}

#[test]
fn an_all_optional_shape_stays_a_shape_that_cant_be_empty() {
    let shape = Type::ArrayShape(Shape::sealed(vec![field(0, Type::String, true)]));
    let ty = intersect_types(vec![shape.clone(), non_empty()]);
    assert_eq!(ty, Type::Intersection(vec![shape, non_empty()]));
    assert_eq!(ty.to_string(), "non-empty-array{0?: string}");
}

#[test]
fn a_shape_with_a_required_key_absorbs_non_empty() {
    let shape = Type::ArrayShape(Shape::sealed(vec![field(0, Type::String, false)]));
    assert_eq!(intersect_types(vec![shape.clone(), non_empty()]), shape);
}

#[test]
fn an_offset_makes_an_optional_key_required() {
    let shape = Type::ArrayShape(Shape::sealed(vec![field(0, Type::String, true)]));
    assert_eq!(
        intersect_types(vec![shape, Type::HasOffset(ShapeKey::Int(0))]),
        Type::ArrayShape(Shape::sealed(vec![field(0, Type::String, false)]))
    );
}

#[test]
fn a_member_within_another_drops_the_wider_one() {
    assert_eq!(
        intersect_types(vec![Type::Mixed(false), Type::Int]),
        Type::Int
    );
    assert_eq!(
        intersect_types(vec![Type::Int, Type::IntLiteral(3)]),
        Type::IntLiteral(3)
    );
    assert_eq!(intersect_types(vec![Type::Int, Type::String]), Type::Never);
}

#[test]
fn a_refinement_never_replaces_its_base() {
    let refined = vec![Type::String, Type::Accessory(AccessoryKind::NonEmptyString)];
    assert_eq!(
        intersect_types(refined.clone()),
        Type::Intersection(refined)
    );
}
