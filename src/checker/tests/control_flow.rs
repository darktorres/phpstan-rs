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
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

// Phase 15 item (1), UnreachableStatementRule (`deadCode.unreachable`):
// investigated and declined - see `checker::check_block`'s own doc
// comment and ROADMAP.md for why.
