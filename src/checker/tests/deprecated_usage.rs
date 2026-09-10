//! Phase 39: `@deprecated` usage warnings (`checker::deprecated_usage`) -
//! off by default (`Registry::check_deprecated_usage`), so every test here
//! needs it turned on to see anything.

use super::helpers::*;

fn diagnostics_for(src: &str) -> Vec<super::super::Diagnostic> {
    diagnostics_for_with(src, |r| r.check_deprecated_usage = true)
}

#[test]
fn the_check_is_off_by_default() {
    let d = diagnostics_for_with(
        r#"<?php
            /** @deprecated */
            function oldFunc(): void {}
            oldFunc();
        "#,
        |_r| {},
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.deprecated"),
        "unexpected diagnostics with the flag left off: {d:?}"
    );
}

#[test]
fn calling_a_deprecated_function_with_no_reason_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            function oldFunc(): void {}
            oldFunc();
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.deprecated"
            && x.message == "Call to deprecated function oldFunc()."),
        "got: {d:?}"
    );
}

#[test]
fn calling_a_deprecated_function_with_a_reason_includes_it_in_the_message() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated use newFunc() instead */
            function oldFunc(): void {}
            oldFunc();
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.deprecated"
            && x.message == "Call to deprecated function oldFunc() (use newFunc() instead)."),
        "got: {d:?}"
    );
}

#[test]
fn calling_a_plain_non_deprecated_function_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function niceFunc(): void {}
            niceFunc();
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn calling_a_deprecated_instance_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated */
                public function oldMethod(): void {}
            }
            function f(Foo $foo): void {
                $foo->oldMethod();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.deprecated"
            && x.message == "Call to deprecated method Foo::oldMethod()."),
        "got: {d:?}"
    );
}

#[test]
fn calling_a_deprecated_static_method_is_flagged_as_static() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated */
                public static function oldMethod(): void {}
            }
            function f(): void {
                Foo::oldMethod();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "staticMethod.deprecated"
            && x.message == "Call to deprecated method Foo::oldMethod()."),
        "got: {d:?}"
    );
}

#[test]
fn calling_a_plain_non_deprecated_method_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function niceMethod(): void {}
            }
            function f(Foo $foo): void {
                $foo->niceMethod();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn calling_a_deprecated_function_from_inside_its_own_declaring_scope_is_still_flagged() {
    // Unlike @internal's own namespace-based allowance, @deprecated has
    // no "safe from here" carve-out at all - real usage from anywhere,
    // including the same file, is still worth flagging.
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            function oldFunc(): void {}
            function caller(): void {
                oldFunc();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.deprecated"),
        "got: {d:?}"
    );
}

#[test]
fn accessing_a_deprecated_instance_property_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated */
                public int $oldProp;
            }
            function f(Foo $foo): int {
                return $foo->oldProp;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.deprecated"
            && x.message == "Call to deprecated property Foo::$oldProp."),
        "got: {d:?}"
    );
}

#[test]
fn accessing_a_deprecated_static_property_is_flagged_as_static() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated */
                public static int $oldProp = 1;
            }
            function f(): int {
                return Foo::$oldProp;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "staticProperty.deprecated"
            && x.message == "Call to deprecated property Foo::$oldProp."),
        "got: {d:?}"
    );
}

#[test]
fn accessing_a_plain_non_deprecated_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $niceProp;
            }
            function f(Foo $foo): int {
                return $foo->niceProp;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.deprecated"
            || x.identifier == "staticProperty.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn accessing_a_deprecated_class_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated use NEW_CONST instead */
                public const OLD_CONST = 1;
            }
            function f(): int {
                return Foo::OLD_CONST;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.deprecated"
            && x.message == "Call to deprecated constant Foo::OLD_CONST (use NEW_CONST instead)."),
        "got: {d:?}"
    );
}

#[test]
fn accessing_a_plain_non_deprecated_class_constant_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public const NICE_CONST = 1;
            }
            function f(): int {
                return Foo::NICE_CONST;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn implicit_string_cast_of_a_deprecated_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class OldThing {
                /** @deprecated do not stringify */
                public function __toString(): string { return "y"; }
            }
            function f(OldThing $o): void {
                echo (string) $o;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.toStringDeprecated"
            && x.message == "Casting class OldThing to string is deprecated:\ndo not stringify"),
        "got: {d:?}"
    );
}

