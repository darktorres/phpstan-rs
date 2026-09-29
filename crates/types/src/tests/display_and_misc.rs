//! Display/formatting of `Type` values back to PHPStan-style syntax.
use super::super::*;
use crate::Repr;

#[test]
fn array_shape_display_round_trips_readably() {
    let shape = parse_type_string("array{name: string, age?: int}");
    assert_eq!(shape.to_string(), "array{name: string, age?: int}");
}

#[test]
fn display_matches_phpstan_style_syntax() {
    assert_eq!(parse_type_string("?string").to_string(), "string|null");
    assert_eq!(parse_type_string("int|string").to_string(), "int|string");
    assert_eq!(parse_type_string("array").to_string(), "array");
}

#[test]
fn accessory_intersections_display_as_their_canonical_phpstan_name() {
    assert_eq!(
        parse_type_string("non-empty-string").to_string(),
        "non-empty-string"
    );
    assert_eq!(
        parse_type_string("non-falsy-string").to_string(),
        "non-falsy-string"
    );
    assert_eq!(
        parse_type_string("literal-string").to_string(),
        "literal-string"
    );
    assert_eq!(
        parse_type_string("numeric-string").to_string(),
        "numeric-string"
    );
    assert_eq!(parse_type_string("list<int>").to_string(), "list<int>");
    assert_eq!(parse_type_string("list").to_string(), "list");
    assert_eq!(
        parse_type_string("non-empty-array<int>").to_string(),
        "non-empty-array<int>"
    );
    assert_eq!(
        parse_type_string("non-empty-list<int>").to_string(),
        "non-empty-list<int>"
    );
}

#[test]
fn a_real_class_intersection_still_displays_amp_joined() {
    assert_eq!(
        parse_type_string("Countable&Iterator").to_string(),
        "Countable&Iterator"
    );
}

#[test]
fn int_range_and_float_literal_display() {
    assert_eq!(parse_type_string("positive-int").to_string(), "int<1, max>");
    assert_eq!(parse_type_string("int<0, 255>").to_string(), "int<0, 255>");
    assert_eq!(
        parse_type_string("int<min, 10>").to_string(),
        "int<min, 10>"
    );
    assert_eq!(parse_type_string("1.5").to_string(), "1.5");
}

#[test]
fn equal_types_hash_alike() {
    use std::hash::{BuildHasher, BuildHasherDefault};
    let hash = |ty: &Type| BuildHasherDefault::<rustc_hash::FxHasher>::default().hash_one(ty);
    let pairs = [
        (Type::explicit_mixed(), Type::mixed()),
        (Type::float_literal(0.0), Type::float_literal(-0.0)),
        (
            Type::float_literal(f64::NAN),
            Type::float_literal(-f64::NAN),
        ),
        (
            Type::array(Type(Repr::Int), Type(Repr::Mixed(true, None))),
            Type::array(Type(Repr::Int), Type(Repr::Mixed(false, None))),
        ),
        (
            Type(Repr::Union(vec![Type::int(), Type::string()])),
            Type(Repr::Union(vec![Type::string(), Type::int()])),
        ),
        (
            Type(Repr::Intersection(vec![
                Type::object("A".into(), Vec::new()),
                Type::object("B".into(), Vec::new()),
            ])),
            Type(Repr::Intersection(vec![
                Type::object("B".into(), Vec::new()),
                Type::object("A".into(), Vec::new()),
            ])),
        ),
    ];
    for (a, b) in pairs {
        assert_eq!(a, b);
        assert_eq!(hash(&a), hash(&b), "{a:?} and {b:?}");
    }
    assert_ne!(Type::int_literal(1), Type::int_literal(2));
    assert_ne!(
        Type(Repr::Union(vec![Type::int(), Type::int(), Type::string()])),
        Type(Repr::Union(vec![
            Type::int(),
            Type::string(),
            Type::string()
        ])),
    );
}
