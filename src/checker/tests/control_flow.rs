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
// for, and real PHP/PHPStan never flag it.
#[test]
fn isset_and_empty_on_a_shape_s_missing_key_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{a: string} $x */
            function f(array $x): bool {
                return isset($x['b']) || empty($x['c']);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

// Phase 15 item (2): the constant-boolean-condition family, reinstated
// deliberately narrower than the original (reverted) design -
// `literal_truthiness` only ever trusts a *syntactic* literal, never the
// condition's own inferred type, sidestepping the two real narrowing
// gaps (loop staleness, `?? false` collapse) that got the type-
// inference-trusting version reverted. See `checker::stmt`'s own `If`/
// `While`/`DoWhile` arms for the real-PHPStan-grounded exclusions.

#[test]
fn an_if_with_a_literal_true_condition_is_flagged() {
    let d = diagnostics_for("<?php function f() { if (true) { echo 1; } }");
    assert!(
        d.iter().any(|x| x.identifier == "if.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn an_if_with_a_literal_false_condition_is_flagged() {
    let d = diagnostics_for("<?php function f() { if (false) { echo 1; } }");
    assert!(
        d.iter().any(|x| x.identifier == "if.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn an_if_with_a_real_variable_condition_is_not_flagged() {
    // Regression guard that this stays syntax-only, not a return to
    // trusting the condition's own inferred type.
    let d = diagnostics_for("<?php function f(bool $x) { if ($x) { echo 1; } }");
    assert!(
        !d.iter()
            .any(|x| x.identifier == "if.alwaysTrue" || x.identifier == "if.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_grouped_literal_still_resolves_as_if_condition() {
    // A `!`-negated condition (`if (!false)`) is deliberately *not*
    // caught here anymore, matching real PHPStan's own `ConstantCondition
    // RuleHelper::shouldSkip` exactly (confirmed against `vendor/
    // phpstan-src`): a bare `!$expr` is always excluded from the generic
    // if/while/ternary check, deferred entirely to its own dedicated
    // standalone rule (`BooleanNotConstantConditionRule` - see the
    // `booleanNot.always*` tests below for that rule's own coverage of
    // this exact case).
    let d = diagnostics_for("<?php function f() { if ((true)) { echo 1; } }");
    assert!(
        d.iter().any(|x| x.identifier == "if.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_while_with_a_literal_false_condition_is_flagged() {
    let d = diagnostics_for("<?php function f() { while (false) { echo 1; } }");
    assert!(
        d.iter().any(|x| x.identifier == "while.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_breakless_while_true_is_flagged() {
    let d = diagnostics_for("<?php function f() { while (true) { echo 1; } }");
    assert!(
        d.iter().any(|x| x.identifier == "while.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_while_true_with_a_break_is_not_flagged() {
    let d = diagnostics_for("<?php function f() { while (true) { break; } }");
    assert!(
        !d.iter().any(|x| x.identifier == "while.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_yielding_while_true_is_not_flagged() {
    let d = diagnostics_for("<?php function f() { while (true) { yield 1; } }");
    assert!(
        !d.iter().any(|x| x.identifier == "while.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_while_true_in_a_never_returning_function_is_not_flagged() {
    let d = diagnostics_for("<?php function f(): never { while (true) { echo 1; } }");
    assert!(
        !d.iter().any(|x| x.identifier == "while.alwaysTrue"),
        "got: {d:?}"
    );
}

// Re-examining Phase 15's own disclosed Pending entry ("breakless-
// detection reuses stmt_contains_break's own 'any break anywhere'
// conservatism... a narrow, disclosed simplification") against real
// PHPStan's own actual source (`WhileLoopAlwaysTrueConditionRule`'s
// `BreaklessWhileLoopNode::getExitPoints()`) found this understated the
// real gap: a `continue` whose own level escapes past this loop also
// disqualifies, not just a literal `break` - confirmed via a real, live
// corpus false positive, `PhpBench\Path\Path::getLongestCommonBasePath()`'s
// own `while (true) { ...; continue 2; }` (vendored identically four
// times across the corpus).
#[test]
fn a_while_true_with_an_escaping_continue_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $items): void {
                foreach ($items as $item) {
                    while (true) {
                        if ($item) {
                            continue 2;
                        }
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "while.alwaysTrue"),
        "got: {d:?}"
    );
}

// A bare `continue;` (or `continue 1;`) only restarts *this* loop's own
// next iteration - real PHPStan's own rule doesn't treat that as
// disqualifying either (only `num > 1` does), so this must still flag.
#[test]
fn a_while_true_with_a_same_level_continue_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                while (true) {
                    if (rand(0, 1)) {
                        continue;
                    }
                    echo 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "while.alwaysTrue"),
        "got: {d:?}"
    );
}

// Same real gap, the other disqualifying shape real PHPStan's own
// `getExitPoints()` also covers: any `return`/`throw` reachable inside
// the loop body is a real exit point too, not just `break`/`continue`.
// Real, live corpus false positive: `Amp\Http\Server\Driver\Http1Driver::
// handleClient()`'s own `while (true) { ...; return; ... }`.
#[test]
fn a_while_true_with_a_return_inside_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                while (true) {
                    if (rand(0, 1)) {
                        return;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "while.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_do_while_true_with_a_return_inside_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                do {
                    if (rand(0, 1)) {
                        return;
                    }
                } while (true);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "doWhile.alwaysTrue"),
        "got: {d:?}"
    );
}

// Phase 15 continuation: real bug found and fixed this session - a
// `while` condition's own constant-condition check (and any standalone
// check reached while checking it, like `BooleanNot`) used to run
// exactly once against the pre-loop entry state, entirely bypassing
// Phase 55's own fixed-point loop-body convergence. Real corpus false
// positive this produced: `Piwigo\Admin\Extensions\CoreUpdateService::
// upgradeTo`'s `$end = false; while (!$end) { ...; $end = true; ... }` -
// `$end`'s pre-loop type is exactly `false`, so `!$end` looked
// unconditionally `true` even though the body can reassign it. Moving
// the condition check inside `converge_loop_body`'s own closure (so it
// runs against each pass's own converging state, exactly matching real
// PHPStan's own `WhileHandler::processStmt`) fixes this.
#[test]
fn a_while_condition_reassigned_inside_the_body_is_not_a_false_constant() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $end = false;
                while (!$end) {
                    if (rand(0, 1)) {
                        $end = true;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier.starts_with("booleanNot.")
                || x.identifier.starts_with("while.always")),
        "expected $end's loop-carried reassignment to be visible to the \
         while condition's own constant-condition check, got: {d:?}"
    );
}

// Same real bug, same fix, `For`'s own last condition expression instead
// of `While`'s bare condition - real corpus false positive:
// `Piwigo\Search\QMultiToken::parseExpression`'s `for ($stop = false; !
// $stop && ...; $qi++)`, where `$stop` is reassigned inside the body.
#[test]
fn a_for_loop_condition_reassigned_inside_the_body_is_not_a_false_constant() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): void {
                for ($stop = false, $i = 0; !$stop && $i < $n; $i++) {
                    if ($i === 5) {
                        $stop = true;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("booleanNot.")),
        "expected $stop's loop-carried reassignment to be visible to the \
         for loop's own last condition expression, got: {d:?}"
    );
}

// Real regression found and fixed this session, caused by the two fixes
// directly above: moving the condition check into `converge_loop_body`'s
// own closure means it only ever mutates per-pass *copies* of the entry,
// never the real `locals` directly - so a variable *the condition
// itself* assigns (`while (($x = f()) !== null) {...}`, a real, common
// idiom) no longer survived the "loop may run zero times" merge
// afterward (`merge_scopes` drops anything present on only one side, and
// the real pre-loop `locals` never got the binding at all anymore).
// Real corpus false positive: `Symfony\Component\DependencyInjection\
// Compiler\AbstractRecursivePass::getConstructor`'s `while ((null ===
// $class = $definition->getClass()) && $definition instanceof
// ChildDefinition)`.
#[test]
fn a_variable_assigned_by_the_while_condition_itself_is_defined_after_the_loop() {
    let d = diagnostics_for(
        r#"<?php
            class Definition {
                public function getClass(): ?string { return null; }
                public function isChild(): bool { return false; }
            }
            function f(Definition $definition): void {
                while ((null === $class = $definition->getClass()) && $definition->isChild()) {
                    echo "loop";
                }
                echo $class;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "variable.undefined"),
        "expected $class to be defined after the loop (the condition \
         always runs at least once), got: {d:?}"
    );
}

#[test]
fn a_breakless_do_while_false_is_flagged() {
    let d = diagnostics_for("<?php function f() { do { echo 1; } while (false); }");
    assert!(
        d.iter().any(|x| x.identifier == "doWhile.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_do_while_false_with_a_break_is_not_flagged() {
    // The real, common "structured goto" idiom - not dead code.
    let d = diagnostics_for(
        r#"<?php
            function f(bool $x) {
                do {
                    if ($x) {
                        break;
                    }
                    echo 1;
                } while (false);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "doWhile.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_breakless_do_while_true_is_flagged() {
    let d = diagnostics_for("<?php function f() { do { echo 1; } while (true); }");
    assert!(
        d.iter().any(|x| x.identifier == "doWhile.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_do_while_true_with_a_break_is_not_flagged() {
    let d = diagnostics_for("<?php function f() { do { break; } while (true); }");
    assert!(
        !d.iter().any(|x| x.identifier == "doWhile.alwaysTrue"),
        "got: {d:?}"
    );
}

// Phase 15 continuation: reinstating the *complete* real PHPStan
// constant-boolean-condition family with real inferred-type trust
// (`inferred_constant_condition`), not just bare syntactic literals -
// `elseif`'s own identifier/exemption, `&&`/`||`/`xor`'s own left/right/
// whole-result diagnostics, and the standalone `!$expr` rule.

#[test]
fn a_ternary_with_a_literal_true_condition_is_flagged() {
    let d = diagnostics_for("<?php function f() { true ? 1 : 2; }");
    assert!(
        d.iter().any(|x| x.identifier == "ternary.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_ternary_with_a_real_variable_condition_is_not_flagged() {
    let d = diagnostics_for("<?php function f(bool $x) { $x ? 1 : 2; }");
    assert!(
        !d.iter()
            .any(|x| x.identifier == "ternary.alwaysTrue" || x.identifier == "ternary.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn an_elseif_with_a_literal_false_condition_is_flagged_with_its_own_identifier() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $x) {
                if ($x) {
                    echo 1;
                } elseif (false) {
                    echo 2;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "elseif.alwaysFalse"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "if.alwaysFalse"),
        "an elseif must not also report the generic if.* identifier: {d:?}"
    );
}

#[test]
fn a_last_elseif_with_no_trailing_else_and_a_literal_true_condition_is_exempt() {
    // Matches real PHPStan's own `reportAlwaysTrueInLastCondition: false`
    // default - a trailing `elseif (true)` with nothing after it is a
    // real, common "acts as an implicit else" idiom.
    let d = diagnostics_for(
        r#"<?php
            function f(bool $x) {
                if ($x) {
                    echo 1;
                } elseif (true) {
                    echo 2;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "elseif.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_last_elseif_with_a_trailing_else_and_a_literal_true_condition_is_not_exempt() {
    // The exemption only applies when there's genuinely nothing after
    // this elseif - once a trailing `else` exists, `elseif (true)` really
    // is dead code (the `else` branch never runs).
    let d = diagnostics_for(
        r#"<?php
            function f(bool $x) {
                if ($x) {
                    echo 1;
                } elseif (true) {
                    echo 2;
                } else {
                    echo 3;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "elseif.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_last_elseif_with_a_literal_true_condition_is_not_exempt() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $x) {
                if ($x) {
                    echo 1;
                } elseif (true) {
                    echo 2;
                } elseif ($x) {
                    echo 3;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "elseif.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_standalone_boolean_and_with_a_literal_false_left_side_is_flagged() {
    // Not the direct condition of an if/while/ternary - a plain
    // expression statement - so `is_first_level` is false and the
    // right-side/whole-result checks also apply.
    let d = diagnostics_for("<?php function f(bool $x) { $y = false && $x; }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanAnd.leftAlwaysFalse"),
        "got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanAnd.alwaysFalse"),
        "a literal-false left side makes the whole && result always false too: {d:?}"
    );
}

#[test]
fn a_standalone_boolean_and_with_a_literal_true_right_side_is_flagged() {
    let d = diagnostics_for("<?php function f(bool $x) { $y = $x && true; }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanAnd.rightAlwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_standalone_boolean_or_with_a_literal_true_left_side_is_flagged() {
    let d = diagnostics_for("<?php function f(bool $x) { $y = true || $x; }");
    assert!(
        d.iter().any(|x| x.identifier == "booleanOr.leftAlwaysTrue"),
        "got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.identifier == "booleanOr.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_boolean_and_used_directly_as_an_if_condition_does_not_double_report_the_right_side() {
    // `is_first_level` (`Checker::in_condition_position`) should suppress
    // the right-side/whole-result checks here - the `if` construct itself
    // already reports the whole condition being always-true via
    // `if.alwaysTrue`.
    let d = diagnostics_for("<?php function f(bool $x) { if ($x && true) { echo 1; } }");
    assert!(
        !d.iter()
            .any(|x| x.identifier == "booleanAnd.rightAlwaysTrue"),
        "got: {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanAnd.alwaysTrue"),
        "got: {d:?}"
    );
}

// Real bug found and fixed this session: `is_first_level` only ever
// considered `in_condition_position` (an if/while/ternary's own direct
// condition), missing real PHPStan's other real
// `isInFirstLevelStatement()` case - a bare `||`/`&&` expression used
// directly as its own statement (`ExpressionContext::createTopLevel()`,
// confirmed against `ExpressionHandler::processStmt`). Without also
// checking `in_bare_statement_position` here, the whole-result check ran
// unconditionally on a bare `$a or sideEffect();` statement. Real corpus
// false positive: `Piwigo\Url\UrlService::makeSectionInUrl`'s
// `isset($x) or trigger_error(...);` (`trigger_error()`'s own stub
// return type is exactly `true`).
#[test]
fn a_boolean_or_used_as_its_own_bare_statement_does_not_report_the_whole_result() {
    let d = diagnostics_for(
        r#"<?php
            function alwaysTrue(): true { return true; }
            function f(bool $x) {
                $x or alwaysTrue();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanOr.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_boolean_and_used_directly_as_an_if_condition_still_reports_its_left_side() {
    // The left-side check is unconditional, unlike right-side/whole-result.
    let d = diagnostics_for("<?php function f(bool $x) { if (true && $x) { echo 1; } }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanAnd.leftAlwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_real_variable_on_both_sides_of_boolean_and_is_not_flagged() {
    let d = diagnostics_for("<?php function f(bool $x, bool $y) { $z = $x && $y; }");
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("booleanAnd.")),
        "got: {d:?}"
    );
}

// Real bug found and fixed this session: `check_boolean_and_or`'s left-
// side check used to run against the *already left-narrowed* scope (the
// same one the right-side check correctly wants), so a bare-variable
// left operand looked constantly true purely by circularity - it had
// just been narrowed truthy on the assumption it held, then that exact
// narrowed value was checked as if it were the operand's own real type.
// Real corpus false positive: `Piwigo\Controller\Admin\
// ConfigurationSubController::processSizes`'s `if ($same and ... )`
// where `$same` is reassigned earlier in the same block.
#[test]
fn a_reassignable_variable_narrowed_by_its_own_use_as_the_left_operand_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond) {
                $same = true;
                if ($cond) {
                    $same = false;
                }
                if ($same && true) {
                    echo 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "booleanAnd.leftAlwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_logical_xor_with_a_literal_operand_on_each_side_is_flagged_independently() {
    let d = diagnostics_for("<?php function f(bool $x) { $y = true xor $x; }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "logicalXor.leftAlwaysTrue"),
        "got: {d:?}"
    );
    // No whole-result diagnostic exists for xor at all.
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("logicalXor.")
            && x.identifier != "logicalXor.leftAlwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_logical_xor_with_literal_operands_on_both_sides_reports_both() {
    let d = diagnostics_for("<?php function f() { $y = true xor false; }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "logicalXor.leftAlwaysTrue"),
        "got: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "logicalXor.rightAlwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_logical_xor_with_a_real_variable_on_both_sides_is_not_flagged() {
    let d = diagnostics_for("<?php function f(bool $x, bool $y) { $z = $x xor $y; }");
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("logicalXor.")),
        "got: {d:?}"
    );
}

#[test]
fn a_negated_literal_false_is_flagged_as_always_true_anywhere_not_just_as_a_condition() {
    // The standalone `BooleanNot` rule fires everywhere `!expr` appears,
    // not just as a condition - unlike the generic if/while/ternary
    // check, which deliberately excludes `!` entirely (see
    // `a_grouped_literal_still_resolves_as_if_condition` above).
    let d = diagnostics_for("<?php function f() { $x = !false; }");
    assert!(
        d.iter().any(|x| x.identifier == "booleanNot.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_negated_literal_true_used_as_an_if_condition_is_flagged_as_always_false() {
    let d = diagnostics_for("<?php function f() { if (!true) { echo 1; } }");
    assert!(
        d.iter().any(|x| x.identifier == "booleanNot.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_negated_real_variable_is_not_flagged() {
    let d = diagnostics_for("<?php function f(bool $x) { $y = !$x; }");
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("booleanNot.")),
        "got: {d:?}"
    );
}

#[test]
fn a_negated_literal_true_as_a_last_elseif_without_else_is_exempt() {
    // `BooleanNotConstantConditionRule` has its own copy of the same
    // last-condition exemption `elseif` itself gets - `!true` (always
    // false) negated becomes always-true, so this mirrors the `elseif
    // (true)`-is-exempt case above, just reached via `!`.
    let d = diagnostics_for(
        r#"<?php
            function f(bool $x) {
                if ($x) {
                    echo 1;
                } elseif (!false) {
                    echo 2;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanNot.alwaysTrue"),
        "got: {d:?}"
    );
}

// Real corpus false positive found while corpus-verifying the family
// above: `strpos()`'s own `$offset` parameter (typed `int<0, max>`)
// collapsed to bare `Int` the moment it was reassigned via `$offset =
// $pos + 1;` inside a loop, surfacing only once the `While`-condition
// convergence fix made `strpos()`'s own later, real re-check of
// `$offset` visible at all (`Tools\p58\assign.php`). `infer_binary`'s
// `Add`/`Sub` arms gained real, narrow `int<min,max>` arithmetic -
// confirmed directly against `vendor/phpstan-src`'s own
// `InitializerExprTypeResolver::integerRangeMath` (deliberately not the
// *full* range-arithmetic surface: `Mul`/`Div`'s own negative/positive-
// interval splitting isn't replicated, and `Sub` only handles a constant
// right operand, not the full range-minus-range case - no corpus
// evidence needs either, and both are meaningfully more involved to
// transcribe correctly).
#[test]
fn adding_a_literal_to_a_bounded_int_range_shifts_both_ends() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, 10> $x */
            function f(int $x): void {
                assertType('int<1, 11>', $x + 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_a_literal_to_an_upper_unbounded_int_range_keeps_it_unbounded() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, max> $x */
            function f(int $x): void {
                assertType('int<1, max>', $x + 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_a_literal_from_a_bounded_int_range_shifts_both_ends() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, 10> $x */
            function f(int $x): void {
                assertType('int<-1, 9>', $x - 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_a_literal_from_an_upper_unbounded_int_range_keeps_it_unbounded() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int<0, max> $x */
            function f(int $x): void {
                assertType('int<-1, max>', $x - 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn adding_two_int_ranges_with_a_shared_unbounded_end_stays_unbounded() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<0, 10> $x
             * @param int<0, max> $y
             */
            function f(int $x, int $y): void {
                assertType('int<0, max>', $x + $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn subtracting_a_non_constant_int_range_falls_back_to_plain_int() {
    // The narrower, undocumented-as-general `Sub` case: a genuinely
    // ranged (not single-valued) right operand isn't handled, matching
    // this fix's own disclosed scope.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param int<0, 10> $x
             * @param int<0, 5> $y
             */
            function f(int $x, int $y): void {
                assertType('int', $x - $y);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
