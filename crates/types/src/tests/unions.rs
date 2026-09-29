//! `Type::union`: which members merge and which stay apart.

use crate::{Repr, Shape, ShapeField, ShapeKey, Type};

fn shape(fields: &[(&str, Type, bool)]) -> Type {
    Type::array_shape(Shape::sealed(
        fields
            .iter()
            .map(|(k, t, o)| ShapeField::new(ShapeKey::from_name(k), t.clone(), *o))
            .collect(),
    ))
}

fn lit(s: &str) -> Type {
    Type::string_literal(s.into())
}

#[test]
fn a_tagged_union_of_shapes_stays_two_members() {
    let a = shape(&[("t", lit("a"), false), ("v", Type::int_literal(1), false)]);
    let b = shape(&[("t", lit("b"), false), ("v", lit("s"), false)]);
    assert_eq!(
        Type::union(a.clone(), b.clone()),
        Type::new(Repr::Union(vec![a, b]))
    );
}

#[test]
fn shapes_differing_only_in_their_tag_merge() {
    let a = shape(&[("t", lit("a"), false), ("v", Type::int(), false)]);
    let b = shape(&[("t", lit("b"), false), ("v", Type::int(), false)]);
    assert_eq!(
        Type::union(a, b),
        shape(&[
            ("t", Type::new(Repr::Union(vec![lit("a"), lit("b")])), false),
            ("v", Type::int(), false)
        ])
    );
}

#[test]
fn an_empty_array_stays_apart_from_a_shape_with_a_required_key() {
    let empty = shape(&[]);
    let one = Type::array_shape(Shape::sealed(vec![ShapeField::new(
        ShapeKey::Int(0),
        lit("x"),
        false,
    )]));
    assert_eq!(
        Type::union(empty.clone(), one.clone()),
        Type::new(Repr::Union(vec![empty, one]))
    );
}

#[test]
fn an_all_optional_shape_absorbs_the_empty_array() {
    let empty = shape(&[]);
    let optional = shape(&[("a", Type::int(), true)]);
    assert_eq!(Type::union(empty, optional.clone()), optional);
}

#[test]
fn a_general_array_absorbs_every_shape() {
    let general = Type::array(Type::new(Repr::String), Type::new(Repr::Int));
    let s = shape(&[("a", Type::int_literal(1), false)]);
    assert_eq!(
        Type::union(general, s),
        Type::array(Type::new(Repr::String), Type::new(Repr::Int))
    );
}

#[test]
fn never_true_and_false_fold_away() {
    assert_eq!(
        Type::union(Type::int_literal(1), Type::never()),
        Type::int_literal(1)
    );
    assert_eq!(
        Type::union(Type::bool_literal(true), Type::bool_literal(false)),
        Type::bool()
    );
    assert_eq!(
        Type::union(
            Type::new(Repr::Union(vec![Type::bool(), Type::int()])),
            Type::bool_literal(false)
        ),
        Type::new(Repr::Union(vec![Type::bool(), Type::int()]))
    );
}

#[test]
fn a_union_keeps_an_explicit_mixed_explicit() {
    assert!(matches!(
        (Type::union(Type::explicit_mixed(), Type::null())).repr(),
        Repr::Mixed(true, None)
    ));
    assert!(matches!(
        (Type::union(Type::mixed(), Type::null())).repr(),
        Repr::Mixed(false, None)
    ));
}

#[test]
fn a_shape_with_two_more_required_keys_stays_apart() {
    let key = |i: i64, t: Type| ShapeField::new(ShapeKey::Int(i), t, false);
    let bare = Type::array_shape(Shape::sealed(vec![key(0, Type::new(Repr::String))]));
    let with = Type::array_shape(Shape::sealed(vec![
        key(0, Type::new(Repr::String)),
        key(1, lit("ab")),
        key(2, lit("b")),
    ]));
    assert_eq!(
        Type::union(with.clone(), bare.clone()),
        Type::new(Repr::Union(vec![with, bare]))
    );
}

#[test]
fn an_error_absorbs_a_union() {
    assert_eq!(Type::union(Type::error(), Type::int()), Type::error());
    assert_eq!(
        Type::union(
            Type::new(Repr::Union(vec![Type::int(), Type::error()])),
            Type::string()
        ),
        Type::error()
    );
}
