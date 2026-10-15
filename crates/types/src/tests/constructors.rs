//! The public constructors build normalized types.

use crate::{AccessoryKind, Type};

#[test]
fn union_of_absorbs_covered_members_and_is_never_when_empty() {
    assert_eq!(
        Type::union_of(vec![Type::int(), Type::int_literal(1)]),
        Type::int()
    );
    assert_eq!(Type::union_of(Vec::new()), Type::never());
    assert_eq!(
        Type::union_of(vec![Type::int(), Type::string()]).to_string(),
        "int|string"
    );
}

#[test]
fn intersection_of_keeps_a_refinement_and_folds_arrays() {
    let non_empty = Type::intersection_of(vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonEmptyString),
    ]);
    assert_eq!(non_empty.to_string(), "non-empty-string");
    let list = Type::intersection_of(vec![
        Type::array(Type::unknown_array_key(), Type::int()),
        Type::accessory(AccessoryKind::ArrayList),
    ]);
    assert_eq!(list.to_string(), "list<int>");
}

#[test]
fn benevolent_union_of_stays_benevolent_when_members_remain() {
    let key = Type::benevolent_union_of(vec![Type::int(), Type::string()]);
    assert!(key.is_benevolent_union());
    assert_eq!(Type::benevolent_union_of(vec![Type::int()]), Type::int());
}

#[test]
fn an_integer_literal_is_the_same_type_inside_the_small_table_and_outside_it() {
    for value in [-3, -2, -1, 0, 1, 7, 254, 255, 256, 257, i64::MAX, i64::MIN] {
        let literal = Type::int_literal(value);
        assert_eq!(
            literal,
            Type::new(crate::Repr::IntLiteral(value)),
            "{value}"
        );
        assert_eq!(literal.int_literal_value(), Some(value), "{value}");
        assert_eq!(Type::int_literal(value), literal, "{value}");
    }
}
