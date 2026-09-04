//! Phase 63: a single class/interface/trait/enum body literally
//! redeclaring the same constant/enum-case/property/method name twice -
//! `DuplicateDeclarationHelper`'s own core case.

use super::helpers::*;

#[test]
fn a_duplicate_property_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public $a;
                public $a;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.duplicateProperty" && x.message.contains("X::$a")),
        "got: {d:?}"
    );
}

#[test]
fn a_single_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public $a;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateProperty"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_duplicate_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public const C = 1;
                public const C = 2;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.duplicateConstant" && x.message.contains("X::C")),
        "got: {d:?}"
    );
}

#[test]
fn a_duplicate_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public function foo(): void {}
                public function foo(): void {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.duplicateMethod" && x.message.contains("X::foo()")),
        "got: {d:?}"
    );
}

#[test]
fn a_duplicate_method_is_case_insensitive() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public function foo(): void {}
                public function FOO(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.duplicateMethod"),
        "got: {d:?}"
    );
}

#[test]
fn two_distinct_methods_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public function foo(): void {}
                public function bar(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateMethod"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_duplicate_enum_case_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                case Active;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "enum.duplicateEnumCase"
                && x.message.contains("Status::Active")),
        "got: {d:?}"
    );
}

#[test]
fn two_distinct_enum_cases_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                case Inactive;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "enum.duplicateEnumCase"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_enum_case_and_a_constant_of_the_same_name_collide() {
    // Real PHP semantics: an enum case and a class constant share one
    // namespace - a case is itself a constant under the hood.
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                const Active = 1;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "enum.duplicateConstant" && x.message.contains("Active")),
        "got: {d:?}"
    );
}

#[test]
fn a_duplicate_method_on_a_trait_carries_the_trait_identifier() {
    let d = diagnostics_for(
        r#"<?php
            trait T {
                public function bar(): void {}
                public function bar(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "trait.duplicateMethod"),
        "got: {d:?}"
    );
}

#[test]
fn a_duplicate_method_on_an_interface_carries_the_interface_identifier() {
    let d = diagnostics_for(
        r#"<?php
            interface I {
                public function baz(): void;
                public function baz(): void;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "interface.duplicateMethod"),
        "got: {d:?}"
    );
}

#[test]
fn a_promoted_constructor_property_colliding_with_an_ordinary_one_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Z {
                public int $val;
                public function __construct(public int $val) {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.duplicateProperty" && x.message.contains("Z::$val")),
        "got: {d:?}"
    );
}

#[test]
fn a_promoted_constructor_property_alone_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Z {
                public function __construct(public int $val) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateProperty"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_promoted_constructor_parameter_is_never_treated_as_a_property() {
    let d = diagnostics_for(
        r#"<?php
            class Z {
                public int $val;
                public function __construct(int $val) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateProperty"),
        "unexpected diagnostics: {d:?}"
    );
}

// Real corpus find: phpstorm-stubs represents "this member's own shape
// changed across PHP versions" by declaring the same conceptual member
// twice, once per era - a real bug (an actual copy-paste duplicate)
// never has two *different* signatures for the "same" declaration,
// since it's definitionally the same one twice.
#[test]
fn two_methods_of_the_same_name_but_different_param_types_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public function setAccessible($accessible): void {}
                public function setAccessible(bool $accessible): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateMethod"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn two_methods_of_the_same_name_and_arity_but_different_return_types_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public function fetch() {}
                public function fetch(): array {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateMethod"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn two_methods_of_the_same_name_but_different_arity_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public function foo($a) {}
                public function foo($a, $b) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateMethod"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn two_properties_of_the_same_name_but_different_types_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Directory {
                public $path;
                public readonly string $path;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateProperty"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn two_properties_of_the_same_name_and_type_but_different_readonly_ness_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class X {
                public int $val;
                public readonly int $val;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.duplicateProperty"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn two_properties_of_the_exact_same_shape_are_still_flagged() {
    // The exemption is specific to a genuine shape *difference* - an
    // exact duplicate (the real bug this check exists to catch) must
    // still be reported.
    let d = diagnostics_for(
        r#"<?php
            class X {
                public int $val;
                public int $val;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.duplicateProperty"),
        "got: {d:?}"
    );
}
