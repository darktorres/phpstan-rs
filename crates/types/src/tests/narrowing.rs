//! Narrowing: what was known meets what a check proves.
use crate::{AccessoryKind, Relation, Type};

fn narrow(original: &Type, target: &Type) -> Type {
    Relation::structural().narrow(original, target)
}

#[test]
fn narrowing_keeps_what_was_already_known() {
    let non_empty = Type::string().with_string_accessory(AccessoryKind::NonEmptyString);
    let target = Type::union_of(vec![
        Type::string_literal("a".into()),
        Type::string_literal("".into()),
        Type::int(),
    ]);
    assert_eq!(
        narrow(&non_empty, &target),
        Type::string_literal("a".into())
    );
    assert_eq!(narrow(&non_empty, &Type::string()), non_empty);
}

#[test]
fn narrowing_a_range_meets_the_target_range() {
    let range = Type::int_range(Some(0), Some(10));
    let target = Type::int_range(Some(5), Some(20));
    assert_eq!(narrow(&range, &target), Type::int_range(Some(5), Some(10)));
    let union = Type::union(range, Type::null());
    assert_eq!(narrow(&union, &target), Type::int_range(Some(5), Some(10)));
}

#[test]
fn a_check_nothing_can_pass_leaves_never() {
    assert!(narrow(&Type::int(), &Type::string()).is_never());
    assert!(narrow(&Type::union(Type::int(), Type::float()), &Type::string()).is_never());
}

fn cut(current: Type, op: crate::OrderingOp, other: Type, holds: bool) -> Type {
    Relation::structural().narrow_ordering(
        &current,
        op,
        &other,
        holds,
        Some(crate::PhpVersion::new(8, 5)),
    )
}

#[test]
fn an_ordering_cuts_an_int_by_the_other_sides_bounds() {
    use crate::OrderingOp::*;
    let n = Type::int_range(Some(0), Some(10));
    assert_eq!(
        cut(Type::int(), Lt, n.clone(), true),
        Type::int_range(None, Some(9))
    );
    assert_eq!(
        cut(Type::int(), Lt, n.clone(), false),
        Type::int_range(Some(0), None)
    );
    assert_eq!(
        cut(Type::int(), LtEq, n.clone(), true),
        Type::int_range(None, Some(10))
    );
    assert_eq!(
        cut(Type::int(), LtEq, n.clone(), false),
        Type::int_range(Some(1), None)
    );
    assert_eq!(
        cut(Type::int(), Gt, n.clone(), true),
        Type::int_range(Some(1), None)
    );
    assert_eq!(
        cut(Type::int(), Gt, n.clone(), false),
        Type::int_range(None, Some(10))
    );
    assert_eq!(
        cut(Type::int(), GtEq, n.clone(), true),
        Type::int_range(Some(0), None)
    );
    assert_eq!(
        cut(Type::int(), GtEq, n, false),
        Type::int_range(None, Some(9))
    );
}

#[test]
fn a_float_bound_rounds_inward_and_nan_fails_every_ordering() {
    use crate::OrderingOp::*;
    assert_eq!(
        cut(Type::int(), Lt, Type::float_literal(2.5), true),
        Type::int_range(None, Some(2))
    );
    assert_eq!(
        cut(Type::int(), Gt, Type::float_literal(2.5), true),
        Type::int_range(Some(3), None)
    );
    assert!(cut(Type::int(), Lt, Type::float_literal(f64::NAN), true).is_never());
}

#[test]
fn an_ordering_keeps_the_members_that_may_satisfy_it() {
    use crate::OrderingOp::*;
    let nullable = Type::union(Type::int(), Type::null());
    assert_eq!(
        cut(nullable.clone(), Lt, Type::int_literal(5), true),
        Type::union(Type::int_range(None, Some(4)), Type::null())
    );
    assert_eq!(
        cut(nullable, Lt, Type::int_literal(5), false),
        Type::int_range(Some(5), None)
    );
}
