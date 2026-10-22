//! Data-flow sets carried on types: identity, and what union, intersection,
//! widening and generalizing keep.
use crate::{Flow, Type};

fn env() -> Flow {
    Flow::of([1])
}

fn param() -> Flow {
    Flow::of([2])
}

#[test]
fn a_set_is_neither_a_types_identity_nor_its_display() {
    let plain = Type::int();
    let traced = plain.with_flow(&env());
    assert_eq!(plain, traced);
    assert_eq!(traced.deep_flow(), env());
    assert_eq!(traced.to_string(), "int");
    assert!(traced.has_flow());
    assert!(!plain.has_flow());
}

#[test]
fn a_set_reaches_scalar_parts_and_array_values_only() {
    let ty = Type::union(Type::int(), Type::null()).with_flow(&env());
    assert_eq!(ty.scalar_flow(), env());
    let array = Type::array(Type::int(), Type::string()).with_flow(&env());
    assert!(array.has_flow());
    assert!(array.scalar_flow().is_empty());
    assert_eq!(array.array_value_type().unwrap().scalar_flow(), env());
}

#[test]
fn a_union_keeps_each_members_set_on_the_member_of_its_kind() {
    let range = Type::int_range(Some(0), Some(5)).with_flow(&env());
    let int = Type::int().with_flow(&param());
    let union = Type::union(range, int);
    assert_eq!(union.to_string(), "int");
    assert_eq!(union.scalar_flow(), Flow::of([1, 2]));
    let mixed_with_string = Type::union(Type::string().with_flow(&env()), Type::int());
    assert!(
        mixed_with_string
            .union_members()
            .iter()
            .any(|m| m.is_int().yes() && m.scalar_flow().is_empty())
    );
}

#[test]
fn narrowing_a_traced_mixed_to_an_int_keeps_its_set() {
    let narrowed = crate::intersect_types(vec![Type::mixed().with_flow(&env()), Type::int()]);
    assert!(narrowed.is_int().yes());
    assert_eq!(narrowed.scalar_flow(), env());
    let bounded = crate::intersect_types(vec![
        Type::int().with_flow(&env()),
        Type::int_range(Some(0), Some(99)),
    ]);
    assert_eq!(bounded.int_hull(), Some((Some(0), Some(99))));
    assert_eq!(bounded.scalar_flow(), env());
}

#[test]
fn widening_and_generalizing_keep_the_set() {
    let before = Type::int_literal(0).with_flow(&env());
    let next = Type::union(Type::int_literal(0), Type::int_literal(1)).with_flow(&param());
    assert_eq!(before.widen_towards(&next).scalar_flow(), Flow::of([1, 2]));
    assert_eq!(
        Type::int_literal(3)
            .with_flow(&env())
            .generalize()
            .scalar_flow(),
        env()
    );
}
