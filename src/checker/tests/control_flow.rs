//! If/loop/switch/try merging, return-path exhaustiveness, break/continue, foreach/catch binding.

use super::helpers::*;

#[test]
fn scope_fork_does_not_leak_then_branch_assignment_into_else() {
    // Before real forking, then/else shared one mutable scope
    // sequentially, so an assignment in the then-branch could leak into
    // what the else-branch (wrongly) saw.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function f(bool $cond) {
                if ($cond) {
                    $x = "a string";
                } else {
                    $x = 5;
                    needsInt($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn merged_type_after_non_terminating_if_else_is_a_union() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function f(bool $cond) {
                if ($cond) {
                    $x = "a string";
                } else {
                    $x = 5;
                }
                needsInt($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn foreach_binds_the_value_variable_to_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $words */
            function f(array $words) {
                foreach ($words as $w) {
                    needsInt($w);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn foreach_over_correctly_typed_array_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param int[] $nums */
            function f(array $nums) {
                foreach ($nums as $n) {
                    needsInt($n);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn catch_binds_the_exception_variable_to_its_class() {
    let d = diagnostics_for(
        r#"<?php
            class MyError extends \Exception {
                public function detail(): string { return "detail"; }
            }
            function f() {
                try {
                    throw new MyError();
                } catch (MyError $e) {
                    return $e->detail();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn catch_binds_the_exception_variable_to_its_class_in_a_namespaced_file() {
    // The bare-name twin above passes trivially even with zero
    // namespace resolution, since an empty namespace makes FQN ==
    // bare name - it can't tell "resolved correctly" apart from
    // "resolution never ran at all". This is the version that
    // actually distinguishes them: asserting a diagnostic *does*
    // appear on an undefined method call is what proves $e's type
    // resolved to the real "App\MyError", not an unresolved bare
    // "MyError" (which the undefined-method check's own leniency
    // guard would have silently suppressed, since a bogus class
    // isn't in the registry - the exact same "looks like silence"
    // failure mode the instanceof/self bug had).
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            class MyError extends \Exception {}
            function f() {
                try {
                    throw new MyError();
                } catch (MyError $e) {
                    return $e->totallyMadeUp();
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("App\\MyError::totallyMadeUp")),
        "expected the resolved fully-qualified class in the diagnostic, got: {d:?}"
    );
}

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

#[test]
fn nested_class_and_enum_declared_inside_an_if_get_their_bodies_checked_too() {
    // Same fix as the FunctionDecl test above, but for the other two
    // arms check_stmt gained at the same time - ClassDecl and
    // EnumDecl nested inside a block (`if (!class_exists('Foo')) {
    // class Foo {...} }`, a real BC-compat idiom). Coverage
    // measurement found these two arms specifically still had zero
    // executions even after that fix landed, since the earlier test
    // only exercised the FunctionDecl arm.
    let class_diagnostics = diagnostics_for(
        r#"<?php
            if (!class_exists('Foo')) {
                class Foo {
                    public function bar() {
                        return $undefinedInNestedClass;
                    }
                }
            }
        "#,
    );
    assert!(
        class_diagnostics
            .iter()
            .any(|x| x.message.contains("undefined variable")),
        "expected an undefined-variable diagnostic, got: {class_diagnostics:?}"
    );

    let enum_diagnostics = diagnostics_for(
        r#"<?php
            if (!enum_exists('Status')) {
                enum Status {
                    case Active;
                    public function label(): string {
                        return $undefinedInNestedEnum;
                    }
                }
            }
        "#,
    );
    assert!(
        enum_diagnostics
            .iter()
            .any(|x| x.message.contains("undefined variable")),
        "expected an undefined-variable diagnostic, got: {enum_diagnostics:?}"
    );
}

#[test]
fn do_while_switch_and_static_var_bodies_are_all_checked() {
    // Coverage measurement found StmtKind::DoWhile, StmtKind::Switch, and
    // StmtKind::StaticVar all had zero executions anywhere in checker.rs's
    // own test suite - manually verified correct via the CLI earlier
    // in this audit; this locks all three in as permanent tests.
    let do_while = diagnostics_for(
        r#"<?php
            function f() {
                do {
                    echo $undefinedInDoWhile;
                } while (true);
            }
        "#,
    );
    assert!(
        do_while
            .iter()
            .any(|x| x.message.contains("undefined variable")),
        "expected an undefined-variable diagnostic, got: {do_while:?}"
    );

    let switch = diagnostics_for(
        r#"<?php
            function f($x) {
                switch ($x) {
                    case 1:
                        echo $undefinedInSwitchCase;
                        break;
                }
            }
        "#,
    );
    assert!(
        switch
            .iter()
            .any(|x| x.message.contains("undefined variable")),
        "expected an undefined-variable diagnostic, got: {switch:?}"
    );

    let static_var = diagnostics_for(
        r#"<?php
            function f() {
                static $count = 0;
                return $undefinedInStaticVarBody;
            }
        "#,
    );
    assert!(
        static_var
            .iter()
            .any(|x| x.message.contains("undefined variable")),
        "expected an undefined-variable diagnostic, got: {static_var:?}"
    );
}

#[test]
fn an_inline_var_tag_on_a_static_declaration_overrides_its_default_expression() {
    // Real corpus find (Piwigo's own `MessengerRoundTripTest::marker()`):
    // `static $x = null;` used to type `$x` as exactly `Null`, silently
    // clobbering an already-applied `/** @var string|null */` tag right
    // above it - every other assignment shape already lets `@var`
    // override its own natural inference; a `static` declaration is no
    // different. Asserts against Phase 60's own `nullCoalesce.variable`
    // (which the pre-fix `Null` inference wrongly fired, "always null")
    // as the most direct evidence the annotation is actually honored.
    let d = diagnostics_for(
        r#"<?php
            function marker(): string {
                /** @var string|null */
                static $marker = null;
                return $marker ??= 'x';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "nullCoalesce.variable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn while_loop_narrows_its_body_and_merges_types_correctly_after() {
    // Same coverage-driven finding: StmtKind::While had zero test coverage
    // for both its instanceof-narrowing inside the body and its
    // post-loop merge_scopes (the loop may run zero times, so a type
    // reassigned only inside the body must come out as a union of the
    // pre-loop and post-body types afterward, not just the body's).
    let inside_narrowing = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            function f(?Foo $x) {
                while ($x instanceof Foo) {
                    $x->bar();
                    $x->totallyMadeUp();
                }
            }
        "#,
    );
    assert!(
        inside_narrowing
            .iter()
            .any(|x| x.message.contains("Foo::totallyMadeUp")),
        "expected instanceof narrowing to apply inside a while body, got: {inside_narrowing:?}"
    );

    let post_loop_merge = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function f() {
                $x = 5;
                while (rand(0, 1)) {
                    $x = "hello";
                }
                needsInt($x);
            }
        "#,
    );
    assert!(
        post_loop_merge
            .iter()
            .any(|x| x.message.contains("expects int")),
        "expected the post-loop type to be a union of both possible types, got: {post_loop_merge:?}"
    );
}

#[test]
fn for_loop_init_binds_its_variable() {
    // Regression test: the actual assignment-binding side effect used
    // to live only in a separate pass triggered from bare expression
    // statements, so `for ($j = 0; ...)`'s own $j was never bound and
    // would have misfired as undefined the moment this check existed.
    let d = diagnostics_for(
        r#"<?php
            function f() {
                for ($j = 0; $j < 3; $j++) {
                    echo $j;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn variable_defined_in_only_one_if_branch_is_flagged_after_the_if() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond) {
                if ($cond) {
                    $x = 5;
                }
                return $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("$x"));
}

#[test]
fn variable_defined_in_both_if_and_else_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond) {
                if ($cond) {
                    $x = 5;
                } else {
                    $x = 10;
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn throwing_a_non_throwable_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class NotAnException2 {}
            function test(): void { throw new NotAnException2(); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("cannot throw NotAnException2: it does not implement Throwable")),
        "expected a cannot-throw diagnostic, got: {d:?}"
    );
}

#[test]
fn throw_expression_of_a_non_throwable_class_is_flagged() {
    // Same check as `throwing_a_non_throwable_class_is_flagged` just
    // above, but via a `throw` *expression* (PHP 8's `??`/arrow-fn/
    // match-arm shape) rather than a bare `throw` statement - a
    // previously-real gap (`ExprKind::Throw` never ran this check at
    // all, only `StmtKind::Throw` did).
    let d = diagnostics_for(
        r#"<?php
            class NotAnException3 {}
            function test(?int $x): int {
                return $x ?? throw new NotAnException3();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("cannot throw NotAnException3: it does not implement Throwable")),
        "expected a cannot-throw diagnostic, got: {d:?}"
    );
}

// Real bug found and fixed this session: both `StmtKind::Throw` and
// `ExprKind::Throw` required the thrown value's own type to be literally
// `Type::Object`, silently skipping the check for a `Union`/
// `Intersection` - a value narrowed via `instanceof` from an unrelated
// interface, then thrown, is exactly as real a shape here as it was for
// property access (see `checker::tests::narrowing`'s own tests for that
// bug). Neither `View` nor `NotThrowable` implements `Throwable`, so
// every member of the resulting intersection fails.
#[test]
fn throwing_a_value_narrowed_to_an_intersection_of_two_non_throwables_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            class NotThrowable {}
            function test(View $view): void {
                if ($view instanceof NotThrowable) {
                    throw $view;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "throw.notThrowable"),
        "got: {d:?}"
    );
}

// Same shape, but the intersection's *second* member genuinely is
// Throwable - real PHP only needs the runtime value to satisfy the
// property once, so this must NOT be flagged.
#[test]
fn throwing_a_value_narrowed_to_an_intersection_including_a_real_throwable_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            class RealException extends \Exception {}
            function test(View $view): void {
                if ($view instanceof RealException) {
                    throw $view;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throw.notThrowable"),
        "got: {d:?}"
    );
}

// Real bug found and fixed this session, discovered via a real corpus
// line the Union fix above newly made reachable: `is_subclass_of`
// silently treats a name the registry has no entry for as "not a
// subclass of Throwable," indistinguishable from a real, known,
// non-Throwable class - `class_name_exists` must be checked first,
// mirroring `StmtKind::Try`'s own multi-catch handling a few hundred
// lines up (which already guards this exact case). Real corpus shape:
// a multi-catch `catch (A|B $e) { throw $e; }` binds `$e` to a real
// union type - one of `A`/`B` living in an unresolvable/unscanned
// vendored package wrongly reported "does not implement Throwable"
// even though the real interface genuinely does.
#[test]
fn throwing_a_union_including_an_unresolvable_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(bool $cond): void {
                try {
                    if ($cond) {
                        throw new \RuntimeException('x');
                    }
                } catch (\SomeUnresolvableInterface|\RuntimeException $e) {
                    throw $e;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throw.notThrowable"),
        "got: {d:?}"
    );
}

#[test]
fn throw_expression_of_a_real_exception_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(?int $x): int {
                return $x ?? throw new \RuntimeException('bad');
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "throw.notThrowable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn throwing_a_real_exception_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void { throw new \RuntimeException("x"); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn catching_a_non_throwable_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class NotAnException1 {}
            function doStuff1(): void {}
            function test(): void {
                try { doStuff1(); } catch (NotAnException1 $e) { echo "caught"; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("cannot catch NotAnException1: it does not implement Throwable")),
        "expected a cannot-catch diagnostic, got: {d:?}"
    );
}

#[test]
fn catching_a_real_exception_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function doStuff1(): void {}
            function test(): void {
                try { doStuff1(); } catch (\RuntimeException $e) { echo "caught"; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 42: CatchWithUnthrownExceptionRule's "already caught" half
// (`catch.alreadyCaught`) - a later catch clause every one of whose
// types is already a subtype of something an earlier clause caught can
// never actually run, since PHP tries each clause in source order and
// stops at the first match.

#[test]
fn a_later_catch_clause_for_a_subtype_already_caught_above_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function doStuff2(): void {}
            function test(): void {
                try {
                    doStuff2();
                } catch (\Exception $e) {
                    echo "outer";
                } catch (\RuntimeException $e) {
                    echo "inner";
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "catch.alreadyCaught"
            && x.message == "Dead catch - RuntimeException is already caught above."),
        "got: {d:?}"
    );
}

#[test]
fn a_later_catch_clause_for_the_exact_same_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function doStuff3(): void {}
            function test(): void {
                try {
                    doStuff3();
                } catch (\RuntimeException $e) {
                    echo "first";
                } catch (\RuntimeException $e) {
                    echo "second";
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "catch.alreadyCaught"),
        "got: {d:?}"
    );
}

#[test]
fn catch_clauses_in_a_narrow_to_wide_order_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function doStuff4(): void {}
            function test(): void {
                try {
                    doStuff4();
                } catch (\RuntimeException $e) {
                    echo "specific";
                } catch (\Exception $e) {
                    echo "general";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "catch.alreadyCaught"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn unrelated_catch_clauses_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function doStuff5(): void {}
            function test(): void {
                try {
                    doStuff5();
                } catch (\LogicException $e) {
                    echo "logic";
                } catch (\RuntimeException $e) {
                    echo "runtime";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "catch.alreadyCaught"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 42(4), `OverwrittenExitPointByFinallyRule`: a real, well-known
// PHP gotcha - a `finally` block's own `return`/`throw`/`break`/
// `continue` always wins, even over an in-flight exception or the try
// body's own return, silently discarding it. Verified byte-for-byte
// against real PHPStan (message text, line numbers, and identifier)
// via a direct `vendor/bin/phpstan` run on this exact file.
#[test]
fn a_finally_block_unconditionally_returning_overwrites_the_try_bodys_return() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                try {
                    return 1;
                } finally {
                    return 2;
                }
            }
        "#,
    );
    let exit_points: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "finally.exitPoint")
        .collect();
    assert_eq!(exit_points.len(), 2, "got: {d:?}");
    assert!(
        exit_points[0]
            .message
            .contains("This return is overwritten by a different one in the finally block below.")
    );
    assert!(
        exit_points[1]
            .message
            .contains("The overwriting return is on this line.")
    );
}

// Phase 68.2: `finally`'s own real entry scope isn't just the try body's
// full-completion state - real PHP can jump into `finally` the moment an
// exception is thrown anywhere inside the try body, including before a
// variable it conditionally reassigns ever runs. Real corpus false
// positive this fixes: Piwigo's own `tests/Integration/
// GroupServiceTest.php`, `$copyId = null;` reassigned only conditionally
// inside `try`, `finally` wrongly seeing only the reassigned state.

#[test]
fn finally_sees_the_pre_try_state_too_when_an_earlier_statement_could_throw() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function mightThrow(): void {}
            function f(): void {
                $x = null;
                try {
                    mightThrow();
                    $x = new Foo();
                } finally {
                    assertType('null|Foo', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn finally_widens_even_when_the_reassignment_is_the_trys_own_first_statement() {
    // A real, disclosed trade-off of this "materially cheaper
    // approximation": it has no real per-statement throw-point model at
    // all (out of reach at reasonable size - see this function's own
    // doc comment), so it merges the *whole* pre-try snapshot with the
    // post-try state unconditionally, even here, where `$x` is
    // reassigned as the *very first* statement in the try body and real
    // PHP genuinely cannot throw between entering the try and completing
    // a plain assignment. A real per-throw-point model could narrow this
    // back down to plain `Foo`; this approximation deliberately doesn't
    // attempt to.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function f(): void {
                $x = null;
                try {
                    $x = new Foo();
                } finally {
                    assertType('null|Foo', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn finally_sees_a_catch_clauses_own_binding_merged_with_the_pre_try_state() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function f(): void {
                $x = null;
                try {
                    mightThrowOrReturn();
                    $x = new Foo();
                } catch (\Exception $e) {
                } finally {
                    assertType('null|Foo', $x);
                }
            }
            function mightThrowOrReturn(): void {}
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn code_after_the_whole_try_statement_is_unaffected_by_the_finally_widening() {
    // The widened, pre-try-merged scope is only ever fed to `finally`'s
    // own body - normal code after the whole try/finally statement still
    // sees the real, narrower post-try/catch completion state (unless
    // `finally` itself further widens it, which this simple case
    // doesn't).
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function f(): void {
                $x = null;
                try {
                    $x = new Foo();
                } finally {
                    echo "cleanup";
                }
                assertType('Foo', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn a_finally_block_with_no_exit_point_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                try {
                    return 1;
                } finally {
                    echo "cleanup";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "finally.exitPoint"),
        "unexpected diagnostics: {d:?}"
    );
}

// Both sides nested inside an if/else - `collect_exit_point_statements`
// must recurse into both branches, not just the block's own last
// top-level statement.
#[test]
fn a_finally_block_exit_point_nested_in_if_else_is_still_detected() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond): int {
                try {
                    if ($cond) {
                        return 1;
                    } else {
                        return 2;
                    }
                } finally {
                    if ($cond) {
                        return 3;
                    } else {
                        return 4;
                    }
                }
            }
        "#,
    );
    let exit_points: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "finally.exitPoint")
        .collect();
    assert_eq!(exit_points.len(), 4, "got: {d:?}");
}

// The try side's own catch clause, not just its main body, is a real
// source of overwritten exit points too.
#[test]
fn a_finally_block_overwrites_a_catch_clauses_own_return() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                try {
                    return 1;
                } catch (\Exception $e) {
                    return 2;
                } finally {
                    return 3;
                }
            }
        "#,
    );
    let exit_points: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "finally.exitPoint")
        .collect();
    assert_eq!(exit_points.len(), 3, "got: {d:?}");
}

// `break`/`continue` inside a loop are exit points too, not just
// `return`/`throw` - real PHPStan's own rule treats all four uniformly.
#[test]
fn a_finally_blocks_break_overwrites_the_try_bodys_continue() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $items): void {
                foreach ($items as $item) {
                    try {
                        continue;
                    } finally {
                        break;
                    }
                }
            }
        "#,
    );
    let exit_points: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "finally.exitPoint")
        .collect();
    assert_eq!(exit_points.len(), 2, "got: {d:?}");
    assert!(
        exit_points[0]
            .message
            .contains("This continue is overwritten")
    );
    assert!(
        exit_points[1]
            .message
            .contains("The overwriting break is on this line.")
    );
}

// Phase 23 Finding 5, real shape:
// tests/Unit/Mail/MailServiceTestFakeSmtpServer.php's `$conn =
// stream_socket_accept(...); if ($conn === false) { fwrite(STDERR,
// ...); exit(1); } stream_set_timeout($conn, 5);` - `exit`/`die`
// never inferred as `Type::Never`, so this guard clause wasn't
// recognized as terminating and the then-branch's narrowing leaked
// into the surrounding scope past the `if`.
#[test]
fn a_guard_clause_built_on_exit_terminates_like_return() {
    let d = diagnostics_for(
        r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $conn = open();
                if ($conn === false) {
                    exit(1);
                }
                fclose($conn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_guard_clause_built_on_die_terminates_like_return() {
    let d = diagnostics_for(
        r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $conn = open();
                if ($conn === false) {
                    die('failed');
                }
                fclose($conn);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn for_loop_that_can_run_zero_times_does_not_define_a_body_only_variable() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                for ($i = 0; $i < 0; $i++) {
                    $x = 5;
                }
                return $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefined variable $x"), "got: {d:?}");
}

// Phase 29, real shape: Piwigo\Core\Lang::load's own `$source_file =
// ''; foreach ($languages as $language) { $f = ...; if (found) {
// $source_file = $f; break; } } if ($source_file === '') { return
// false; } preg_replace(..., $source_file);` - a `break` reached
// inside an `if` had its own state (a real string) silently
// discarded from what flows past the loop, since `always_terminates`
// correctly stops it from leaking into the *rest of the same block*
// but nothing captured it for the *loop's own* exit state. Left
// `$source_file` permanently typed as the literal `''` after the
// loop, which then collapsed to `Type::Never` at the `=== ''` guard -
// vacuously assignable to anything, masking every real check after it.
#[test]
fn a_break_reached_inside_an_if_is_seen_by_code_after_its_foreach_loop() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $items): string {
                $found = '';
                foreach ($items as $item) {
                    if (is_string($item)) {
                        $found = $item;
                        break;
                    }
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_break_reached_inside_an_if_is_seen_by_code_after_its_while_loop() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $items): string {
                $found = '';
                $i = 0;
                while ($i < count($items)) {
                    if (is_string($items[$i])) {
                        $found = $items[$i];
                        break;
                    }
                    $i++;
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_break_reached_inside_an_if_is_seen_by_code_after_its_for_loop() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $rows): string {
                $found = '';
                for ($i = 0; $i < count($rows); $i++) {
                    if (is_string($rows[$i])) {
                        $found = $rows[$i];
                        break;
                    }
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_multi_level_break_reaches_the_correct_outer_loop() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $matrix): string {
                $found = '';
                foreach ($matrix as $row) {
                    foreach ($row as $cell) {
                        if (is_string($cell)) {
                            $found = $cell;
                            break 2;
                        }
                    }
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_break_inside_a_switch_inside_a_loop_does_not_leak_to_the_loop() {
    // The switch's own `break` targets the switch itself, not the
    // enclosing loop - confirming the frame this fix pushes around a
    // `Switch` (to intercept exactly this) doesn't over-eagerly
    // propagate the switch's own internal break into the loop's own
    // post-loop merge (which, here, has no break of its own at all,
    // so it should merge exactly as if this fix didn't exist).
    let d = diagnostics_for(
        r#"<?php
            function f(array $items): void {
                $x = null;
                foreach ($items as $item) {
                    switch (true) {
                        case is_string($item):
                            $x = 'from switch';
                            break;
                    }
                }
                if ($x !== null) {
                    strtoupper($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 29, real shape: found and reverted mid-fix -
// `Piwigo\Calendar\CalendarMonthly::buildMonthCalendar`'s own
// `foreach ($items as $day => $data) { ...; $items[$day]['derivative']
// = ...; $items[$day]['file'] = ...; }` mutates the very same array
// it iterates, via a nested write keyed by the loop variable itself.
// An earlier version of this fix also merged `Foreach` with its own
// pre-loop state (matching `While`/`For`'s existing "may run zero
// times" treatment, for consistency) - but every key iterated over
// here really does get the loop's own treatment, so unioning in the
// *stale pre-loop* shape as an "also possible" alternative produced a
// false "array{...} has no key 'derivative'" the real code never
// risks. `Foreach` deliberately keeps checking its body once directly
// against the real `locals`, gaining only the `break`-state capture.
#[test]
fn foreach_mutating_its_own_subject_via_the_loop_key_does_not_regress() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $rows): void {
                $items = [];
                foreach ($rows as $row) {
                    $items[(int) $row] = ['nb_images' => 1];
                }
                foreach ($items as $day => $data) {
                    $items[$day]['derivative'] = 'x';
                    $items[$day]['file'] = 'y';
                }
                foreach ($items as $day => $data) {
                    echo $items[$day]['derivative'];
                    echo $items[$day]['file'];
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real Piwigo corpus bug: an unassigned superglobal defaulted to
// Type::Mixed rather than Array(Mixed) - real phpstorm-stubs' own
// bootstrap file declares $_SESSION with a literal `= []`, which real
// PHPStan generalizes to a bare `array`. Because widen_coalesce_
// result's "keep the left side's real array type" rule only fires for
// an already-known Type::Array, a Mixed left side let `??=` collapse
// to the literal empty shape, flagging the very next unset() as
// targeting a key that provably doesn't exist.
#[test]
fn a_superglobal_coalesce_assign_does_not_collapse_to_an_empty_shape() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $_SESSION ??= [];
                unset($_SESSION['some_key']);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real Piwigo corpus bug: isset()/empty()'s own direct argument went
// through the ordinary check_expr, so isset($shaped['maybe_missing'])
// hit the same check_shape_key_access a bare read would - even though
// probing a possibly-absent key is exactly what isset()/empty() are
// for, and real PHP/PHPStan never flag it *that way*.
//
// Phase 60 correction: real PHPStan's own `checkAdvancedIsset` (live at
// level 4+, confirmed directly against `vendor/phpstan-src` - this
// project doesn't model PHPStan's own level system as a separate opt-in
// toggle any more than it does for any other level-gated check, matching
// `Diagnostic::level`'s own "metadata for --level filtering, not a
// runtime gate" precedent) genuinely *does* flag `isset($shaped['b'])`
// here - `b` is provably absent from `array{a: string}`, a real,
// distinct diagnostic (`isset.offset`, "does not exist") from the old
// `check_shape_key_access` bug this test originally guarded against.
// Phase 60 follow-up: `empty()` now gets the identical offset/property
// treatment as `isset()` (both funnel through the same real PHPStan
// `IssetCheck`) - `empty($x['c'])` is flagged too, `empty.offset`, since
// `c` is just as provably absent as `b`.
#[test]
fn isset_and_empty_on_a_shape_s_missing_key_never_hit_the_old_shape_key_bug() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{a: string} $x */
            function f(array $x): bool {
                return isset($x['b']) || empty($x['c']);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "shapeAccess.notFound"),
        "the old check_shape_key_access bug reappeared: {d:?}"
    );
    assert_eq!(d.len(), 2, "expected isset.offset and empty.offset: {d:?}");
    assert!(d.iter().any(|x| x.identifier == "isset.offset"));
    assert!(d.iter().any(|x| x.identifier == "empty.offset"));
}

// Real Piwigo corpus bug (test-double-recording idiom): a closure's
// own `use (&$x)` mutation never wrote back to the declaring scope,
// so `$x`'s type at a later read site stayed frozen at whatever it
// was *before* the closure literal.
#[test]
fn a_closure_s_byref_use_mutation_is_visible_after_the_closure_literal() {
    let d = diagnostics_for(
        r#"<?php
            function invoke(callable $cb): void {
                $cb();
            }
            function f(): string {
                $seen = [];
                $record = function () use (&$seen): void {
                    $seen[] = 'x';
                };
                invoke($record);
                return $seen[0];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 31 from the architecture audit - a bare `return;`, and
// falling off the end of the function body with no `return` at all,
// were never checked against a non-nullable, non-void declared
// return type.

#[test]
fn bare_return_is_flagged_against_a_non_nullable_return_type() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): int {
                if ($c) {
                    return;
                }
                return 5;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("got null"), "got: {d:?}");
}

#[test]
fn bare_return_is_not_flagged_against_void_or_nullable() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): void {
                if ($c) {
                    return;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): ?int {
                if ($c) {
                    return;
                }
                return 5;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn falling_off_the_end_is_flagged_against_a_non_nullable_return_type() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int { echo "hello"; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    // Phase 61: distinct from `return.type` (`bare_return_is_flagged_
    // against_a_non_nullable_return_type`'s own "returned literal null"
    // case just above) - falling off the end never explicitly returns
    // anything at all, real PHPStan's own dedicated `return.missing`.
    assert_eq!(d[0].identifier, "return.missing", "got: {d:?}");
    assert!(
        d[0].message.contains("return statement is missing"),
        "got: {d:?}"
    );
}

#[test]
fn a_never_typed_function_falling_off_the_end_gets_return_never() {
    let d = diagnostics_for(
        r#"<?php
            function f(): never { echo "hello"; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "return.never", "got: {d:?}");
}

#[test]
fn a_never_typed_function_reaching_a_valued_return_gets_return_never() {
    let d = diagnostics_for(
        r#"<?php
            function f(): never {
                return 5;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "return.never", "got: {d:?}");
}

#[test]
fn a_never_typed_function_reaching_a_bare_return_gets_return_never() {
    let d = diagnostics_for(
        r#"<?php
            function f(): never {
                return;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "return.never", "got: {d:?}");
}

#[test]
fn a_never_typed_function_that_actually_never_returns_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): never {
                throw new \Exception();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_void_function_returning_a_value_gets_return_void() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                return 5;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "return.void", "got: {d:?}");
}

// `return anotherVoidFn();` delegating to another void-returning call is
// legal, common PHP - real PHPStan doesn't flag it either, since it's
// not "returning a value" at all, just handing back void-ness itself.
// Real corpus false positive this exact idiom used to hit ("expected
// void, got void"): `RectorPrefix202608\trigger_deprecation`-style
// `function shim() { return \Prefix\shim(...func_get_args()); }`
// forwarding wrappers, once the delegate's own signature correctly
// resolves to `void`.
#[test]
fn a_void_function_delegating_to_another_void_function_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function inner(): void {}
            function outer(): void {
                return inner();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_void_function_with_a_bare_return_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                return;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_function_whose_every_path_returns_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): int {
                if ($c) {
                    return 1;
                } else {
                    return 2;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_function_ending_in_an_exhaustive_switch_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): string {
                switch ($n) {
                    case 1:
                        return "one";
                    default:
                        return "other";
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_function_ending_in_a_switch_with_no_default_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): string {
                switch ($n) {
                    case 1:
                        return "one";
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn switch_cases_ending_in_break_still_propagate_their_definitions_past_the_switch() {
    // Regression test for a real bug caught while implementing this
    // fix: the Finding 55 rewrite of Switch's own narrowing stopped
    // tracking what a `break`-terminated case (the most common real
    // switch style) leaves defined for the code after the switch.
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): int {
                switch ($x) {
                    case 1:
                        $y = 1;
                        break;
                    default:
                        $y = 2;
                        break;
                }
                return $y;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Re-examining Phase 15's own disclosed "continue N's own level argument
// is parsed but never consulted for narrowing purposes" per the user's
// own explicit "do all those narrow [items], our corpus not having them
// is not a reason not to do them": real PHP's own well-known
// `continue`-inside-`switch` gotcha (a bare `continue;`/`continue 1;`
// written directly in a case is exactly `break`-equivalent - it exits
// the switch, not any enclosing loop) was not recognized anywhere. Two
// distinct, real bugs shared this one root cause, both found and fixed
// together: `always_terminates`'s own `Switch` arm treated *any*
// `Continue` (regardless of level) the same as `Return`/`Throw`, so a
// switch with every case ending in either `return` or a break-equivalent
// `continue` was wrongly judged to always terminate the whole function,
// flagging the code after it as unreachable dead code; separately,
// `reaches_after_switch`'s own first, unconditional arm only special-
// cased a literal `Break` this way, leaving a *non-last* case ending in
// a break-equivalent `continue` gated behind `is_last` (which a non-last
// case can never satisfy) even after `exits_function_or_loop` was
// itself already fixed - `is_switch_break_equivalent` now covers both.
// `exits_function_or_loop`'s own half of the same fix, isolated: the
// *last* case (here the only one) ending in a bare `continue` - routed
// through `reaches_after_switch`'s second, `is_last`-gated arm, not the
// first, unconditional one the test above exercises.
#[test]
fn the_last_switch_case_ending_in_a_bare_continue_reaches_after_the_switch() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): string {
                switch ($x) {
                    default:
                        $result = "value";
                        continue;
                }
                return $result;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_non_last_switch_case_ending_in_a_bare_continue_reaches_after_the_switch() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): string {
                switch ($x) {
                    case 1:
                        $onlyFromCase1 = "one";
                        continue;
                    default:
                        return "other";
                }
                return $onlyFromCase1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `is_switch_break_equivalent`'s own further real find, verifying the
// fix just above wasn't complete either: a case's own last statement can
// be an `if` whose *every* branch is itself break-equivalent
// (`if ($c) { continue; } else { continue; }`), unconditionally exiting
// the switch on every path just like a bare `continue;` would - not
// recognized at all until `is_switch_break_equivalent` itself recursed
// through `Block`/`If` the same way `always_terminates` already does.
#[test]
fn a_switch_case_ending_in_an_if_whose_every_branch_continues_reaches_after_the_switch() {
    // `default:` deliberately never reaches after the switch at all
    // (ends in `return`) - the *only* way `return $onlyFromCase1;` is
    // reachable is via `case 1`'s own nested-`continue` exit, isolating
    // whether that exit is correctly recognized (a `default:` that also
    // assigned `$onlyFromCase1` would pass either way, masking the bug).
    let d = diagnostics_for(
        r#"<?php
            function f(int $x, bool $c): string {
                switch ($x) {
                    case 1:
                        $onlyFromCase1 = "a";
                        if ($c) {
                            continue;
                        } else {
                            continue;
                        }
                    default:
                        return "other";
                }
                return $onlyFromCase1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_function_ending_in_a_try_catch_that_both_return_is_not_flagged() {
    // Regression test for a real bug caught while implementing this
    // fix: `always_terminates` had no case for `Try` at all, so a
    // function ending in `try { return ...; } catch (...) { return
    // ...; }` was incorrectly flagged as possibly falling off the
    // end.
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): string {
                try {
                    if ($c) {
                        throw new \Exception("bad");
                    }
                    return "ok";
                } catch (\Throwable $e) {
                    return "error";
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real corpus shapes (NotificationController::findAvailableFeedId,
// AdvisorySessionLock::acquirePostgres, NotificationByMailService::
// findAvailableCheckKey, all `while (true) { ...; if (...) { return
// $x; } }` find-a-free-key/retry loops) - `always_terminates` had no
// case for `While`/`For` at all, so a function ending in one of these
// (with no `break` anywhere in the loop body) was incorrectly flagged
// as possibly falling off the end.
#[test]
fn a_while_true_loop_with_no_break_and_a_return_inside_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                while (true) {
                    $key = 'x';
                    if ($key !== 'y') {
                        return $key;
                    }
                }
            }
        "#,
    );
    // Phase 15 item (2): this breakless `while (true)` now legitimately
    // gets its own `while.alwaysTrue` (a `return` inside doesn't
    // disqualify it - matches real PHPStan's own rule, confirmed
    // against `vendor/phpstan-src`) - this test's own actual purpose is
    // narrower than "no diagnostics at all", see the doc comment above.
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_for_loop_with_a_true_condition_and_no_break_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                for ($i = 0; true; $i++) {
                    if ($i > 3) {
                        return 'x';
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_while_true_loop_with_a_reachable_break_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): string {
                while (true) {
                    if ($c) {
                        return 'x';
                    }
                    break;
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "return.missing", "got: {d:?}");
}

// A round of Piwigo-corpus bucketing found a catch clause's own
// ending state was discarded entirely after the whole try/catch -
// `locals` only ever kept the try body's own final state, matching
// this project's own documented "no scope merge back after try/
// catch" limitation. Real shape: `$body = null; try { ...; } catch
// (...) { $body = "x"; }` then using `$body` afterward.
#[test]
fn a_variable_assigned_only_inside_a_catch_clause_is_visible_after_the_try_catch() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $x */
            function sink(string $x): void {}
            function f(): void {
                $body = 'default';
                try {
                    riskyCall();
                } catch (\Throwable $e) {
                    $body = 'from catch';
                }
                sink($body);
            }
            function riskyCall(): void {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_catch_clause_that_always_returns_is_excluded_from_the_post_try_merge() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $x */
            function sink(string $x): void {}
            function f(): string {
                $body = 'default';
                try {
                    riskyCall();
                } catch (\Throwable $e) {
                    return 'error';
                }
                sink($body);
                return $body;
            }
            function riskyCall(): void {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 37 from the architecture audit - array destructuring bound
// nothing in a `foreach` value position, and a nested destructuring
// pattern bound nothing even in a plain assignment; checking the
// left-hand side itself had the identical one-level-only gap.

#[test]
fn foreach_value_destructuring_binds_its_variables() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $pairs = [[1, 'a'], [2, 'b']];
                foreach ($pairs as [$num, $letter]) {
                    echo $num . $letter;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn foreach_value_list_syntax_destructuring_binds_its_variables() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $pairs = [[1, 'a'], [2, 'b']];
                foreach ($pairs as list($num, $letter)) {
                    echo $num . $letter;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn nested_destructuring_assignment_binds_every_level() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $nested = [[1, 2], 3];
                [[$a, $b], $c] = $nested;
                echo $a . $b . $c;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn nested_destructuring_does_not_check_its_own_targets_as_reads() {
    // Regression test for a second, related bug found while fixing
    // this one: checking the assignment's own left-hand side (not
    // just binding it) only recursed one level into a destructuring
    // pattern, so `$a`/`$b` were flagged as undefined on the
    // destructuring statement itself, before binding ever ran.
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $nested = [[1, 2], 3];
                [[$a, $b], $c] = $nested;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn swap_idiom_still_leaves_a_genuinely_undefined_variable_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                [$a, $b] = [1, 2];
                echo $c;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefined variable $c"), "got: {d:?}");
}

// Finding 27 from the architecture audit - a multi-type catch clause
// bound only its first listed type, and MethodCall/PropertyAccess
// never matched a Union/Intersection receiver at all, silently
// skipping undefined-method/property checking for either.

#[test]
fn multi_catch_binds_the_union_of_every_listed_type_regardless_of_order() {
    let first_order = diagnostics_for(
        r#"<?php
            class LogicEx extends \LogicException {}
            class SpecialEx extends \Exception {
                public function getSpecialData(): string { return "data"; }
            }
            function f(): void {
                try { throw new SpecialEx(); }
                catch (LogicEx|SpecialEx $e) { echo $e->getSpecialData(); }
            }
        "#,
    );
    let second_order = diagnostics_for(
        r#"<?php
            class LogicEx extends \LogicException {}
            class SpecialEx extends \Exception {
                public function getSpecialData(): string { return "data"; }
            }
            function f(): void {
                try { throw new SpecialEx(); }
                catch (SpecialEx|LogicEx $e) { echo $e->getSpecialData(); }
            }
        "#,
    );
    // Both orderings describe the same real ambiguity ($e could be
    // LogicEx, which lacks getSpecialData()) - the verdict must not
    // depend on which type happens to be written first.
    assert_eq!(first_order.len(), 1, "got: {first_order:?}");
    assert_eq!(second_order.len(), 1, "got: {second_order:?}");
}

// Finding 38 from the architecture audit - unset($x) was treated as
// *defining* $x (a side effect of the same mechanism that makes
// isset()/empty() safe on a maybe-undefined variable) rather than
// removing it.

#[test]
fn using_a_variable_after_unset_is_flagged_as_undefined() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $x = 5;
                unset($x);
                echo $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefined variable $x"), "got: {d:?}");
}

#[test]
fn unset_with_multiple_arguments_removes_every_one() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $a = 1;
                $b = 2;
                unset($a, $b);
                echo $a;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn isset_and_empty_are_still_unaffected_by_the_unset_fix() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): string {
                if ($c) { $r = "x"; }
                return isset($r) ? $r : "no";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpstan_caught_exception_existence_flags_a_non_throwable_catch() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Exceptions/data/catch.php:
    // upstream's own expected message is "Caught class
    // TestCatch\FooCatch is not an exception."
    let d = diagnostics_for(
        r#"<?php
            class FooCatch {}
            function test(): void {
                try {
                } catch (FooCatch $e) {
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| {
        x.message
            .contains("cannot catch FooCatch: it does not implement Throwable")
    }));
}

#[test]
fn phpstan_throw_expr_type_flags_throwing_a_non_throwable() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Exceptions/data/throw-values.php's
    // `throw new InvalidException();` case (a plain class with no
    // Throwable in its hierarchy).
    let d = diagnostics_for(
        r#"<?php
            class InvalidException {}
            function test(): void {
                throw new InvalidException();
            }
        "#,
    );
    assert!(d.iter().any(|x| {
        x.message
            .contains("cannot throw InvalidException: it does not implement Throwable")
    }));
}

// Phase 70: `generator.returnType` - confirmed directly via `php -l` as
// a real, unconditional PHP fatal error at compile time: a function
// body containing `yield` must declare a return type compatible with
// Generator/Iterator/Traversable/iterable.

#[test]
fn a_generator_function_declared_to_return_int_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                yield 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_generator_function_declared_to_return_array_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                yield 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_generator_function_declared_to_return_void_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                yield 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_generator_function_declared_to_return_generator_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): Generator {
                yield 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_docblock_return_array_shape_on_a_native_iterable_generator_is_never_flagged() {
    // Real corpus find (vendor/doctrine/inflector's own
    // `Inflectible::getSingular()`): a `@return Transformation[]`
    // docblock on a `: iterable`-typed generator describes what
    // iterating it *produces*, not a redeclaration of the native type -
    // real PHP's own "Generator return type must be a supertype of
    // Generator" fatal error is checked against the *native* type alone,
    // never the PHPDoc-refined one.
    let d = diagnostics_for(
        r#"<?php
            class Transformation {}
            class Foo {
                /** @return Transformation[] */
                public function f(): iterable {
                    yield new Transformation();
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_generator_function_declared_to_return_iterator_or_traversable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): Iterator {
                yield 1;
            }
            function g(): Traversable {
                yield 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_generator_function_declared_to_return_iterable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): iterable {
                yield 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_generator_function_declared_to_return_a_nullable_generator_is_never_flagged() {
    // Confirmed directly via php -l: real PHP accepts this even though
    // calling the function never actually produces null.
    let d = diagnostics_for(
        r#"<?php
            function f(): ?Generator {
                yield 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_generator_function_declared_to_return_an_unrelated_union_with_generator_is_never_flagged() {
    // Confirmed directly via php -l: real PHP accepts `int|Generator`
    // outright - only one union member needs to be Generator-compatible.
    let d = diagnostics_for(
        r#"<?php
            function f(): int|Generator {
                yield 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_generator_function_with_no_declared_return_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                yield 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_generator_function_is_never_flagged_regardless_of_return_type() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                return 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generator.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_method_that_yields_with_a_wrong_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function f(): int {
                    yield 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_yield_from_with_a_wrong_declared_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                yield from [1, 2];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_generator_closure_with_a_wrong_declared_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            $gen = function (): int {
                yield 1;
            };
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_generator_arrow_function_with_a_wrong_declared_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            $gen = fn (): int => yield 1;
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "generator.returnType"),
        "got: {d:?}"
    );
}

// Phase 55: fixed-point loop-body convergence (`Checker::
// converge_loop_body`). Before this, a loop body was checked exactly
// once against the pre-loop entry state, so a variable reassigned inside
// the body on one iteration was invisible to a narrowing check reading
// it on a later one - exactly the gap `check_block`'s own doc comment
// (above `Phase 15 item (1)`) traces as the reverted
// `UnreachableStatementRule`'s real root cause.
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

// Real corpus false positive (sebastian/diff's `Parser::parse`, found
// while corpus-verifying Phase 15's reinstated `UnreachableStatementRule`):
// a variable reassigned *only* on a path that ends in `continue` never
// fed its new value into the next convergence pass at all, since the
// pass-to-pass entry computation only looked at the body's own ordinary
// fall-through exit - which this shape never reaches. Convergence wrongly
// concluded "nothing changed" after a single pass and kept that pass's
// own diagnostics, computed against the stale, guaranteed-null entry -
// so `$diff !== null` narrowed to `Type::Never` on *every* pass, not just
// the first, and a read inside that guard looked unreachable.
#[test]
fn a_variable_reassigned_only_on_a_path_ending_in_continue_still_converges() {
    let d = diagnostics_for(
        r#"<?php
            class Diff {}
            function needsString(string $x): void {}
            function f(bool $cond): array {
                $diffs = [];
                $diff  = null;
                while ($cond) {
                    if ($cond) {
                        if ($diff !== null) {
                            needsString($diff);
                            $diffs[] = $diff;
                        }
                        $diff = new Diff();
                        continue;
                    }
                }
                return $diffs;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "deadCode.unreachable"),
        "expected the continue-carried reassignment to converge, got: {d:?}"
    );
}

// Phase 15 continuation: a second, distinct bug in the same
// `continue`-handling area as the test above, found via a real Piwigo
// corpus false positive once `BooleanNotConstantConditionRule` started
// exercising it (`Piwigo\Db\BatchWriter::updateRow`). The previous fix
// (`a_variable_reassigned_only_on_a_path_ending_in_continue_still_
// converges`) made `continue_states` feed the *next pass's own entry*
// correctly - but `converge_loop_body`'s own *return value* (the state
// the caller treats as "what's true after the loop") was still built
// from the kept pass's bare fallthrough-only `body_locals`, silently
// dropping that same pass's own continue-reached states. A variable
// only ever assigned on the non-`continue` path therefore looked
// unconditionally assigned after the loop, instead of "maybe assigned,
// maybe not."
#[test]
fn a_variable_only_assigned_on_a_non_continue_path_is_not_definite_after_the_loop() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $data): void {
                $flag = false;
                foreach ($data as $v) {
                    if ($v) {
                        continue;
                    }
                    $flag = true;
                }
                if (!$flag) {
                    echo "unset";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("booleanNot.")),
        "expected $flag to converge to a plain bool (maybe true, maybe \
         still its pre-loop false) rather than a definite constant, \
         got: {d:?}"
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

#[test]
fn a_loop_body_with_no_internal_reassignment_produces_no_diagnostic_churn() {
    // Regression guard: a loop body that never reassigns anything it
    // reads converges after its very first pass (`converge_loop_body`'s
    // own early-exit), so this must behave identically to the old
    // single-pass model - no diagnostic should fire from convergence
    // itself running the body more than once.
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): int {
                $total = 0;
                while ($n > 0) {
                    $total = $total + $n;
                    $n = $n - 1;
                }
                return $total;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_diagnostic_unrelated_to_convergence_is_reported_exactly_once_not_once_per_pass() {
    // `$x`'s own reassignment forces more than one convergence pass
    // (entry `null` != post-body `Foo`), so this body is checked at
    // least twice before a result is kept. An unrelated, unconditional
    // diagnostic inside the same body must still be reported exactly
    // once - proving every non-final pass's diagnostics are actually
    // discarded (`diagnostics.truncate`), not accumulated alongside the
    // kept pass's.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f() {
                $x = null;
                while (rand(0, 1)) {
                    totallyUndefinedFunction();
                    $x = new Foo();
                }
            }
        "#,
    );
    let matches: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "function.notFound")
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one function.notFound diagnostic, got: {d:?}"
    );
}

// Phase 15 item (1), `UnreachableStatementRule` (`deadCode.unreachable`):
// reinstated once Phase 55's fixed-point loop convergence removed the
// false-positive class that got it reverted in the first place - see
// `checker::check_block`'s own doc comment and ROADMAP.md.
#[test]
fn a_statement_after_an_unconditional_return_is_flagged_as_unreachable() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                return 1;
                echo "dead";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "deadCode.unreachable"),
        "got: {d:?}"
    );
}

#[test]
fn only_the_first_statement_after_a_terminator_is_flagged_not_every_one_after() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                return 1;
                echo "dead one";
                echo "dead two";
                echo "dead three";
            }
        "#,
    );
    let matches: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "deadCode.unreachable")
        .collect();
    assert_eq!(matches.len(), 1, "got: {d:?}");
}

// Real corpus false positive: `Amp\Http\Client\Connection\Internal\
// Http1Parser::parse()`'s own `return $response; body_identity: { ... }`,
// where `goto body_identity;` (elsewhere in the same function - a real,
// hand-written HTTP/1.1 parsing state machine) makes the labeled block
// a genuine jump target, not dead code, whatever the `return` right
// before it does.
#[test]
fn a_labeled_statement_reachable_via_an_earlier_goto_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond): int {
                if ($cond) {
                    goto skip;
                }
                return 1;
                skip:
                return 2;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "deadCode.unreachable"),
        "got: {d:?}"
    );
}

// A label nobody `goto`s at all is exactly as dead as any other
// statement after an unconditional terminator - this fix must not
// suppress the diagnostic unconditionally for every label.
#[test]
fn an_untargeted_label_after_a_return_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                return 1;
                never_reached:
                echo "dead";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "deadCode.unreachable"),
        "got: {d:?}"
    );
}

#[test]
fn a_statement_after_an_unconditional_throw_is_flagged_as_unreachable() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                throw new \Exception();
                echo "dead";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "deadCode.unreachable"),
        "got: {d:?}"
    );
}

#[test]
fn a_statement_after_a_conditionally_reachable_return_is_never_flagged() {
    // The `if` has no `else`, so control can still reach the `echo` -
    // only an *unconditional* terminator makes what follows unreachable.
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond): void {
                if ($cond) {
                    return;
                }
                echo "reachable";
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "deadCode.unreachable"),
        "got: {d:?}"
    );
}

#[test]
fn dead_code_inside_a_nested_block_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond): void {
                if ($cond) {
                    return;
                } else {
                    return;
                    echo "dead";
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "deadCode.unreachable"),
        "got: {d:?}"
    );
}

// Regression guard for a real bug this reinstatement surfaced and fixed:
// both `check_function`'s own falls-off-the-end check and
// `always_terminates`'s own `Block` arm only ever looked at a block's
// *last* statement to decide whether it always terminates - so a
// function ending in dead code after a real, unconditional `return`
// wrongly reported `return.missing` (the dead trailing statement, not
// the `return`, was the last one `always_terminates` ever looked at).
#[test]
fn dead_code_after_a_return_does_not_also_trigger_a_spurious_return_missing() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                return 1;
                echo "dead";
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

// Phase 61: reinstated empty-concrete-method-body completeness check,
// opt-in via `Registry::check_empty_method_bodies`.
#[test]
fn check_empty_method_bodies_flag_off_by_default_does_not_flag_empty_body() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                public function f(): int {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn check_empty_method_bodies_flags_a_concrete_method_with_an_empty_body_and_non_void_return_type() {
    let d = diagnostics_for_with(
        r#"<?php
            class C {
                public function f(): int {}
            }
        "#,
        |r| r.check_empty_method_bodies = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn check_empty_method_bodies_flags_never_typed_concrete_empty_body_as_return_never() {
    let d = diagnostics_for_with(
        r#"<?php
            class C {
                public function f(): never {}
            }
        "#,
        |r| r.check_empty_method_bodies = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.never"),
        "got: {d:?}"
    );
}

#[test]
fn check_empty_method_bodies_does_not_flag_an_abstract_method_declaration() {
    let d = diagnostics_for_with(
        r#"<?php
            abstract class C {
                abstract public function f(): int;
            }
        "#,
        |r| r.check_empty_method_bodies = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn check_empty_method_bodies_does_not_flag_an_interface_method_declaration() {
    let d = diagnostics_for_with(
        r#"<?php
            interface I {
                public function f(): int;
            }
        "#,
        |r| r.check_empty_method_bodies = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}

#[test]
fn check_empty_method_bodies_does_not_flag_a_void_or_nullable_return_type() {
    let d = diagnostics_for_with(
        r#"<?php
            class C {
                public function f(): void {}
                public function g(): ?int {}
            }
        "#,
        |r| r.check_empty_method_bodies = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "return.missing"),
        "got: {d:?}"
    );
}
