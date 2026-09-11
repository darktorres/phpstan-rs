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

// Phase 48.F: `FunctionDefinitionCheck`'s PHP-version-gated native
// declaration-syntax checks.

#[test]
fn a_native_union_parameter_type_pre_8_0_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int|string $x): void {}
        "#,
        |r| r.php_version = Some((7, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.unionTypeNotSupported"
                && x.message.contains(
                    "uses native union types but they're supported only on PHP 8.0 and later"
                )),
        "got: {d:?}"
    );
}

#[test]
fn a_native_union_return_type_pre_8_0_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): int|string { return 1; }
        "#,
        |r| r.php_version = Some((7, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "return.unionTypeNotSupported"),
        "got: {d:?}"
    );
}

#[test]
fn a_native_union_parameter_type_on_8_0_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int|string $x): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.unionTypeNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn with_no_configured_php_version_the_union_type_check_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function f(int|string $x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "parameter.unionTypeNotSupported" | "return.unionTypeNotSupported"
        )),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_plain_typed_parameter_defaulting_to_null_on_8_4_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(Foo $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.implicitlyNullable"
            && x.message
                == "Deprecated in PHP 8.4: Parameter #1 $x (Foo) is implicitly nullable via default value null."),
        "got: {d:?}"
    );
}

#[test]
fn a_nullable_typed_parameter_defaulting_to_null_is_never_flagged_implicitly_nullable() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(?Foo $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.implicitlyNullable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_union_with_null_defaulting_to_null_is_never_flagged_implicitly_nullable() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(Foo|null $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.implicitlyNullable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_plain_typed_parameter_defaulting_to_null_before_8_4_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(Foo $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 3)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.implicitlyNullable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_required_parameter_after_an_optional_one_on_8_0_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($a = 1, $b): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.requiredAfterOptional"
            && x.message
                == "Deprecated in PHP 8.0: Required parameter $b follows optional parameter $a."),
        "got: {d:?}"
    );
}

#[test]
fn a_variadic_parameter_after_an_optional_one_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($a = 1, ...$rest): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.requiredAfterOptional"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn ordinary_parameter_ordering_is_never_flagged_required_after_optional() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($a, $b = 1): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.requiredAfterOptional"),
        "unexpected diagnostics: {d:?}"
    );
}
