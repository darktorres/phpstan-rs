//! Enum registration: cases, backing types, and implemented interfaces.

// Expected types are built without a class hierarchy.
#![allow(clippy::disallowed_methods)]

use super::super::*;
use super::helpers::*;

// A case constant's own singleton type, and the reverse
// literal-value -> case lookup `::from()`/`::tryFrom()` narrowing needs.

#[test]
fn a_case_constant_s_type_is_its_own_singleton() {
    let r = build_registry(
        r#"<?php
            enum Suit {
                case Hearts;
            }"#,
    );
    assert_eq!(
        r.lookup_const("Suit", "Hearts").map(|cs| &cs.ty),
        Some(&Type::enum_case("Suit".into(), "Hearts".into()))
    );
}

#[test]
fn enum_case_value_and_its_reverse_lookup_agree() {
    let r = build_registry(
        r#"<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }"#,
    );
    assert_eq!(
        r.enum_case_value("Suit", "Hearts"),
        Some(&Type::string_literal("H".into()))
    );
    assert_eq!(
        r.enum_case_for_value("Suit", &Type::string_literal("S".into())),
        Some("Spades")
    );
    assert_eq!(
        r.enum_case_for_value("Suit", &Type::string_literal("Z".into())),
        None
    );
}

#[test]
fn a_non_literal_case_value_never_appears_in_enum_case_values() {
    let r = build_registry(
        r#"<?php
            class C { const FOO = 'FOO'; }
            enum WithConst: string {
                case Foo = C::FOO;
                case Bar = 'BAR';
            }"#,
    );
    assert_eq!(r.enum_case_value("WithConst", "Foo"), None);
    assert_eq!(
        r.enum_case_value("WithConst", "Bar"),
        Some(&Type::string_literal("BAR".into()))
    );
}

#[test]
fn enum_consts_are_registered_too() {
    let r = build_registry(
        r#"<?php
            enum Suit {
                case Hearts;
                const KIND = "hearts";
            }"#,
    );
    assert_eq!(
        r.lookup_const("Suit", "KIND").map(|cs| &cs.ty),
        Some(&Type::string_literal("hearts".into()))
    );
}

#[test]
fn enum_is_registered_with_its_methods_and_implements() {
    let r = build_registry(
        r#"<?php
            interface HasLabel {}
            enum Suit: string implements HasLabel {
                case Hearts = 'H';
                public function label(): string { return "Hearts"; }
            }
        "#,
    );
    assert!(r.is_subclass_of("Suit", "HasLabel"));
    assert_eq!(
        r.lookup_method("Suit", "label").unwrap().return_type,
        Type::string()
    );
}

#[test]
fn every_enum_implements_unit_enum_and_a_backed_one_also_implements_backed_enum() {
    // doctrine/lexer's own `AbstractLexer<T of UnitEnum|string|int>`
    // requires every backed enum to implement `UnitEnum`/`BackedEnum`;
    // the synthesized cases()/name/value members alone aren't enough.
    let r = build_registry(
        r#"<?php
            enum Suit: string {
                case Hearts = 'H';
            }
            enum Pure {
                case A;
            }
        "#,
    );
    assert!(r.is_subclass_of("Suit", "UnitEnum"));
    assert!(r.is_subclass_of("Suit", "BackedEnum"));
    assert!(r.is_subclass_of("Pure", "UnitEnum"));
    assert!(!r.is_subclass_of("Pure", "BackedEnum"));
    assert!(r.class_by_name("Pure").unwrap().is_static_method("cases"));
    assert!(r.class_by_name("Suit").unwrap().is_static_method("from"));
    assert!(r.class_by_name("Suit").unwrap().is_static_method("tryFrom"));
}

// A backed enum's ->value is the union of its own case literals, not
// the generic backing type - real shape, Piwigo\Common\Enum\SortOrder.
#[test]
fn a_backed_enum_s_value_property_is_the_union_of_its_case_literals() {
    let r = build_registry(
        r#"<?php
            enum SortOrder: string {
                case Asc = 'ASC';
                case Desc = 'DESC';
            }
        "#,
    );
    assert_eq!(
        r.lookup_property("SortOrder", "value").unwrap().ty,
        Type::union_of(vec![
            Type::string_literal("ASC".into()),
            Type::string_literal("DESC".into()),
        ])
    );
}

#[test]
fn a_single_case_backed_enum_s_value_is_the_bare_literal_not_a_one_member_union() {
    let r = build_registry(
        r#"<?php
            enum OnlyOne: string {
                case Only = 'ONLY';
            }
        "#,
    );
    assert_eq!(
        r.lookup_property("OnlyOne", "value").unwrap().ty,
        Type::string_literal("ONLY".into())
    );
}

#[test]
fn a_backed_int_enum_s_value_is_the_union_of_its_int_case_literals() {
    let r = build_registry(
        r#"<?php
            enum Status: int {
                case Active = 1;
                case Inactive = 0;
            }
        "#,
    );
    assert_eq!(
        r.lookup_property("Status", "value").unwrap().ty,
        Type::union_of(vec![Type::int_literal(1), Type::int_literal(0)])
    );
}

#[test]
fn a_backed_enum_with_a_non_literal_case_value_falls_back_to_the_plain_backing_type() {
    let r = build_registry(
        r#"<?php
            class C { const FOO = 'FOO'; }
            enum WithConst: string {
                case Foo = C::FOO;
                case Bar = 'BAR';
            }
        "#,
    );
    assert_eq!(
        r.lookup_property("WithConst", "value").unwrap().ty,
        Type::string()
    );
}
