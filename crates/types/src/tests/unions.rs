//! `union_types`: which members merge and which stay apart.

use crate::{Repr, Shape, ShapeField, ShapeKey, Type};

fn shape(fields: &[(&str, Type, bool)]) -> Type {
    Type(Repr::ArrayShape(Shape::sealed(
        fields
            .iter()
            .map(|(k, t, o)| ShapeField::new(ShapeKey::from_name(k), t.clone(), *o))
            .collect(),
    )))
}

fn lit(s: &str) -> Type {
    Type(Repr::StringLiteral(s.into()))
}

#[test]
fn a_tagged_union_of_shapes_stays_two_members() {
    let a = shape(&[
        ("t", lit("a"), false),
        ("v", Type(Repr::IntLiteral(1)), false),
    ]);
    let b = shape(&[("t", lit("b"), false), ("v", lit("s"), false)]);
    assert_eq!(
        Type::union(a.clone(), b.clone()),
        Type(Repr::Union(vec![a, b]))
    );
}

#[test]
fn shapes_differing_only_in_their_tag_merge() {
    let a = shape(&[("t", lit("a"), false), ("v", Type(Repr::Int), false)]);
    let b = shape(&[("t", lit("b"), false), ("v", Type(Repr::Int), false)]);
    assert_eq!(
        Type::union(a, b),
        shape(&[
            ("t", Type(Repr::Union(vec![lit("a"), lit("b")])), false),
            ("v", Type(Repr::Int), false)
        ])
    );
}

#[test]
fn an_empty_array_stays_apart_from_a_shape_with_a_required_key() {
    let empty = shape(&[]);
    let one = Type(Repr::ArrayShape(Shape::sealed(vec![ShapeField::new(
        ShapeKey::Int(0),
        lit("x"),
        false,
    )])));
    assert_eq!(
        Type::union(empty.clone(), one.clone()),
        Type(Repr::Union(vec![empty, one]))
    );
}

#[test]
fn an_all_optional_shape_absorbs_the_empty_array() {
    let empty = shape(&[]);
    let optional = shape(&[("a", Type(Repr::Int), true)]);
    assert_eq!(Type::union(empty, optional.clone()), optional);
}

#[test]
fn a_general_array_absorbs_every_shape() {
    let general = Type(Repr::Array(
        Box::new(Type(Repr::String)),
        Box::new(Type(Repr::Int)),
    ));
    let s = shape(&[("a", Type(Repr::IntLiteral(1)), false)]);
    assert_eq!(
        Type::union(general, s),
        Type(Repr::Array(
            Box::new(Type(Repr::String)),
            Box::new(Type(Repr::Int))
        ))
    );
}

#[test]
fn never_true_and_false_fold_away() {
    assert_eq!(
        Type::union(Type(Repr::IntLiteral(1)), Type(Repr::Never)),
        Type(Repr::IntLiteral(1))
    );
    assert_eq!(
        Type::union(Type(Repr::True), Type(Repr::False)),
        Type(Repr::Bool)
    );
    assert_eq!(
        Type::union(
            Type(Repr::Union(vec![Type(Repr::Bool), Type(Repr::Int)])),
            Type(Repr::False)
        ),
        Type(Repr::Union(vec![Type(Repr::Bool), Type(Repr::Int)]))
    );
}

#[test]
fn a_union_keeps_an_explicit_mixed_explicit() {
    assert!(matches!(
        Type::union(Type(Repr::Mixed(true)), Type(Repr::Null)),
        Type(Repr::Mixed(true))
    ));
    assert!(matches!(
        Type::union(Type(Repr::Mixed(false)), Type(Repr::Null)),
        Type(Repr::Mixed(false))
    ));
}

#[test]
fn a_shape_with_two_more_required_keys_stays_apart() {
    let key = |i: i64, t: Type| ShapeField::new(ShapeKey::Int(i), t, false);
    let bare = Type(Repr::ArrayShape(Shape::sealed(vec![key(
        0,
        Type(Repr::String),
    )])));
    let with = Type(Repr::ArrayShape(Shape::sealed(vec![
        key(0, Type(Repr::String)),
        key(1, lit("ab")),
        key(2, lit("b")),
    ])));
    assert_eq!(
        Type::union(with.clone(), bare.clone()),
        Type(Repr::Union(vec![with, bare]))
    );
}

#[test]
fn an_error_absorbs_a_union() {
    assert_eq!(
        Type::union(Type(Repr::Error), Type(Repr::Int)),
        Type(Repr::Error)
    );
    assert_eq!(
        Type::union(
            Type(Repr::Union(vec![Type(Repr::Int), Type(Repr::Error)])),
            Type(Repr::String)
        ),
        Type(Repr::Error)
    );
}
