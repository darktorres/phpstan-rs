//! Phase 41/82's transitive-purity payoff: `registry::purity::Registry::
//! resolve_transitive_purity` proves a declaration effect-free, and
//! `checker::noop::check_pure_call_without_effect` flags a bare,
//! discarded statement-level call to one. Uses `all_diagnostics_for`
//! throughout, not the ordinary `diagnostics_for` - see
//! `checker::tests::helpers::is_pure_call_result_unused_diagnostic`'s
//! own doc comment for why this family is filtered out of the shared
//! helper by default.

use super::helpers::*;

#[test]
fn a_bare_call_to_a_final_method_with_a_trivial_body_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function noop(): void {
                }
            }
            $f = new Foo();
            $f->noop();
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_call_to_an_overridable_method_is_not_flagged() {
    // Public, non-final, on a non-final class - a subclass could
    // override it with real side effects, so this project's own
    // classifier (matching real PHPStan's own `MethodCall` handling
    // exactly) must never treat it as provably effect-free.
    let d = all_diagnostics_for(
        r#"<?php
            class Foo {
                public function noop(): void {
                }
            }
            $f = new Foo();
            $f->noop();
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_bare_call_to_a_private_method_with_a_trivial_body_is_flagged() {
    // Private is non-overridable regardless of the class's own
    // final-ness - a distinct, separate non-overridability path from
    // the `final class`/`final method` one above. `run()` is itself
    // public and on a non-final class (deliberately, to isolate the
    // private-method half of `is_non_overridable` from the final-class
    // half `a_bare_call_to_a_final_method_with_a_trivial_body_is_flagged`
    // already covers) - it's transitively pure only because the *callee*,
    // `helper()`, is private.
    let d = all_diagnostics_for(
        r#"<?php
            class Foo {
                public function run(): void {
                    $this->helper();
                }
                private function helper(): void {
                }
            }
            $f = new Foo();
            $f->run();
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_method_calling_another_proven_pure_method_is_transitively_pure() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function outer(): void {
                    $this->inner();
                }
                public function inner(): void {
                }
            }
            $f = new Foo();
            $f->outer();
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_chain_of_three_levels_still_resolves() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function a(): void {
                    $this->b();
                }
                public function b(): void {
                    $this->c();
                }
                public function c(): void {
                }
            }
            $f = new Foo();
            $f->a();
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_method_with_an_unconditional_echo_is_never_pure_regardless_of_what_else_it_calls() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function outer(): void {
                    $this->inner();
                    echo "hi";
                }
                public function inner(): void {
                }
            }
            $f = new Foo();
            $f->outer();
        "#,
    );
    // `inner()` itself genuinely is pure (final class, empty body) - the
    // bare `$this->inner();` *inside* `outer()`'s own body is correctly
    // flagged. `outer()` itself must never be, since its own `echo`
    // makes it impure regardless of what else it calls - the real
    // assertion is that no diagnostic names `outer` as the pure one.
    assert!(
        !d.iter()
            .any(|x| x.identifier == "method.resultUnused" && x.message.contains("outer")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_method_that_writes_a_property_is_never_pure() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public int $x = 0;
                public function setX(int $v): void {
                    $this->x = $v;
                }
            }
            $f = new Foo();
            $f->setX(1);
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_method_that_explicitly_throws_is_never_pure() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function fail(): void {
                    throw new \Exception("no");
                }
            }
            $f = new Foo();
            $f->fail();
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_cyclic_pair_of_methods_calling_only_each_other_is_never_pure() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function a(): void {
                    $this->b();
                }
                public function b(): void {
                    $this->a();
                }
            }
            $f = new Foo();
            $f->a();
        "#,
    );
    // No external base case - the fixed point can never mark either
    // side pure, matching real PHPStan's own identical behavior for
    // this exact shape. The real point of this test is that classifying
    // a genuine cycle terminates at all rather than looping forever.
    assert!(
        !d.iter().any(|x| x.identifier == "method.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_bare_call_whose_result_is_used_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function pure(): int {
                    return 1;
                }
            }
            $f = new Foo();
            $x = $f->pure();
            echo $x;
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_bare_call_to_a_pure_plain_function_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function pureAdd(int $a, int $b): int {
                return $a + $b;
            }
            pureAdd(1, 2);
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.resultUnused"),
        "got: {d:?}"
    );
}

// A promoted constructor property (`private int $x` right in the
// parameter list) has no explicit `$this->x = $x;` *statement* in the
// body at all - real PHP synthesizes the assignment at the engine
// level, invisible to a real AST walk. An ordinary, explicit
// `$this->x = $x;` inside an otherwise-plain constructor is a genuine
// `propertyAssign` impure point instead (this project's classifier
// draws that line exactly like real PHPStan's own `ClassMethodHandler`/
// `PropertyHooksProcessor` do for property hooks/promotion specifically -
// not attempting the harder, more general "is this the object's own,
// not-yet-observable state" exception for an explicit in-body assign),
// so a promoted-only constructor is the one realistic shape a `new`
// expression can ever be proven pure through.
#[test]
fn a_bare_call_to_new_on_a_class_with_only_promoted_properties_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public function __construct(private int $x) {
                }
            }
            new Foo(1);
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "new.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_call_to_new_on_a_class_whose_constructor_explicitly_assigns_a_property_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            final class Foo {
                public int $x;
                public function __construct(int $x) {
                    $this->x = $x;
                }
            }
            new Foo(1);
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "new.resultUnused"),
        "unexpected diagnostics: {d:?}"
    );
}
