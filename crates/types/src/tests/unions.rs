//! `union_types`: which members merge and which stay apart.

use crate::{Shape, ShapeField, ShapeKey, Type, union_types};

fn shape(fields: &[(&str, Type, bool)]) -> Type {
    Type::ArrayShape(Shape::sealed(
        fields
            .iter()
            .map(|(k, t, o)| ShapeField::new(ShapeKey::from_name(k), t.clone(), *o))
            .collect(),
    ))
}

fn lit(s: &str) -> Type {
    Type::StringLiteral(s.into())
}

#[test]
fn a_tagged_union_of_shapes_stays_two_members() {
    let a = shape(&[("t", lit("a"), false), ("v", Type::IntLiteral(1), false)]);
    let b = shape(&[("t", lit("b"), false), ("v", lit("s"), false)]);
    assert_eq!(union_types(a.clone(), b.clone()), Type::Union(vec![a, b]));
}

#[test]
fn shapes_differing_only_in_their_tag_merge() {
    let a = shape(&[("t", lit("a"), false), ("v", Type::Int, false)]);
    let b = shape(&[("t", lit("b"), false), ("v", Type::Int, false)]);
    assert_eq!(
        union_types(a, b),
        shape(&[
            ("t", Type::Union(vec![lit("a"), lit("b")]), false),
            ("v", Type::Int, false)
        ])
    );
}

#[test]
fn an_empty_array_stays_apart_from_a_shape_with_a_required_key() {
    let empty = shape(&[]);
    let one = Type::ArrayShape(Shape::sealed(vec![ShapeField::new(
        ShapeKey::Int(0),
        lit("x"),
        false,
    )]));
    assert_eq!(
        union_types(empty.clone(), one.clone()),
        Type::Union(vec![empty, one])
    );
}

#[test]
fn an_all_optional_shape_absorbs_the_empty_array() {
    let empty = shape(&[]);
    let optional = shape(&[("a", Type::Int, true)]);
    assert_eq!(union_types(empty, optional.clone()), optional);
}

#[test]
fn a_general_array_absorbs_every_shape() {
    let general = Type::Array(Box::new(Type::String), Box::new(Type::Int));
    let s = shape(&[("a", Type::IntLiteral(1), false)]);
    assert_eq!(
        union_types(general, s),
        Type::Array(Box::new(Type::String), Box::new(Type::Int))
    );
}

#[test]
fn never_true_and_false_fold_away() {
    assert_eq!(
        union_types(Type::IntLiteral(1), Type::Never),
        Type::IntLiteral(1)
    );
    assert_eq!(union_types(Type::True, Type::False), Type::Bool);
    assert_eq!(
        union_types(Type::Union(vec![Type::Bool, Type::Int]), Type::False),
        Type::Union(vec![Type::Bool, Type::Int])
    );
}

#[test]
fn a_union_keeps_an_explicit_mixed_explicit() {
    assert!(matches!(
        union_types(Type::Mixed(true), Type::Null),
        Type::Mixed(true)
    ));
    assert!(matches!(
        union_types(Type::Mixed(false), Type::Null),
        Type::Mixed(false)
    ));
}

#[test]
fn a_shape_with_two_more_required_keys_stays_apart() {
    let key = |i: i64, t: Type| ShapeField::new(ShapeKey::Int(i), t, false);
    let bare = Type::ArrayShape(Shape::sealed(vec![key(0, Type::String)]));
    let with = Type::ArrayShape(Shape::sealed(vec![
        key(0, Type::String),
        key(1, lit("ab")),
        key(2, lit("b")),
    ]));
    assert_eq!(
        union_types(with.clone(), bare.clone()),
        Type::Union(vec![with, bare])
    );
}

#[test]
fn an_error_absorbs_a_union() {
    assert_eq!(union_types(Type::Error, Type::Int), Type::Error);
    assert_eq!(
        union_types(Type::Union(vec![Type::Int, Type::Error]), Type::String),
        Type::Error
    );
}
