//! Enum registration: cases, backing types, and implemented interfaces.

use super::super::*;
use super::helpers::*;

// Phase 79: a case constant's own singleton type, and the reverse
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
        r.lookup_const("Suit", "Hearts"),
        Some(&Type::EnumCase("Suit".to_string(), "Hearts".to_string()))
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
        Some(&Type::StringLiteral("H".to_string()))
    );
    assert_eq!(
        r.enum_case_for_value("Suit", &Type::StringLiteral("S".to_string())),
        Some("Spades")
    );
    assert_eq!(
        r.enum_case_for_value("Suit", &Type::StringLiteral("Z".to_string())),
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
        Some(&Type::StringLiteral("BAR".to_string()))
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
        r.lookup_const("Suit", "KIND"),
        Some(&Type::StringLiteral("hearts".to_string()))
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
        Type::String
    );
}

#[test]
fn every_enum_implements_unit_enum_and_a_backed_one_also_implements_backed_enum() {
    // Real bug found on the real Piwigo corpus: doctrine/lexer's own
    // `AbstractLexer<T of UnitEnum|string|int>` flagged every real
    // backed enum passed to it as a bound violation, since nothing
    // ever registered an enum as implementing `UnitEnum`/`BackedEnum`
    // - only their synthesized cases()/name/value members existed,
    // never the hierarchy relationship itself.
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
}

// Phase 34 Finding C: a backed enum's ->value is the union of its
// own case literals, not the generic backing type - real shape,
// Piwigo\Common\Enum\SortOrder.
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
        Type::Union(vec![
            Type::StringLiteral("ASC".to_string()),
            Type::StringLiteral("DESC".to_string()),
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
        Type::StringLiteral("ONLY".to_string())
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
        Type::Union(vec![Type::IntLiteral(1), Type::IntLiteral(0)])
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
        Type::String
    );
}
