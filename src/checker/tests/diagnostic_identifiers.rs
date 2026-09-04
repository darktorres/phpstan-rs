//! Phase 47: `Diagnostic` grew an `identifier`/`tip`/`non_ignorable`/
//! `metadata`/`fix` beyond its original 4 fields, and `DiagnosticBuilder`
//! is the fluent way to assemble one. These tests exercise both halves:
//! that real checker call sites actually populate `identifier` (and, for
//! the couple of call sites that attach one, `tip`) correctly, and that
//! `DiagnosticBuilder` itself behaves as a plain, independently-testable
//! unit.

use super::helpers::*;
use crate::checker::DiagnosticBuilder;
use crate::token::{Position, Span};

fn dummy_span() -> Span {
    Span {
        start: Position { line: 1, col: 1 },
        end: Position { line: 1, col: 1 },
    }
}

#[test]
fn undefined_variable_carries_the_variable_undefined_identifier() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                echo $undefinedVariable;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "variable.undefined");
}

#[test]
fn defined_variable_is_not_flagged_at_all() {
    // Negative case for the above: a variable that genuinely is bound
    // (a parameter) must never be flagged, identifier or not.
    let d = diagnostics_for(
        r#"<?php
            function f(int $x) {
                echo $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn undefined_function_call_carries_the_function_not_found_identifier() {
    let d = diagnostics_for(
        r#"<?php
            totallyUndefinedFunction();
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "function.notFound");
}

#[test]
fn argument_count_mismatch_carries_the_arguments_count_identifier() {
    let d = diagnostics_for(
        r#"<?php
            function needsTwo(int $a, int $b): int {
                return $a + $b;
            }
            needsTwo(1);
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "arguments.count" && x.message.contains("needsTwo")),
        "got: {d:?}"
    );
}

#[test]
fn argument_type_mismatch_carries_the_argument_type_identifier() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int {
                return $x;
            }
            needsInt("not an int");
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_fully_correct_call_produces_no_diagnostics() {
    // Negative case for both argument checks above.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int {
                return $x;
            }
            needsInt(1);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn return_type_mismatch_carries_the_return_type_identifier() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                return "not an int";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "got: {d:?}"
    );
}

#[test]
fn readonly_property_write_outside_constructor_carries_identifier_and_tip() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(public readonly int $x) {}
                public function moveTo(int $newX): void {
                    $this->x = $newX;
                }
            }
        "#,
    );
    let found = d
        .iter()
        .find(|x| x.identifier == "property.readOnlyAssignNotInConstructor");
    let found = found.unwrap_or_else(|| panic!("expected a readonly-write diagnostic, got: {d:?}"));
    assert!(
        found.tip.is_some(),
        "expected the readonly-write diagnostic to carry a tip"
    );
    assert!(found.tip.as_deref().unwrap().contains("constructor"));
}

#[test]
fn readonly_property_written_from_its_own_constructor_is_not_flagged() {
    // Negative case for the above: writing a readonly property from
    // inside the declaring class's own constructor is exactly the one
    // legitimate write real PHP allows.
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public readonly int $x;
                public function __construct(int $x) {
                    $this->x = $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instantiating_an_abstract_class_carries_identifier_and_tip() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Shape {
                abstract public function area(): float;
            }
            function f() {
                new Shape();
            }
        "#,
    );
    let found = d.iter().find(|x| x.identifier == "new.abstract");
    let found = found
        .unwrap_or_else(|| panic!("expected an abstract-instantiation diagnostic, got: {d:?}"));
    assert!(found.tip.is_some());
}

