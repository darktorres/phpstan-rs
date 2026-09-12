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

// Phase 42 Sub-items A/C: `TooWideFunctionThrowTypeRule`/
// `TooWideMethodThrowTypeRule` - a declared `@throws` member never
// actually produced anywhere in the body is flagged `throws.unusedType`.

#[test]
fn a_declared_throws_type_actually_thrown_is_not_flagged_too_wide() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \RuntimeException */
            function f(): void {
                throw new \RuntimeException('real');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

// Real, previously-live bug found while tracing a real Piwigo corpus
// false positive (`ConfigLoader::validateRequired`, whose own
// `new ReflectionClass(...)` call contributes ReflectionException - a
// declared throw type on that stub's own constructor - alongside the
// method's own genuine, matching `throw`): `check_too_wide_throw_type`
// used to call `is_assignable(actual, &declared)` directly, treating the
// *whole* union of everything this body could ever throw as the "sub"
// side - is_assignable's own (otherwise entirely correct) rule for a
// `Union` in that position requires *every* member to satisfy the
// supertype, so one single unrelated throw point anywhere in the body
// was enough to make an otherwise genuinely-thrown declared type look
// unused. The real question is "was `declared` thrown *somewhere*," not
// "is everything this body throws covered by `declared`."
#[test]
fn a_declared_throws_type_actually_thrown_alongside_an_unrelated_one_is_not_flagged_too_wide() {
    let d = diagnostics_for(
        r#"<?php
            class NeedleException extends \RuntimeException {}
            class UnrelatedException extends \RuntimeException {}
            /** @throws UnrelatedException */
            function callee(): void {
                throw new UnrelatedException('unrelated');
            }
            /** @throws NeedleException */
            function f(bool $b): void {
                if ($b) {
                    callee();
                }
                throw new NeedleException('real');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_declared_throws_type_never_thrown_anywhere_is_flagged_too_wide() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \RuntimeException */
            function f(): void {
                echo 'never throws anything';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"
            && x.message.contains(
                "Function f() has RuntimeException in PHPDoc @throws tag but it's not thrown."
            )),
        "got: {d:?}"
    );
}

#[test]
fn an_abstract_methods_own_throws_tag_is_never_flagged_too_wide() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Base {
                /** @throws \RuntimeException */
                abstract public function f(): void;
            }
            interface HasThrows {
                /** @throws \RuntimeException */
                public function g(): void;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "an abstract/interface method has no body to check for a real throw \
         point at all - its own @throws tag documents what a real \
         implementor is expected to throw, not what this (nonexistent) \
         body throws: {d:?}"
    );
}

#[test]
fn a_declared_throws_type_thrown_by_a_called_functions_own_throws_tag_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \RuntimeException */
            function inner(): void {
                throw new \RuntimeException('real');
            }
            /** @throws \RuntimeException */
            function outer(): void {
                inner();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_declared_throws_type_on_a_method_never_thrown_is_flagged_with_method_wording() {
    // `final` (rather than an ordinary overridable method) so this test
    // stays unaffected by the visibility/finality gate added later in this
    // file (`TooWideMethodThrowTypeRule`'s own gate) - see that gate's
    // dedicated test group below for the gate's own behavior.
    let d = diagnostics_for(
        r#"<?php
            final class Repo {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never throws';
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"
            && x.message
                .contains("Method Repo::find() has RuntimeException in PHPDoc @throws tag but it's not thrown.")),
        "got: {d:?}"
    );
}

#[test]
fn a_call_to_a_function_with_no_throws_tag_at_all_is_not_treated_as_unannotated() {
    // Sub-item A's own deliberate calibration: an ordinary, resolved
    // function with simply no `@throws` tag (the overwhelmingly common
    // case, e.g. real stdlib calls like `strlen()`) must NOT suppress
    // this whole check - only a genuinely unresolvable/dynamic call
    // should.
    let d = diagnostics_for(
        r#"<?php
            function noThrowsTagAtAll(): void {}
            /** @throws \RuntimeException */
            function f(): void {
                noThrowsTagAtAll();
                echo 'still never throws RuntimeException';
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_call_to_a_genuinely_unresolvable_function_suppresses_the_too_wide_check() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \RuntimeException */
            function f(callable $cb): void {
                $cb();
                echo 'never explicitly throws, but the dynamic call might';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_void_throws_tag_is_never_flagged_too_wide() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws void */
            function f(): void {
                echo 'never throws, exactly as declared';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_throw_inside_a_nested_closure_does_not_satisfy_the_outer_functions_own_throws_tag() {
    // Mirrors the `throws.void` test of the same shape above: a closure's
    // own throw points belong to *it*, not the enclosing declaration.
    let d = diagnostics_for(
        r#"<?php
            /** @throws \RuntimeException */
            function f(): void {
                $g = function () {
                    throw new \RuntimeException('belongs to the closure, not f()');
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "got: {d:?}"
    );
}

// Phase 42 Sub-item D: per-callee dynamic throw-type extensions.

#[test]
fn intdiv_by_a_literal_nonzero_divisor_satisfies_a_division_by_zero_error_throws_tag_check() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \DivisionByZeroError */
            function f(): void {
                intdiv(10, 2);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "a literal nonzero divisor can never divide by zero, so the tag should still be unused: {d:?}"
    );
}

#[test]
fn intdiv_by_a_non_literal_divisor_satisfies_a_division_by_zero_error_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \DivisionByZeroError */
            function f(int $divisor): void {
                intdiv(10, $divisor);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

// Same bug, same fix as intdiv's own pair of tests just above: a *valid*
// literal DateInterval spec used to fall back to the constructor's own
// generic stub-level @throws tag instead of correctly contributing
// nothing, so a declared @throws Exception looked "used" even though this
// exact call could never throw it.
#[test]
fn dateinterval_with_a_valid_literal_spec_satisfies_an_exception_throws_tag_check() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \Exception */
            function f(): void {
                new DateInterval('P1D');
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "a valid literal spec can never throw, so the tag should still be unused: {d:?}"
    );
}

#[test]
fn dateinterval_with_an_invalid_literal_spec_satisfies_an_exception_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \Exception */
            function f(): void {
                new DateInterval('garbage');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn assert_with_no_custom_exception_satisfies_an_assertion_error_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \AssertionError */
            function f(bool $cond): void {
                assert($cond);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn assert_with_a_custom_throwable_second_argument_satisfies_that_types_own_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \LogicException */
            function f(bool $cond): void {
                assert($cond, new \LogicException('custom'));
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn json_encode_with_throw_on_error_flag_satisfies_a_json_exception_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \JsonException */
            function f($x): void {
                json_encode($x, JSON_THROW_ON_ERROR);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn json_encode_without_throw_on_error_flag_does_not_satisfy_a_json_exception_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \JsonException */
            function f($x): void {
                json_encode($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn constructing_reflection_class_satisfies_a_reflection_exception_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \ReflectionException */
            function f(string $class): void {
                new \ReflectionClass($class);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn calling_sub_on_a_datetime_satisfies_a_date_invalid_operation_exception_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \DateInvalidOperationException */
            function f(\DateTime $dt, \DateInterval $i): void {
                $dt->sub($i);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn create_element_with_a_non_literal_name_satisfies_a_dom_exception_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \DOMException */
            function f(\DOMDocument $doc, string $name): void {
                $doc->createElement($name);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn constructing_simplexml_element_from_a_non_literal_argument_satisfies_an_exception_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /** @throws \Exception */
            function f(string $xml): void {
                new \SimpleXMLElement($xml);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 42 bonus item: opt-in "missing checked exception" family - off
// by default, enabled per-test via `diagnostics_for_with`.

#[test]
fn the_missing_checked_exception_check_is_off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                throw new \RuntimeException('undocumented');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.missing"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_explicit_throw_not_covered_by_any_throws_tag_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): void {
                throw new \RuntimeException('undocumented');
            }
        "#,
        |r| r.check_missing_checked_exceptions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.missing"
            && x.message.contains(
                "Function f() throws RuntimeException but it's not covered by any @throws tag."
            )),
        "got: {d:?}"
    );
}

#[test]
fn an_explicit_throw_covered_by_its_own_throws_tag_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            /** @throws \RuntimeException */
            function f(): void {
                throw new \RuntimeException('documented');
            }
        "#,
        |r| r.check_missing_checked_exceptions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.missing"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_explicit_throw_covered_by_a_broader_throws_tag_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            /** @throws \Exception */
            function f(): void {
                throw new \RuntimeException('a real RuntimeException is-a Exception');
            }
        "#,
        |r| r.check_missing_checked_exceptions = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.missing"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_throw_type_not_covered_by_a_narrower_throws_tag_is_still_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            /** @throws \LogicException */
            function f(): void {
                throw new \RuntimeException('not a LogicException');
            }
        "#,
        |r| r.check_missing_checked_exceptions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.missing"),
        "got: {d:?}"
    );
}

#[test]
fn a_throw_covered_by_a_called_functions_own_throws_tag_is_still_flagged() {
    // Sub-item A's own deliberate calibration: a *called* function's own
    // `@throws` tag counts for the caller's own `TooWideFunctionThrowTypeRule`
    // comparison, but never for this check - what a callee documents is
    // that callee's own author's responsibility, not proof this
    // declaration's own explicit `throw` is covered.
    let d = diagnostics_for_with(
        r#"<?php
            /** @throws \RuntimeException */
            function inner(): void {
                throw new \RuntimeException('inner is fully documented');
            }
            function outer(): void {
                throw new \RuntimeException('outer has no tag of its own');
            }
        "#,
        |r| r.check_missing_checked_exceptions = true,
    );
    assert_eq!(
        d.iter()
            .filter(|x| x.identifier == "throws.missing")
            .count(),
        1,
        "got: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "throws.missing" && x.location == "outer"),
        "expected exactly outer() to be flagged, got: {d:?}"
    );
}

#[test]
fn a_throw_inside_a_method_uses_method_wording_when_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Repo {
                public function find(): void {
                    throw new \RuntimeException('undocumented');
                }
            }
        "#,
        |r| r.check_missing_checked_exceptions = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.missing"
            && x.message
                .contains("Method Repo::find() throws RuntimeException but it's not covered by any @throws tag.")),
        "got: {d:?}"
    );
}

// Phase 42 gating fix: `TooWideMethodThrowTypeRule`'s own visibility/
// finality/first-declaration gate - an ordinary overridable method with an
// unused `@throws` tag is a real, corpus-confirmed false-positive flood
// (real PHPStan itself defaults `checkTooWideThrowTypesInProtectedAndPublicMethods`
// to `false` for exactly this reason) and must stay silent unless the
// method is private, final, or on a final class.

#[test]
fn an_ordinary_overridable_public_method_with_an_unused_throws_tag_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            class Repo {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never actually throws';
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_private_method_with_an_unused_throws_tag_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Repo {
                /** @throws \RuntimeException */
                private function find(): void {
                    echo 'never actually throws';
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_final_method_with_an_unused_throws_tag_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Repo {
                /** @throws \RuntimeException */
                final public function find(): void {
                    echo 'never actually throws';
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_method_on_a_final_class_with_an_unused_throws_tag_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            final class Repo {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never actually throws';
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throws.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn an_ordinary_overridable_method_stays_silent_even_with_the_opt_in_flag_on_its_first_declaration()
{
    let d = diagnostics_for_with(
        r#"<?php
            class Repo {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never actually throws, and nothing overrides this yet';
                }
            }
        "#,
        |r| r.check_too_wide_throw_type_in_protected_and_public_methods = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throws.unusedType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_redeclaration_narrowing_an_interfaces_wider_throws_tag_is_flagged_with_the_opt_in_flag_on() {
    let d = diagnostics_for_with(
        r#"<?php
            interface Finder {
                /** @throws \RuntimeException */
                public function find(): void;
            }
            class Repo implements Finder {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never actually throws, and this overrides Finder::find()';
                }
            }
        "#,
        |r| r.check_too_wide_throw_type_in_protected_and_public_methods = true,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "throws.unusedType" && x.location == "Repo::find"),
        "got: {d:?}"
    );
}
