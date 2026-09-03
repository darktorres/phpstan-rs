//! Enum cases, backed enums, and enum method bodies.

use super::super::*;
use super::helpers::*;

#[test]
fn enum_gets_its_built_in_cases_and_backed_enum_gets_from_and_try_from() {
    // Real, built-in static methods every enum has from the language
    // itself (UnitEnum::cases(); BackedEnum::from()/tryFrom() only on
    // a backed one) - none declared in source. The very first
    // real-corpus check after shipping undefined-static-method
    // detection flagged every single real Enum::cases()/::tryFrom()
    // call in the wild before this was added.
    let d = diagnostics_for(
        r#"<?php
            enum Status: string {
                case Active = 'active';
            }
            enum Pure {
                case A;
            }
            function f() {
                foreach (Status::cases() as $s) {
                    echo $s->value;
                }
                Status::from('active');
                Status::tryFrom('nope');
                Pure::cases();
                Pure::from('x');
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message
            .contains("undefined static method Pure::from()"),
        "a non-backed enum should not have from()/tryFrom(): {d:?}"
    );
}

#[test]
fn enum_cases_are_recognized_by_static_access_and_a_bogus_case_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                case Inactive;
            }
            function f() {
                $s = Status::Active;
                echo Status::MadeUpCase;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("undefined constant Status::MadeUpCase")),
        "got: {d:?}"
    );
    assert_eq!(d.len(), 1, "Status::Active should not be flagged: {d:?}");
}

#[test]
fn a_backed_enum_s_value_satisfies_a_matching_literal_union_parameter() {
    // Phase 34 Finding C: Piwigo\Db\SortRenderer's real shape -
    // OrderByClause::__construct(string $column, 'ASC'|'DESC' $dir).
    let d = diagnostics_for(
        r#"<?php
            enum SortOrder: string {
                case Asc = 'ASC';
                case Desc = 'DESC';
            }
            class OrderByClause {
                /** @param 'ASC'|'DESC' $dir */
                public function __construct(string $column, string $dir) {}
            }
            function render(SortOrder $order): OrderByClause {
                return new OrderByClause('col', $order->value);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn enum_method_body_is_type_checked() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            enum Suit: string {
                case Hearts = 'H';
                public function bad(): string {
                    return needsInt("not an int");
                }
            }
        "#,
    );
    assert!(d.iter().any(|d| d.message.contains("needsInt")));
}

#[test]
fn enum_method_with_correct_usage_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            enum Suit: string {
                case Hearts = 'H';
                public function good(): int {
                    return needsInt(5);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_backed_enum_with_a_duplicate_int_value_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status3: int {
                case Active = 1;
                case Inactive = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("duplicate value in enum Status3 for case Inactive")),
        "expected a duplicate-enum-value diagnostic, got: {d:?}"
    );
}

#[test]
fn a_backed_enum_with_a_duplicate_string_value_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'H';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("duplicate value in enum Suit for case Spades")),
        "expected a duplicate-enum-value diagnostic, got: {d:?}"
    );
}

#[test]
fn a_backed_enum_with_all_unique_values_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum StatusOk: int {
                case Active = 1;
                case Inactive = 2;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpstan_enum_sanity_flags_duplicate_backing_values() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Classes/data/enum-sanity.php's
    // `EnumDuplicateValue` case - upstream's own expected messages are
    // "has duplicate value 1 for cases A, E." and "... value 2 for
    // cases B, C.".
    let d = diagnostics_for(
        r#"<?php
            enum EnumDuplicateValue: int {
                case A = 1;
                case B = 2;
                case C = 2;
                case D = 3;
                case E = 1;
            }
        "#,
    );
    let dupes: Vec<&Diagnostic> = d
        .iter()
        .filter(|x| {
            x.message
                .contains("duplicate value in enum EnumDuplicateValue")
        })
        .collect();
    assert_eq!(
        dupes.len(),
        2,
        "expected 2 duplicate-value diagnostics, got: {d:?}"
    );
    assert!(dupes.iter().any(|x| x.message.contains("for case C")));
    assert!(dupes.iter().any(|x| x.message.contains("for case E")));
}
