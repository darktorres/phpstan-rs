//! Phase 67: the `@internal` restricted-usage engine
//! (`checker::internal_usage`) - off by default (`Registry::
//! check_internal_usage`, mirroring real PHPStan's own opt-in
//! `featureToggles.internalTag`), so every test here needs it turned on
//! to see anything. Cross-namespace access is exercised with multiple
//! `namespace X;` statements in one source string - this project's own
//! `NameContext` switches on every top-level `Namespace` statement it
//! sees, the same shape a real `namespace A {} namespace B {}`
//! brace-delimited multi-namespace file produces.

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

// Phase 67 follow-up: a constant's own individual `@internal` tag - not
// just its declaring class's - is now tracked too.

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
    // Previously a real, undetected gap (see ROADMAP.md's own Phase 67
    // entry): a plain write reaches `check_property_assignment`
    // directly, not the `ExprKind::PropertyAccess` read arm that
    // already carried this check.
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
