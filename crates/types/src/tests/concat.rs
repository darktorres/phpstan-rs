//! `concat_types`: what concatenating two types gives.

use crate::{AccessoryKind, NoClasses, PhpString, Type, concat_types};

fn concat(l: &Type, r: &Type) -> Type {
    concat_types(l, r, &NoClasses)
}

#[test]
fn a_float_literal_operand_uses_the_php_float_formatter() {
    assert_eq!(
        concat(
            &Type::float_literal(1.5),
            &Type::string_literal("-suffix".into())
        ),
        Type::string_literal("1.5-suffix".into())
    );
}

#[test]
fn two_literal_strings_fully_constant_fold() {
    assert_eq!(
        concat(
            &Type::string_literal("a".into()),
            &Type::string_literal("b".into())
        ),
        Type::string_literal("ab".into())
    );
}

#[test]
fn an_empty_string_operand_is_dropped() {
    assert_eq!(
        concat(&Type::string_literal(PhpString::default()), &Type::int()),
        Type::int().to_string_type(&NoClasses).unwrap()
    );
}

#[test]
fn an_int_left_operand_with_a_digit_literal_right_is_numeric() {
    let result = concat(&Type::int(), &Type::string_literal("5".into()));
    assert!(result.is_refined_string(AccessoryKind::NumericString).yes());
}

#[test]
fn an_int_left_operand_with_a_negative_literal_right_is_not_numeric() {
    let result = concat(&Type::int(), &Type::string_literal("-5".into()));
    assert!(!result.is_refined_string(AccessoryKind::NumericString).yes());
}

#[test]
fn a_float_is_uppercase_and_non_empty_but_not_numeric() {
    let s = Type::float().to_string_type(&NoClasses).unwrap();
    assert!(s.is_refined_string(AccessoryKind::UppercaseString).yes());
    assert!(s.is_refined_string(AccessoryKind::NonEmptyString).yes());
    assert!(!s.is_refined_string(AccessoryKind::NumericString).yes());
}

#[test]
fn an_enum_case_has_no_string_form_and_an_object_may_have_one() {
    assert_eq!(
        Type::enum_case("E".into(), "A".into()).to_string_type(&NoClasses),
        None
    );
    assert!(
        Type::object_without_class()
            .to_string_type(&NoClasses)
            .is_some()
    );
}
