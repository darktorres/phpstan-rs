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

// Phase 37 follow-up (this session): constructor-argument validation -
// `resolve_call_args`/`check_args` reused directly for an attribute's own
// constructor call, the same machinery `ExprKind::New` already uses.

#[test]
fn an_attribute_with_no_constructor_given_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {}

            #[MyAttr(1, 2)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.noConstructor"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_class_inheriting_its_constructor_from_an_ancestor_is_not_flagged() {
    // Regression test for a real corpus false positive (PHPBench's own
    // `#[BeforeMethods]`, `class BeforeMethods extends
    // AbstractMethodsAttribute {}`) - the constructor lookup must walk
    // the ancestor chain (`Registry::lookup_method`), not just this
    // class's own declared methods.
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class BaseAttr {
                public function __construct(public string $name) {}
            }

            #[Attribute]
            class MyAttr extends BaseAttr {}

            #[MyAttr('reset')]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_with_no_constructor_given_no_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {}

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_missing_a_required_constructor_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b) {}
            }

            #[MyAttr(1)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_with_too_many_constructor_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a) {}
            }

            #[MyAttr(1, 2)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_constructor_argument_of_the_wrong_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a) {}
            }

            #[MyAttr("not an int")]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_constructor_called_with_the_correct_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b) {}
            }

            #[MyAttr(1, "x")]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_constructor_called_with_named_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b = "default") {}
            }

            #[MyAttr(a: 1)]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_abstract_attribute_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            abstract class MyAttr {}

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.abstract"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_class_with_a_non_public_constructor_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                private function __construct() {}
            }

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "attribute.constructorNotPublic"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_argument_referencing_an_undefined_class_constant_is_checked() {
    // Confirms attribute argument expressions are now actually visited by
    // `check_expr` (previously never checked at all - only the attribute
    // name itself was) - a real class's own genuinely undefined constant
    // (attribute arguments are constant expressions in real PHP, so a
    // plain `SomeClass::CONST` is exactly the shape that appears there)
    // should be caught the same way it would anywhere else.
    let d = diagnostics_for(
        r#"<?php
            class Real {}

            #[Attribute]
            class MyAttr {
                public function __construct(public $a) {}
            }

            #[MyAttr(Real::UNDEFINED_CONST)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.notFound"),
        "got: {d:?}"
    );
}

// Phase 37 follow-up: `attribute.notAttribute`/`attribute.target` - the
// vendored real `#[\Override]` stub declaration (`Attribute::
// TARGET_METHOD|TARGET_PROPERTY|TARGET_CLASS_CONSTANT`, confirmed
// directly against `vendor/phpstorm-stubs/Core/Core_c.php`) is real,
// already-registered data these tests exercise directly rather than
// needing a synthetic attribute class.

#[test]
fn using_override_on_a_plain_function_is_flagged_wrong_target() {
    // `Override`'s own real flags don't include `TARGET_FUNCTION`.
    let d = all_diagnostics_for(
        r#"<?php
            #[Override]
            function f(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class Override does not have the function target."),
        "got: {d:?}"
    );
}

#[test]
fn using_override_on_an_ordinary_method_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                #[Override]
                function f(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn using_override_on_an_ordinary_property_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                #[Override]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_attribute_on_a_property_naming_a_class_with_no_attribute_meta_is_flagged_not_an_attribute() {
    let d = all_diagnostics_for(
        r#"<?php
            class MyAttr {}

            class C {
                #[MyAttr]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.notAttribute"
            && x.message == "Class MyAttr is not an Attribute class."),
        "got: {d:?}"
    );
}

#[test]
fn a_user_defined_attribute_class_targeting_only_properties_used_on_a_method_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                #[PropOnly]
                function f(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class PropOnly does not have the method target."),
        "got: {d:?}"
    );
}

#[test]
fn a_user_defined_attribute_class_targeting_only_properties_used_on_a_property_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                #[PropOnly]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.target" || x.identifier == "attribute.notAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_bare_attribute_meta_with_no_flags_argument_defaults_to_target_all() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute]
            class AnyTarget {}

            class C {
                #[AnyTarget]
                function f(): void {}

                #[AnyTarget]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.target" || x.identifier == "attribute.notAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_promoted_constructor_property_accepts_either_the_parameter_or_property_target() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                function __construct(#[PropOnly] public int $x) {}
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.target" || x.identifier == "attribute.notAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_promoted_parameter_rejects_a_property_only_attribute() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                function f(#[PropOnly] int $x) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class PropOnly does not have the parameter target."),
        "got: {d:?}"
    );
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

// Real bug found and fixed this session: `check_no_discard_statement`
// required the receiver's own type to be literally `Type::Object`,
// silently skipping the check for a `Union`/`Intersection` receiver - a
// value narrowed via `instanceof` from an unrelated interface, then
// called through, same as the other bugs this session found via the
// exact same anti-pattern.
#[test]
fn a_bare_call_to_a_no_discard_method_through_an_intersection_receiver_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            class C {
                #[NoDiscard]
                function m(): int { return 1; }
            }
            function test(View $view) {
                if ($view instanceof C) {
                    $view->m();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.resultDiscarded"),
        "got: {d:?}"
    );
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
