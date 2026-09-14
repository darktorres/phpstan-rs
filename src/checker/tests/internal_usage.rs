//! The `@internal` restricted-usage engine (`checker::internal_usage`),
//! off by default (`Registry::check_internal_usage`, mirroring real
//! PHPStan's own opt-in `featureToggles.internalTag`), so every test
//! here needs it turned on to see anything. Cross-namespace access is
//! exercised with multiple `namespace X;` statements in one source
//! string - this project's own `NameContext` switches on every
//! top-level `Namespace` statement it sees, the same shape a real
//! `namespace A {} namespace B {}` brace-delimited multi-namespace file
//! produces.

use super::helpers::*;

fn diagnostics_for(src: &str) -> Vec<super::super::Diagnostic> {
    diagnostics_for_with(src, |r| r.check_internal_usage = true)
}

#[test]
fn the_check_is_off_by_default() {
    let d = diagnostics_for_with(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            function make(): void {}

            namespace Vendor\App;
            function f() {
                \Vendor\Internal\make();
            }
        "#,
        |_r| {},
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.internal"),
        "unexpected diagnostics with the flag left off: {d:?}"
    );
}

#[test]
fn an_internal_function_called_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            function make(): void {}

            namespace Other\App;
            function f() {
                \Vendor\Internal\make();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.internal"),
        "got: {d:?}"
    );
}

#[test]
fn an_internal_function_called_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            function make(): void {}

            namespace Vendor\App;
            function f() {
                \Vendor\Internal\make();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.internal"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_plain_non_internal_function_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            function make(): void {}

            namespace Other\App;
            function f() {
                \Vendor\Internal\make();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.internal"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn calling_a_method_of_an_internal_class_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function doStuff(): void {}
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->doStuff();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn calling_an_internal_method_of_an_otherwise_ordinary_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            class Thing {
                /** @internal */
                public function doStuff(): void {}
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->doStuff();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internal"),
        "got: {d:?}"
    );
}

#[test]
fn calling_a_method_of_an_internal_class_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function doStuff(): void {}
            }

            namespace Vendor\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->doStuff();
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("method.internal")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn calling_a_static_method_of_an_internal_class_is_flagged_as_static() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public static function make(): void {}
            }

            namespace Other\App;
            function f() {
                \Vendor\Internal\Thing::make();
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "staticMethod.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn reading_a_property_of_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public int $x = 1;
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                echo $t->x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn reading_a_static_property_of_an_internal_class_is_flagged_as_static() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public static int $x = 1;
            }

            namespace Other\App;
            function f() {
                echo \Vendor\Internal\Thing::$x;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "staticProperty.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn reading_a_constant_of_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                const X = 1;
            }

            namespace Other\App;
            function f() {
                echo \Vendor\Internal\Thing::X;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.internalClass"),
        "got: {d:?}"
    );
}

// A constant's own individual `@internal` tag - not just its declaring
// class's - is tracked too.

#[test]
fn reading_an_internal_constant_of_an_otherwise_public_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            class Thing {
                /** @internal */
                const X = 1;
            }

            namespace Other\App;
            function f() {
                echo \Vendor\Internal\Thing::X;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.internal" && x.message.contains("Thing::X")),
        "got: {d:?}"
    );
}

