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

// ---- Phase 66: the rest of EnumSanityRule, generic rejection, and
// generic-bound wiring - all confirmed directly against real `php -l`
// (constructor/destructor/magic-method/redeclaration/backing-type/
// case-value shapes) or matched against real PHPStan's own
// EnumTemplateTypeRule (Serializable and @template aren't PHP parse
// errors, just real-PHPStan-flagged design errors).

#[test]
fn an_enum_constructor_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                public function __construct() {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.constructor"),
        "got: {d:?}"
    );
}

#[test]
fn an_enum_destructor_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                public function __destruct() {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.destructor"),
        "got: {d:?}"
    );
}

#[test]
fn an_enum_magic_get_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                public function __get($name) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.magicMethod"),
        "got: {d:?}"
    );
}

#[test]
fn an_enum_call_method_is_never_flagged_as_magic() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                public function __call($name, $args) {}
                public static function __callStatic($name, $args) {}
                public function __invoke() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.magicMethod"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_enum_redeclaring_cases_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                public static function cases(): array { return []; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.methodRedeclaration"),
        "got: {d:?}"
    );
}

#[test]
fn a_pure_enum_redeclaring_from_is_never_flagged() {
    // `from()`/`tryFrom()` only exist on a *backed* enum - a pure
    // enum's own `from()` isn't a native-method collision at all.
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                public static function from($value): self { return self::Active; }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.methodRedeclaration"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_backed_enum_redeclaring_try_from_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status: int {
                case Active = 1;
                public static function tryFrom($value): ?self { return null; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.methodRedeclaration"),
        "got: {d:?}"
    );
}

#[test]
fn a_backed_enum_with_a_float_backing_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status: float {
                case Active;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.backingType"),
        "got: {d:?}"
    );
}

#[test]
fn a_backed_enum_with_an_int_backing_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status: int {
                case Active = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.backingType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_enum_implementing_serializable_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status implements \Serializable {
                case Active;
                public function serialize(): ?string { return null; }
                public function unserialize($data): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.serializable"),
        "got: {d:?}"
    );
}

#[test]
fn a_plain_enum_is_never_flagged_as_serializable() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.serializable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_backed_enum_case_with_a_value_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.caseWithValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_backed_enum_case_missing_a_value_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status: int {
                case Active;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.missingCase"),
        "got: {d:?}"
    );
}

#[test]
fn a_string_value_on_an_int_backed_enum_case_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status: int {
                case Active = 'active';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.caseType"),
        "got: {d:?}"
    );
}

#[test]
fn an_int_value_on_a_string_backed_enum_case_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status: string {
                case Active = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.caseType"),
        "got: {d:?}"
    );
}

#[test]
fn a_matching_case_value_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status: int {
                case Active = 1;
            }
            enum Kind: string {
                case A = 'a';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.caseType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_case_value_referencing_another_constant_is_not_guessed_at() {
    // `self::SOME_CONST`-shaped case values aren't literals this
    // project's own const-value inference resolves - skipped rather
    // than flagged, matching the "don't guess" leniency
    // `check_enum_case_values`'s own duplicate-value detection already
    // applies to the same kind of non-literal value.
    let d = diagnostics_for(
        r#"<?php
            enum Status: int {
                const BASE = 1;
                case Active = self::BASE;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.caseType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_template_tag_on_an_enum_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            enum Status {
                case Active;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.generic"),
        "got: {d:?}"
    );
}

#[test]
fn an_enum_with_no_template_tag_is_never_flagged_as_generic() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * A perfectly ordinary enum.
             */
            enum Status {
                case Active;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.generic"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 79: a specific enum case's own singleton type - `Suit::Hearts`'s
// real static type is `Suit::Hearts`, not the coarse `Suit` every case
// used to collapse to - and `->name`/`->value` on a case fetched this
// way narrow to their own exact literal rather than the enum-wide
// generic string/union-of-every-case's-value.

#[test]
fn a_case_fetch_has_its_own_singleton_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit {
                case Hearts;
                case Spades;
            }
            function f(): void {
                assertType('Suit::Hearts', Suit::Hearts);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_case_fetch_s_name_property_is_its_own_literal() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit {
                case Hearts;
                case Spades;
            }
            function f(): void {
                assertType("'Hearts'", Suit::Hearts->name);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_case_fetch_s_value_property_is_its_own_literal() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            function f(): void {
                assertType("'H'", Suit::Hearts->value);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_case_singleton_is_still_assignable_to_its_own_enum_type() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit {
                case Hearts;
            }
            function f(Suit $s): void {}
            f(Suit::Hearts);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_case_singleton_is_still_assignable_to_unit_enum() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit {
                case Hearts;
            }
            function f(UnitEnum $s): void {}
            f(Suit::Hearts);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_case_singleton_still_supports_its_enum_s_own_methods() {
    // Member lookup on a specific case must still see every method the
    // enum itself declares - `EnumCase` is a real instance of its enum
    // for that purpose, only `->name`/`->value`/case identity are
    // case-specific.
    let d = diagnostics_for(
        r#"<?php
            enum Suit {
                case Hearts;
                public function label(): string { return "Hearts"; }
            }
            function f(): void {
                Suit::Hearts->label();
                Suit::Hearts->missing();
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.notFound" && x.message.contains("label")),
        "unexpected diagnostics: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.notFound" && x.message.contains("missing")),
        "got: {d:?}"
    );
}

#[test]
fn from_with_a_literal_matching_a_case_narrows_to_that_case() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            function f(): void {
                assertType('Suit::Hearts', Suit::from('H'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn try_from_with_a_literal_matching_a_case_narrows_to_that_case_not_nullable() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            function f(): void {
                assertType('Suit::Hearts', Suit::tryFrom('H'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn from_with_a_literal_matching_no_known_case_falls_back_to_the_generic_stub() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            function f(): void {
                assertType('Suit', Suit::from('Z'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn from_with_a_non_literal_argument_falls_back_to_the_generic_stub() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'S';
            }
            function f(string $x): void {
                assertType('Suit', Suit::from($x));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn equality_narrowing_gives_the_exact_case_singleton_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum Suit {
                case Hearts;
                case Spades;
            }
            function f(Suit $s): void {
                if ($s === Suit::Hearts) {
                    assertType('Suit::Hearts', $s);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
