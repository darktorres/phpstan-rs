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
    assert!(d[0].message.contains("got null"), "got: {d:?}");
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