#[test]
fn reading_an_internal_constant_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            class Thing {
                /** @internal */
                const X = 1;
            }

            namespace Vendor\App;
            function f() {
                echo \Vendor\Internal\Thing::X;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("classConstant.internal")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn reading_a_non_internal_constant_of_an_otherwise_public_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            class Thing {
                const X = 1;
            }

            namespace Other\App;
            function f() {
                echo \Vendor\Internal\Thing::X;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("classConstant.internal")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn instantiating_an_internal_class_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Other\App;
            function f() {
                new \Vendor\Internal\Thing();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "new.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn instantiating_an_internal_class_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Vendor\App;
            function f() {
                new \Vendor\Internal\Thing();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "new.internalClass"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_internal_interface_gets_its_own_class_type_word() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            interface Thing {
                public function doStuff(): void;
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->doStuff();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalInterface"),
        "got: {d:?}"
    );
}

#[test]
fn a_global_namespace_internal_class_accessed_from_another_global_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @internal */
            class Thing {
                public function doStuff(): void {}
            }
            class Other {
                public function f(Thing $t) {
                    $t->doStuff();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_global_namespace_internal_class_accessed_from_within_itself_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @internal */
            class Thing {
                public function doStuff(): void {}
                public function other(): void {
                    $this->doStuff();
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("method.internal")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn writing_a_property_of_an_internal_class_is_flagged() {
    // A plain write reaches `check_property_assignment` directly, not
    // the `ExprKind::PropertyAccess` read arm - both need this check.
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public int $x = 1;
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->x = 2;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn implicit_string_cast_of_an_internal_class_calls_internal_tostring() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function __toString(): string { return "x"; }
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                echo (string) $t;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn echoing_an_internal_class_calls_internal_tostring() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function __toString(): string { return "x"; }
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                echo $t;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn printing_an_internal_class_calls_internal_tostring() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function __toString(): string { return "x"; }
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                print $t;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn interpolating_an_internal_class_calls_internal_tostring() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function __toString(): string { return "x"; }
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                return "value: {$t}";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn implicit_string_cast_of_a_non_internal_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Thing {
                public function __toString(): string { return "x"; }
            }
            function f(Thing $t) {
                echo (string) $t;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("method.internal")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn writing_a_property_of_an_internal_class_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public int $x = 1;
            }

            namespace Vendor\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->x = 2;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("property.internal")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_instanceof_check_against_an_internal_class_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Other\App;
            function f(object $o) {
                $o instanceof \Vendor\Internal\Thing;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "instanceof.internalClass"
            && x.message == "Instanceof references internal class Vendor\\Internal\\Thing."),
        "got: {d:?}"
    );
}

#[test]
fn an_instanceof_check_against_an_internal_class_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Vendor\App;
            function f(object $o) {
                $o instanceof \Vendor\Internal\Thing;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "instanceof.internalClass"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn catching_an_internal_exception_class_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class ThingException extends \Exception {
            }

            namespace Other\App;
            function f() {
                try {
                    doStuff();
                } catch (\Vendor\Internal\ThingException $e) {
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "catch.internalClass"
            && x.message == "Catching internal class Vendor\\Internal\\ThingException."),
        "got: {d:?}"
    );
}

#[test]
fn catching_an_internal_exception_class_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class ThingException extends \Exception {
            }

            namespace Vendor\App;
            function f() {
                try {
                    doStuff();
                } catch (\Vendor\Internal\ThingException $e) {
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "catch.internalClass"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn catching_a_non_throwable_internal_class_still_reports_both_diagnostics_independently() {
    // `catch.notThrowable` and `catch.internalClass` are two entirely
    // separate rules in real PHPStan (`CaughtExceptionExistenceRule` vs
    // `RestrictedInternalClassNameUsageExtension`) - a class that's both
    // non-Throwable and `@internal` should get both, not just one.
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class NotAnException {
            }

            namespace Other\App;
            function f() {
                try {
                    doStuff();
                } catch (\Vendor\Internal\NotAnException $e) {
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "catch.notThrowable"),
        "got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.identifier == "catch.internalClass"),
        "got: {d:?}"
    );
}

// `extends`/`implements`/trait-use for the `@internal`
// class-name-usage check.

#[test]
fn extending_an_internal_class_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Base {}

            namespace Other\App;
            class Child extends \Vendor\Internal\Base {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.extendsInternalClass"),
        "got: {d:?}"
    );
}

#[test]
fn extending_an_internal_class_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Base {}

            namespace Vendor\App;
            class Child extends \Vendor\Internal\Base {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "class.extendsInternalClass"),
        "got: {d:?}"
    );
}

#[test]
fn an_interface_extending_an_internal_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            interface IBase {}

            namespace Other\App;
            interface IChild extends \Vendor\Internal\IBase {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "interface.extendsInternalInterface"),
        "got: {d:?}"
    );
}

#[test]
fn implementing_an_internal_interface_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            interface IBase {}

            namespace Other\App;
            class Impl implements \Vendor\Internal\IBase {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.implementsInternalInterface"),
        "got: {d:?}"
    );
}

#[test]
fn an_enum_implementing_an_internal_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            interface IBase {}

            namespace Other\App;
            enum E implements \Vendor\Internal\IBase {
                case A;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "enum.implementsInternalInterface"),
        "got: {d:?}"
    );
}

