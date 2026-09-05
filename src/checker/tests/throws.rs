//! Phase 42's own self-contained first slice: `@throws` tag validity
//! (`checker::throws`, `throws.notThrowable`) - a `@throws` naming
//! something that isn't actually a `Throwable` subtype.

use super::helpers::*;

#[test]
fn throws_tag_naming_a_real_exception_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \RuntimeException */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.notThrowable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn throws_tag_naming_a_non_throwable_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class NotAThrowable {}
            /** @throws NotAThrowable */
            function f(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.notThrowable"
            && x.message.contains(
                "PHPDoc tag @throws with type NotAThrowable is not subtype of Throwable"
            )),
        "got: {d:?}"
    );
}

#[test]
fn throws_tag_naming_an_unresolvable_class_is_never_flagged() {
    // Real corpus shape: a native-extension-only exception class (e.g.
    // `SVMException` from the `svm` extension) that this project's own
    // stub corpus never vendors - "don't know, don't guess" the same
    // way `CaughtExceptionExistenceRule`'s own ordering already does for
    // the analogous `catch` case, rather than misreporting an unknown
    // class as "confirmed not Throwable".
    let d = diagnostics_for(
        r#"<?php
            /** @throws SomeExtensionOnlyException */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.notThrowable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn throws_tag_union_with_one_non_throwable_member_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class NotAThrowable {}
            /** @throws NotAThrowable|\RuntimeException */
            function f(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.notThrowable"),
        "got: {d:?}"
    );
}

#[test]
fn throws_tag_union_of_real_exceptions_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \RuntimeException|\LogicException */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.notThrowable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn throws_tag_of_void_is_never_flagged() {
    // Real PHPStan's own deliberate "this documents that it never
    // throws" escape hatch.
    let d = diagnostics_for(
        r#"<?php
            /** @throws void */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.notThrowable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn no_throws_tag_at_all_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.notThrowable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn throws_tag_on_a_method_is_also_checked() {
    let d = diagnostics_for(
        r#"<?php
            class NotAThrowable {}
            class Foo {
                /** @throws NotAThrowable */
                public function bar(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.notThrowable"),
        "got: {d:?}"
    );
}

// ThrowsVoidFunctionWithExplicitThrowPointRule/
// ThrowsVoidMethodWithExplicitThrowPointRule (`throws.void`) - an
// explicit `@throws void` directly contradicted by a real `throw` in
// the body.

#[test]
fn explicit_throws_void_contradicted_by_a_real_throw_is_flagged_for_a_function() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws void */
            function f(): void {
                throw new \RuntimeException('surprise');
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.void"
            && x.message == "Function f() throws exception but the PHPDoc contains @throws void."),
        "got: {d:?}"
    );
}

#[test]
fn explicit_throws_void_contradicted_by_a_real_throw_is_flagged_for_a_method() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @throws void */
                public function bar(): void {
                    throw new \RuntimeException('surprise');
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.void"
            && x.message
                == "Method Foo::bar() throws exception but the PHPDoc contains @throws void."),
        "got: {d:?}"
    );
}

#[test]
fn throws_void_with_no_real_throw_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws void */
            function f(): void {
                echo "safe";
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.void"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_real_throw_with_no_throws_void_tag_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                throw new \RuntimeException('fine, no contradicting tag');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.void"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_throw_inside_a_nested_closure_does_not_contradict_the_outer_throws_void() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws void */
            function f(): void {
                $g = function () {
                    throw new \RuntimeException('belongs to the closure, not f()');
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.void"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_throw_inside_a_nested_if_still_contradicts_throws_void() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws void */
            function f(bool $cond): void {
                if ($cond) {
                    throw new \RuntimeException('nested but still f()\'s own body');
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.void"),
        "got: {d:?}"
    );
}
