//! Phase 65: declaration-site modifier-consistency + constructor-shape
//! checks - a batch of small, independent boolean-flag-combination
//! checks real PHP enforces unconditionally at compile time, all
//! confirmed directly against real `php -l`.

use super::helpers::*;

#[test]
fn an_abstract_method_in_a_non_abstract_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.abstract"),
        "got: {d:?}"
    );
}

#[test]
fn an_abstract_method_in_an_abstract_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Foo {
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.abstract"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_abstract_method_in_a_trait_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T {
                abstract public function foo(): void;
            }
            class UsesT {
                use T;
                public function foo(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.abstract"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_abstract_enum_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.abstract" && x.message.contains("Enum")),
        "got: {d:?}"
    );
}

#[test]
fn a_private_abstract_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Foo {
                abstract private function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.abstractPrivate"),
        "got: {d:?}"
    );
}

#[test]
fn a_private_abstract_method_in_an_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Foo {
                private function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.abstractPrivate"),
        "got: {d:?}"
    );
}

#[test]
fn a_public_abstract_method_is_never_flagged_as_abstract_private() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Foo {
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.abstractPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_final_private_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final private function bar(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.finalPrivate"),
        "got: {d:?}"
    );
}

#[test]
fn a_final_private_constructor_is_never_flagged() {
    // A real, common singleton-pattern idiom, matching real PHPStan's
    // own explicit exemption.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final private function __construct() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.finalPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_final_public_method_is_never_flagged_as_final_private() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final public function bar(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.finalPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_public_interface_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Foo {
                protected function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.visibility"),
        "got: {d:?}"
    );
}

#[test]
fn a_public_interface_method_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Foo {
                public function bar(): void;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.visibility"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_constructor_with_a_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __construct(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "constructor.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_constructor_with_no_return_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __construct() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "constructor.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn reassigning_this_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(): void {
                    $this = new Foo();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assign.this"),
        "got: {d:?}"
    );
}

#[test]
fn assigning_an_ordinary_variable_is_never_flagged_as_this_reassignment() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $x = 5;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "assign.this"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn global_this_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                global $this;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "global.this"),
        "got: {d:?}"
    );
}

#[test]
fn global_of_an_ordinary_variable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                global $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "global.this"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn static_this_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                static $this;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "static.this"),
        "got: {d:?}"
    );
}

#[test]
fn static_of_an_ordinary_variable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                static $x = 0;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "static.this"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_class_constant_named_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const class = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.class"),
        "got: {d:?}"
    );
}

#[test]
fn an_ordinarily_named_class_constant_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const BAR = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.class"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_enum_constant_named_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                const class = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.class"),
        "got: {d:?}"
    );
}

// `ClassConstantRule`'s own typed-constant half (PHP 8.3's `const TYPE
// NAME = value;`) - confirmed directly via `php -l` as a real,
// unconditional fatal error ("Cannot use string as value for class
// constant Foo::BAD of type int").

#[test]
fn a_typed_constant_whose_value_mismatches_its_declared_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const int BAD = "not an int";
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.value" && x.message.contains("Foo::BAD")),
        "got: {d:?}"
    );
}

#[test]
fn a_typed_constant_whose_value_matches_its_declared_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const int GOOD = 1;
                const string ALSO_GOOD = "hi";
                const array LIST_OK = [1, 2, 3];
                const ?int NULLABLE_OK = null;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.value"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_untyped_constant_is_never_flagged_by_this_check() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const BAD = "any value is fine here";
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.value"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_typed_enum_constant_whose_value_mismatches_its_declared_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                const int BAD = "not an int";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.value"),
        "got: {d:?}"
    );
}

#[test]
fn a_typed_constant_assigned_an_unresolved_global_constant_is_never_flagged() {
    // Real corpus find: `DEBUG_BACKTRACE_IGNORE_ARGS` (a real PHP
    // builtin int constant this project has no global-constant table
    // for at all) reads as `Mixed`, not a genuinely `mixed`-typed value -
    // don't guess wrong on it, the same leniency established elsewhere.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private const int OPTIONS = DEBUG_BACKTRACE_IGNORE_ARGS;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.value"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_typed_constant_referencing_another_constant_is_checked_via_full_inference() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const int BASE = 1;
                const int DERIVED = self::BASE;
                const string WRONG = self::BASE;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.value" && x.message.contains("DERIVED")),
        "unexpected diagnostics: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.value" && x.message.contains("WRONG")),
        "got: {d:?}"
    );
}