#[test]
fn using_an_internal_trait_from_a_different_root_namespace_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            trait T {}

            namespace Other\App;
            class C {
                use \Vendor\Internal\T;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "traitUse.internalTrait"),
        "got: {d:?}"
    );
}

#[test]
fn using_an_internal_trait_from_the_same_root_namespace_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            trait T {}

            namespace Vendor\App;
            class C {
                use \Vendor\Internal\T;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "traitUse.internalTrait"),
        "got: {d:?}"
    );
}

#[test]
fn extending_a_non_internal_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            class Base {}

            namespace Other\App;
            class Child extends \Vendor\Internal\Base {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.contains("Internal")),
        "got: {d:?}"
    );
}

// `param`/`return` native type-hints for the `@internal`
// class-name-usage check.

#[test]
fn a_parameter_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            function f(\Vendor\Internal\Foo $x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_return_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            function f(): \Vendor\Internal\Foo {
                throw new \Exception();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_nullable_parameter_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            function f(?\Vendor\Internal\Foo $x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_union_parameter_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            function f(\Vendor\Internal\Foo|int $x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_parameter_typehint_referencing_an_internal_class_from_the_same_root_namespace_is_never_flagged()
 {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Vendor\App;
            function f(\Vendor\Internal\Foo $x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_self_typed_parameter_on_an_internal_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {
                public function withOther(self $other): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_promoted_constructor_property_parameter_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            class C {
                public function __construct(private \Vendor\Internal\Foo $foo) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_docblock_only_param_tag_referencing_an_internal_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            /** @param \Vendor\Internal\Foo $x */
            function f($x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

// First-class-callable syntax (`$foo->bar(...)`/`Foo::bar(...)`) runs
// the same `@internal` checks an ordinary call gets
// (`check_internal_method_call`, wired into
// ExprKind::FirstClassCallable's MethodCall/StaticCall arms).

#[test]
fn a_first_class_callable_of_an_internal_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function doStuff(): void {}
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $g = $t->doStuff(...);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_first_class_callable_of_an_internal_static_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public static function make(): void {}
            }

            namespace Other\App;
            function f() {
                $g = \Vendor\Internal\Thing::make(...);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "staticMethod.internalClass"),
        "got: {d:?}"
    );
}

// A closure/arrow-fn literal's own native param/return type-hints need
// this check too, not just the existence check -
// `function (InternalClass $f): InternalClass {}` must be flagged the
// same way the identical named-function signature is.

#[test]
fn a_closure_parameter_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            $f = function (\Vendor\Internal\Foo $x): void {};
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_closure_return_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            $f = function (): \Vendor\Internal\Foo {
                throw new \Exception();
            };
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn an_arrow_fn_parameter_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            $f = fn(\Vendor\Internal\Foo $x) => null;
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn an_arrow_fn_return_typehint_referencing_an_internal_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            $f = fn(\Vendor\Internal\Foo $x): \Vendor\Internal\Foo => $x;
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.internalClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_closure_parameter_typehint_referencing_an_internal_class_from_the_same_root_namespace_is_never_flagged()
 {
    let d = diagnostics_for(
        r#"<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Vendor\App;
            $f = function (\Vendor\Internal\Foo $x): void {};
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.internalClass"),
        "got: {d:?}"
    );
}
