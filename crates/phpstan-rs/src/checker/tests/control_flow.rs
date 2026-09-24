//! If/loop/switch/try merging, return-path exhaustiveness, break/continue, foreach/catch binding.

use super::helpers::*;

#[test]
fn nested_function_declared_inside_an_if_still_gets_its_body_checked() {
    // The extremely common `if (!function_exists('foo')) { function
    // foo() {} }` polyfill idiom (every symfony/polyfill-* package)
    // nests a real FunctionDecl inside a block. check_program's own
    // top-level loop only handles FunctionDecl/ClassDecl/EnumDecl at
    // strictly top-level - Registry::index already recurses into
    // if-bodies to find and index these (so calls to them resolve),
    // but check_stmt itself had no arm for the three declaration
    // kinds at all, so a nested one's own body was invisible to every
    // diagnostic, undefined-variable detection included.
    let d = diagnostics_for(
        r#"<?php
            if (!function_exists('foo')) {
                function foo() {
                    return $undefinedVar;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined variable")),
        "expected an undefined-variable diagnostic, got: {d:?}"
    );
}

// Fixed-point loop-body convergence (`Checker::converge_loop_body`) - a
// loop body must not be checked only once against the pre-loop entry
// state, or a variable reassigned inside the body on one iteration is
// invisible to a narrowing check reading it on a later one.
#[test]
fn a_variable_reassigned_in_a_loop_body_is_visible_to_a_narrowing_check_on_a_later_iteration() {
    // Concrete shape of the bug: `$x` starts `null`; a guard reads it
    // before this iteration's own reassignment reaches the bottom of the
    // loop. Under the old single-pass model, `$x !== null` narrowed a
    // `null`-only entry type to `Type::Never` inside the branch, and a
    // method call on `Never` was silently never checked - a real false
    // negative, not merely imprecision. Once convergence feeds a prior
    // iteration's `$x = new Foo();` back in as part of the entry state,
    // `$x` is correctly seen as `Foo` inside the guard, and calling an
    // undefined method on it is caught.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            function f() {
                $x = null;
                while (rand(0, 1)) {
                    if ($x !== null) {
                        $x->totallyMadeUp();
                    }
                    $x = new Foo();
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.notFound" && x.message.contains("totallyMadeUp")),
        "expected the loop-carried reassignment to be visible on a later \
         iteration's narrowing check, got: {d:?}"
    );
}

#[test]
fn a_loop_counter_incremented_each_iteration_converges_to_a_plain_int() {
    // `converge_loop_body` is hard-capped at 3 passes regardless of
    // whether types stabilize, so this can't hang - what this test
    // actually confirms is that the existing `union_types` generalize-
    // before-collapse machinery (see `accessory_and_refinement_types.rs`)
    // is already enough to widen a repeatedly-incremented `IntLiteral`
    // to plain `Int` within that budget, without needing a bespoke
    // scalar-literal-generalization pass on top.
    let d = diagnostics_for(
        r#"<?php
            function needsArray(array $x): void {}
            function f() {
                $i = 0;
                while (rand(0, 1)) {
                    $i = $i + 1;
                }
                needsArray($i);
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.type" && x.message.contains("int given")),
        "expected $i to converge to plain int, got: {d:?}"
    );
}
