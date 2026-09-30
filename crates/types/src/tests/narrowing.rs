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