#[test]
fn echoing_a_deprecated_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class OldThing {
                /** @deprecated do not stringify */
                public function __toString(): string { return "y"; }
            }
            function f(OldThing $o): void {
                echo $o;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.toStringDeprecated"
            && x.message == "Casting class OldThing to string is deprecated:\ndo not stringify"),
        "got: {d:?}"
    );
}

#[test]
fn printing_a_deprecated_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class OldThing {
                /** @deprecated do not stringify */
                public function __toString(): string { return "y"; }
            }
            function f(OldThing $o): void {
                print $o;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.toStringDeprecated"
            && x.message == "Casting class OldThing to string is deprecated:\ndo not stringify"),
        "got: {d:?}"
    );
}

#[test]
fn interpolating_a_deprecated_tostring_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class OldThing {
                /** @deprecated do not stringify */
                public function __toString(): string { return "y"; }
            }
            function f(OldThing $o): string {
                return "value: {$o}";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.toStringDeprecated"
            && x.message == "Casting class OldThing to string is deprecated:\ndo not stringify"),
        "got: {d:?}"
    );
}

#[test]
fn implicit_string_cast_of_a_non_deprecated_tostring_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class NiceThing {
                public function __toString(): string { return "y"; }
            }
            function f(NiceThing $n): void {
                echo (string) $n;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn instantiating_a_deprecated_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            class OldFoo {}
            function f(): OldFoo {
                return new OldFoo();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "new.deprecated"
            && x.message == "Instantiation of deprecated class OldFoo."),
        "got: {d:?}"
    );
}

#[test]
fn writing_a_deprecated_instance_property_is_flagged() {
    // Same write-side gap as `internal_usage`'s own identical test: a
    // plain write reaches `check_property_assignment` directly, not the
    // `ExprKind::PropertyAccess` read arm.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated */
                public int $oldProp;
            }
            function f(Foo $foo): void {
                $foo->oldProp = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.deprecated"
            && x.message == "Call to deprecated property Foo::$oldProp."),
        "got: {d:?}"
    );
}

#[test]
fn writing_a_plain_non_deprecated_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $niceProp;
            }
            function f(Foo $foo): void {
                $foo->niceProp = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn instantiating_a_plain_non_deprecated_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class NiceFoo {}
            function f(): NiceFoo {
                return new NiceFoo();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "new.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_class_level_deprecated_tag_cascades_to_a_method_call() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            class OldFoo {
                public function bar(): void {}
            }
            function f(OldFoo $foo): void {
                $foo->bar();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.deprecatedClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_class_level_deprecated_tag_cascades_to_a_property_access() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            class OldFoo {
                public int $x = 1;
            }
            function f(OldFoo $foo): void {
                echo $foo->x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.deprecatedClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_class_level_deprecated_tag_cascades_to_a_class_constant_access() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            class OldFoo {
                const X = 1;
            }
            function f(): int {
                return OldFoo::X;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.deprecatedClass"),
        "got: {d:?}"
    );
}

#[test]
fn a_deprecated_methods_own_tag_still_fires_when_the_class_is_not_deprecated() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated */
                public function bar(): void {}
            }
            function f(Foo $foo): void {
                $foo->bar();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.deprecated"),
        "got: {d:?}"
    );
}

#[test]
fn the_cascade_wins_over_the_members_own_tag_with_no_double_report() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            class OldFoo {
                /** @deprecated */
                public function bar(): void {}
            }
            function f(OldFoo $foo): void {
                $foo->bar();
            }
        "#,
    );
    let count = d
        .iter()
        .filter(|x| x.identifier.starts_with("method.deprecated"))
        .count();
    assert_eq!(count, 1, "got: {d:?}");
}

#[test]
fn a_call_from_inside_a_deprecated_method_is_not_reported() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            function oldFunc(): void {}
            class Foo {
                /** @deprecated */
                public function bar(): void {
                    oldFunc();
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_call_from_inside_a_deprecated_classs_method_is_not_reported() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            function oldFunc(): void {}
            /** @deprecated */
            class Foo {
                public function bar(): void {
                    oldFunc();
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "function.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_call_from_a_non_deprecated_scope_is_still_reported() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            function oldFunc(): void {}
            class Foo {
                public function bar(): void {
                    oldFunc();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.deprecated"),
        "got: {d:?}"
    );
}

#[test]
fn casting_a_deprecated_classs_instance_to_string_uses_the_special_message() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @deprecated */
                public function __toString(): string { return "x"; }
            }
            function f(Foo $foo): string {
                return (string) $foo;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.toStringDeprecated"
            && x.message
                .contains("Casting class Foo to string is deprecated")),
        "got: {d:?}"
    );
}