#[test]
fn ordinary_diagnostics_default_to_non_ignorable_false() {
    // Nothing in this project sets `non_ignorable` yet (Phase 44, which
    // would consume it, doesn't exist) - every existing call site's
    // diagnostics should still default to `false`, not silently become
    // unignorable by accident of the Phase 47 struct change.
    let d = diagnostics_for(
        r#"<?php
            function f() {
                echo $undefinedVariable;
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(!d[0].non_ignorable);
    assert!(d[0].metadata.is_empty());
    assert!(d[0].fix.is_none());
}

#[test]
fn diagnostic_builder_assembles_every_field() {
    // A direct, checker-independent unit test of the builder itself -
    // the Rust equivalent of real PHPStan's own
    // `RuleErrorBuilder::message(...)->identifier(...)->tip(...)->build()`
    // chain.
    let diag = DiagnosticBuilder::new(
        "MyClass::myMethod".to_string(),
        "something went wrong".to_string(),
        dummy_span(),
        5,
        "my.identifier",
    )
    .tip("try this instead")
    .non_ignorable()
    .metadata("count", 3)
    .fix(dummy_span(), "replacement text", "swap it out")
    .build();

    assert_eq!(diag.location, "MyClass::myMethod");
    assert_eq!(diag.message, "something went wrong");
    assert_eq!(diag.level, 5);
    assert_eq!(diag.identifier, "my.identifier");
    assert_eq!(diag.tip.as_deref(), Some("try this instead"));
    assert!(diag.non_ignorable);
    assert_eq!(diag.metadata.get("count").and_then(|v| v.as_i64()), Some(3));
    let fix = diag.fix.expect("expected a fix to have been set");
    assert_eq!(fix.replacement, "replacement text");
    assert_eq!(fix.description, "swap it out");
}

// Phase 37: `AttributesCheck`'s existence half - `Checker::check_attributes`,
// wired at every declaration site an `Attribute` list can now appear on.

#[test]
fn an_attribute_naming_an_undefined_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[TotallyUndefinedAttribute]
            class Foo {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "attribute.notFound");
}

#[test]
fn an_attribute_naming_a_real_declared_class_is_not_flagged() {
    // Negative case for the above: a genuinely known class (declared
    // right there in the same file) must not be flagged just because
    // it's being used as an attribute.
    let d = diagnostics_for(
        r#"<?php
            class MyAttr {}

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_on_a_method_parameter_naming_an_undefined_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                function f(#[TotallyUndefinedAttribute] string $x) {}
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "attribute.notFound");
}

#[test]
fn an_attribute_naming_a_real_builtin_attribute_class_is_not_flagged() {
    // `Attribute` itself is a real PHP builtin (from the vendored
    // phpstorm-stubs corpus, not user code) - a genuinely known class,
    // not just a plausible-looking name.
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 37: PHP 8.5's `#[\NoDiscard]` marker -
// `Checker::check_no_discard_statement`, mirroring real PHPStan's own
// `CallToFunctionStatementWithNoDiscardRule`/
// `CallToMethodStatementWithNoDiscardRule`/
// `CallToStaticMethodStatementWithNoDiscardRule`.

#[test]
fn a_bare_call_to_a_no_discard_function_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[NoDiscard]
            function f(): int { return 1; }
            f();
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "function.resultDiscarded");
    assert!(d[0].non_ignorable);
}

#[test]
fn using_a_no_discard_function_s_return_value_is_not_flagged() {
    // Negative case: the exact same function, but its return value is
    // actually consumed this time.
    let d = diagnostics_for(
        r#"<?php
            #[NoDiscard]
            function f(): int { return 1; }
            $x = f();
            echo $x;
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_call_to_an_ordinary_function_with_no_marker_is_not_flagged() {
    // False-positive guard: an otherwise-identical bare-statement call
    // with no `#[NoDiscard]` attribute at all must never be flagged.
    let d = diagnostics_for(
        r#"<?php
            function f(): int { return 1; }
            f();
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_call_to_a_no_discard_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                #[NoDiscard]
                function m(): int { return 1; }
            }
            function test(C $c) {
                $c->m();
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "method.resultDiscarded");
}

#[test]
fn a_bare_call_to_a_no_discard_static_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                #[NoDiscard]
                static function m(): int { return 1; }
            }
            C::m();
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "staticMethod.resultDiscarded");
}

#[test]
fn diagnostic_builder_defaults_are_all_empty_absent_or_false() {
    // The other half of the above: a builder with only the required
    // fields set should never spuriously populate any of the optional
    // ones.
    let diag = DiagnosticBuilder::new(
        "<main>".to_string(),
        "plain message".to_string(),
        dummy_span(),
        0,
        "plain.identifier",
    )
    .build();

    assert!(diag.tip.is_none());
    assert!(!diag.non_ignorable);
    assert!(diag.metadata.is_empty());
    assert!(diag.fix.is_none());
}
