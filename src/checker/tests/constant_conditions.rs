//! `if`/`while`/ternary/`elseif`/boolean-and-or-xor constant-condition detection.

use super::helpers::*;

// The constant-boolean-condition family, deliberately scoped narrower
// than trusting the condition's own inferred type: `literal_truthiness`
// only ever trusts a syntactic literal, sidestepping two real narrowing
// gaps (loop staleness, `?? false` collapse) that a type-inference-
// trusting version would hit. See `checker::stmt`'s own `If`/`While`/
// `DoWhile` arms for the real-PHPStan-grounded exclusions.

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

// A bare function/method/static call condition - real PHPStan's own
// `FunctionCallConstantConditionRule`/`*Helper` isn't a separate rule
// at all, just this exact same if/while/elseif/ternary/&&/|| family
// deferring their own reporting when their own condition happens to be
// a call - no Collector, no new diagnostic identifier.
#[test]
fn an_if_with_a_bare_call_condition_whose_declared_return_type_is_a_literal_true_is_flagged() {
    let d = diagnostics_for(
        "<?php
        /** @return true */
        function alwaysTrue(): bool { return true; }
        function f(): void { if (alwaysTrue()) { echo 1; } }
        ",
    );
    assert!(
        d.iter().any(|x| x.identifier == "if.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn an_if_with_a_bare_method_call_condition_whose_declared_return_type_is_a_literal_false_is_flagged()
 {
    let d = diagnostics_for(
        "<?php
        final class Foo {
            /** @return false */
            public function neverTrue(): bool { return false; }
        }
        function f(Foo $foo): void { if ($foo->neverTrue()) { echo 1; } }
        ",
    );
    assert!(
        d.iter().any(|x| x.identifier == "if.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn an_if_with_a_bare_call_condition_of_an_ordinary_bool_return_type_is_not_flagged() {
    // Regression guard: the overwhelming majority of real bool-returning
    // calls must stay silent - only a declaration whose own return type
    // is *literally* `true`/`false` (not just `bool`) is constant.
    let d = diagnostics_for(
        "<?php
        function maybe(): bool { return random_int(0, 1) === 1; }
        function f(): void { if (maybe()) { echo 1; } }
        ",
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "if.alwaysTrue" || x.identifier == "if.alwaysFalse"),
        "unexpected diagnostics: {d:?}"
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

// `NumberComparisonOperatorsConstantConditionRule` - a `</>/<=/>=`
// comparison whose own outcome is provable from both operands' known
// int ranges joins the same constant-condition family, with zero new
// checker code (`infer_binary`'s own comparison arm returns a literal
// `Type::True`/`Type::False` when provable).

#[test]
fn an_if_with_a_provably_always_false_less_than_comparison_is_flagged() {
    let d = diagnostics_for(
        "<?php
        /** @param int<20, 30> $x */
        function f(int $x): void { if ($x < 10) { echo 1; } }
        ",
    );
    assert!(
        d.iter().any(|x| x.identifier == "smaller.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn an_if_with_a_provably_always_true_greater_than_or_equal_comparison_is_flagged() {
    let d = diagnostics_for(
        "<?php
        /** @param int<20, 30> $x */
        function f(int $x): void { if ($x >= 10) { echo 1; } }
        ",
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "greaterOrEqual.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn an_if_with_an_ordinary_unbounded_int_comparison_is_never_flagged() {
    let d = diagnostics_for(
        "<?php
        function f(int $x): void { if ($x < 10) { echo 1; } }
        ",
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "if.alwaysTrue" || x.identifier == "if.alwaysFalse"),
        "unexpected diagnostics: {d:?}"
    );
}

// Unary `-` on an int literal must actually negate its own value, not
// leave it unchanged (`-32768` inferred as `IntLiteral(32768)`, dropping
// the sign entirely) - otherwise narrowing `$id` by the false branch of
// `$id < -32768` produces `int<32768, max>` instead of
// `int<-32768, max>`, making the second half of
// `$id < -32768 || $id > 32767` look "always true" against the real
// upper bound.
#[test]
fn a_negative_int_literal_narrows_with_the_correct_sign() {
    let d = diagnostics_for(
        "<?php
        function f(int $id): void {
            if ($id < -32768 || $id > 32767) {
                return;
            }
        }
        ",
    );
    assert!(
        !d.iter().any(|x| x.identifier == "greater.alwaysTrue"),
        "unexpected diagnostics: {d:?}"
    );
}

// `++$x`/`--$x` on an `IntLiteral`/`IntRange`-typed operand must not
// return the operand's own type completely unchanged - otherwise `$n =
// 0; foreach (...) { ...; $n++; } if ($n > 0)` (an extremely common
// accumulator idiom) looks "always false", since `$n` never advances
// past `IntLiteral(0)` no matter how many times `$n++` runs.
#[test]
fn a_counter_incremented_inside_a_loop_and_checked_after_is_never_flagged() {
    let d = diagnostics_for(
        "<?php
        function f(array $items): void {
            $n = 0;
            foreach ($items as $item) {
                if ($item) {
                    $n++;
                }
            }
            if ($n > 0) {
                echo 1;
            }
        }
        ",
    );
    assert!(
        !d.iter().any(|x| x.identifier == "greater.alwaysFalse"),
        "unexpected diagnostics: {d:?}"
    );
}

// A do-while's own trailing condition, checked once more right after
// the loop exits, is itself still consulting a loop-carried variable's
// post-convergence (bounded-pass-approximated) type - `do { ...;
// $attempts++; } while ($attempts < 150);` must not look "always true"
// just because this project's own bounded-pass convergence never
// actually reaches anywhere near 150, even though real semantics
// genuinely allow it. `loop_depth`'s own guard on
// `check_constant_numeric_comparison` needs to still see this one
// trailing check as "inside the loop" for exactly this reason.
#[test]
fn a_do_while_s_own_trailing_condition_against_a_loop_carried_counter_is_never_flagged() {
    let d = diagnostics_for(
        "<?php
        function f(): void {
            $attempts = 0;
            do {
                $attempts++;
            } while ($attempts < 150);
        }
        ",
    );
    assert!(
        !d.iter().any(|x| x.identifier == "smaller.alwaysTrue"),
        "unexpected diagnostics: {d:?}"
    );
}

// The vendored `phpstorm-stubs`' own `define('PHP_VERSION_ID', 50306)`
// (a placeholder baked in from whatever PHP version generated that
// stub snapshot) must not be trusted as `PHP_VERSION_ID`'s own literal
// type everywhere, or every real `PHP_VERSION_ID >= 80300`-style
// portability check throughout the entire vendor ecosystem looks
// unconditionally, wrongly false, the moment anything consumes the
// literal value for a "provably constant" purpose.
// `checker::expr::Checker::global_constant_type` derives this from the
// configured `php_version` instead (an `IntRange` spanning every
// possible patch digit) - or a fully unbounded `Type::Int` with none
// configured, never a specific literal.
#[test]
fn php_version_id_reflects_the_configured_php_version_not_the_stub_s_own_placeholder() {
    let d = diagnostics_for_with(
        "<?php
        function f(): void {
            if (PHP_VERSION_ID >= 80300) {
                echo 1;
            }
        }
        ",
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "greaterOrEqual.alwaysFalse"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn php_version_id_with_no_configured_php_version_is_never_flagged() {
    let d = diagnostics_for(
        "<?php
        function f(): void {
            if (PHP_VERSION_ID >= 80300) {
                echo 1;
            }
        }
        ",
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "greaterOrEqual.alwaysFalse"
                || x.identifier == "greaterOrEqual.alwaysTrue"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_comparison_whose_ranges_genuinely_overlap_is_never_flagged() {
    let d = diagnostics_for(
        "<?php
        /** @param int<0, 30> $x */
        function f(int $x): void { if ($x < 10) { echo 1; } }
        ",
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "if.alwaysTrue" || x.identifier == "if.alwaysFalse"),
        "unexpected diagnostics: {d:?}"
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

// A `continue` whose own level escapes past this loop also disqualifies
// breakless-loop detection, not just a literal `break` - matching real
// PHPStan's own `WhileLoopAlwaysTrueConditionRule`'s
// `BreaklessWhileLoopNode::getExitPoints()`.
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

// The other disqualifying shape real PHPStan's own `getExitPoints()`
// also covers: any `return`/`throw` reachable inside the loop body is a
// real exit point too, not just `break`/`continue`.
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

// A `while` condition's own constant-condition check (and any
// standalone check reached while checking it, like `BooleanNot`) must
// not run exactly once against the pre-loop entry state, bypassing
// fixed-point loop-body convergence, or `$end = false; while (!$end) {
// ...; $end = true; ... }` looks unconditionally `true` (since `$end`'s
// pre-loop type is exactly `false`) even though the body can reassign
// it. The condition check runs inside `converge_loop_body`'s own
// closure, against each pass's own converging state, matching real
// PHPStan's own `WhileHandler::processStmt`.
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

// Same requirement, `For`'s own last condition expression instead of
// `While`'s bare condition - a `$stop` reassigned inside the body must
// not make `for ($stop = false; !$stop && ...; $qi++)` look constant.
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

// The non-last condition slot needs the identical fixed-point-
// convergence handling the test just above has for the last slot -
// checking it once, up front against the pre-loop entry, would see
// `$stop`'s own pre-loop literal `false` and never the body's own
// later reassignment, wrongly flagging the bare `!$stop` (itself its
// own standalone check, fires anywhere, not just as a whole condition)
// as always `true`.
#[test]
fn a_for_loop_non_last_condition_reassigned_inside_the_body_is_not_a_false_constant() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): void {
                for ($stop = false, $i = 0; !$stop, $i < $n; $i++) {
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
         for loop's own non-last condition expression, got: {d:?}"
    );
}

// A caveat of moving the condition check into `converge_loop_body`'s
// own closure: it only ever mutates per-pass copies of the entry, never
// the real `locals` directly - so a variable the condition itself
// assigns (`while (($x = f()) !== null) {...}`, a common idiom) must
// still survive the "loop may run zero times" merge afterward
// (`merge_scopes` drops anything present on only one side, so the real
// pre-loop `locals` needs the binding too).
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

// The `and`/`or` word-spelling identifier fidelity - real PHPStan's own
// `BooleanAndConstantConditionRule`/`BooleanOrConstantConditionRule`
// report a `logicalAnd.*`/`logicalOr.*` identifier (and an "and"/"or"
// message) for the word spelling, distinct from `booleanAnd.*`/
// `booleanOr.*` for `&&`/`||` - this project's own `BinOp::LogicalAnd`/
// `LogicalOr` (`src/ast.rs`) exist for exactly this distinction.

#[test]
fn a_standalone_logical_and_with_a_literal_false_left_side_is_flagged_under_the_word_identifier() {
    // Deliberately a bare statement, not `$y = false and $x;` - `and`
    // binds looser than `=`, so that would parse as `($y = false) and
    // $x;`, an unrelated precedence gotcha this test isn't after.
    let d = diagnostics_for("<?php function f(bool $x) { false and $x; }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "logicalAnd.leftAlwaysFalse"),
        "got: {d:?}"
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "booleanAnd.leftAlwaysFalse"),
        "word spelling must not also report under the symbol identifier: {d:?}"
    );
}

#[test]
fn a_standalone_logical_or_with_a_literal_true_left_side_is_flagged_under_the_word_identifier() {
    // Deliberately a bare statement - see the `and` test just above for
    // why an assignment on the left isn't used here either.
    let d = diagnostics_for("<?php function f(bool $x) { true or $x; }");
    assert!(
        d.iter().any(|x| x.identifier == "logicalOr.leftAlwaysTrue"),
        "got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.message.contains("or")),
        "the rendered message should use the \"or\" sigil, not \"||\": {d:?}"
    );
    assert!(
        !d.iter().any(|x| x.identifier == "booleanOr.leftAlwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_symbol_and_condition_still_reports_under_the_boolean_identifier() {
    // Regression guard: adding the word-spelling split must not flip the
    // pre-existing `&&`/`||` behavior over to the new identifiers.
    let d = diagnostics_for("<?php function f(bool $x) { $y = false && $x; }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "booleanAnd.leftAlwaysFalse"),
        "got: {d:?}"
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "logicalAnd.leftAlwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_symbol_and_binary_op_result_unused_reports_under_the_boolean_identifier() {
    let d = diagnostics_for("<?php function f(bool $x, bool $y) { $x && $y; }");
    assert!(
        d.iter().any(|x| x.identifier == "booleanAnd.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_word_and_binary_op_result_unused_reports_under_the_logical_identifier() {
    let d = diagnostics_for("<?php function f(bool $x, bool $y) { $x and $y; }");
    assert!(
        d.iter().any(|x| x.identifier == "logicalAnd.resultUnused"),
        "got: {d:?}"
    );
}

#[test]
fn a_word_or_chain_still_narrows_both_disjuncts_to_non_null_afterward() {
    // The word spelling must keep every narrowing/soundness-critical
    // consumer (`apply_narrowing`'s own `Or`-branch dispatch) working
    // identically to `||` - only the diagnostic identifier/message
    // should differ.
    let d = diagnostics_for(
        r#"<?php
            function f(?string $a, ?string $b): void {
                if ($a === null or $b === null) {
                    return;
                }
                echo strlen($a) + strlen($b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_word_and_chain_still_short_circuit_narrows_the_right_operand() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $a): void {
                if ($a !== null and strlen($a) > 0) {
                    echo 1;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

// `is_first_level` must also treat a bare `||`/`&&` used directly as its
// own statement (`$a or sideEffect();`) as first-level, not only an
// if/while/ternary's own direct condition - otherwise the whole-result
// check fires unconditionally on such statements.
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

// The left-side check must run against the scope *before* left-side
// narrowing, unlike the right-side check - otherwise a bare-variable left
// operand looks constantly true purely by circularity, since it was just
// narrowed truthy on the assumption it held.
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
    let d = diagnostics_for("<?php function f(bool $x) { $y = (true xor $x); }");
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
    let d = diagnostics_for("<?php function f(bool $x, bool $y) { $z = ($x xor $y); }");
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

// A trait method's own call-conditioned constant-truth finding that
// genuinely disagrees between two different hosts (one host's own
// `self::FLAG` makes it provably true, another's makes it provably
// false) is real PHPStan's own `ConstantConditionInTraitRule` shape -
// fully suppressed, not reported for either verdict (`check_trait_
// methods`'s own doc comment, `function_decl.rs`).

#[test]
fn a_trait_condition_that_diverges_across_two_hosts_is_suppressed_entirely() {
    let d = diagnostics_for(
        r#"<?php
            trait Common {
                public function check(): void {
                    if (self::FLAG) {
                        echo 1;
                    }
                }
            }
            class HostTrue {
                use Common;
                const FLAG = true;
            }
            class HostFalse {
                use Common;
                const FLAG = false;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "if.alwaysTrue" || x.identifier == "if.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_trait_condition_that_agrees_across_every_host_is_still_flagged() {
    // Negative control: the new divergence-suppression must not silently
    // eat a genuinely host-independent finding just because there's more
    // than one host - matches the pre-existing exact-duplicate `dedup_by`
    // collapsing this down to one diagnostic, not zero.
    let d = diagnostics_for(
        r#"<?php
            trait Common {
                public function check(): void {
                    if (self::FLAG) {
                        echo 1;
                    }
                }
            }
            class HostA {
                use Common;
                const FLAG = true;
            }
            class HostB {
                use Common;
                const FLAG = true;
            }
        "#,
    );
    assert_eq!(
        d.iter().filter(|x| x.identifier == "if.alwaysTrue").count(),
        1,
        "got: {d:?}"
    );
}

#[test]
fn a_trait_condition_constant_for_only_some_hosts_is_suppressed_entirely() {
    let d = diagnostics_for(
        r#"<?php
            trait Common {
                public function check(): void {
                    if (self::FLAG) {
                        echo 1;
                    }
                }
            }
            class HostConstant {
                use Common;
                const FLAG = true;
            }
            class HostUndecided {
                use Common;
                /** @var bool */
                const FLAG = PHP_INT_SIZE > 4;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "if.alwaysTrue"),
        "got: {d:?}"
    );
}

// `function.impossibleType`/`function.alreadyNarrowedType` - real
// PHPStan's own `Rules/Comparison/ImpossibleCheckTypeFunctionCallRule.php`,
// scoped to the `is_string`/`is_int`/... predicate table this project's
// own narrowing engine already trusts for these same functions.

#[test]
fn is_string_on_a_known_int_is_flagged_impossible() {
    let d = diagnostics_for("<?php function f(int $x) { if (is_string($x)) { echo 1; } }");
    assert!(
        d.iter().any(|x| x.identifier == "function.impossibleType"),
        "got: {d:?}"
    );
}

#[test]
fn is_int_on_a_known_int_is_flagged_already_narrowed() {
    let d = diagnostics_for("<?php function f(int $x) { if (is_int($x)) { echo 1; } }");
    assert!(
        d.iter()
            .any(|x| x.identifier == "function.alreadyNarrowedType"),
        "got: {d:?}"
    );
}

#[test]
fn is_string_on_a_mixed_argument_is_never_flagged() {
    let d = diagnostics_for("<?php function f(mixed $x) { if (is_string($x)) { echo 1; } }");
    assert!(
        !d.iter().any(|x| x.identifier == "function.impossibleType"
            || x.identifier == "function.alreadyNarrowedType"),
        "got: {d:?}"
    );
}

#[test]
fn is_string_on_a_union_that_could_go_either_way_is_never_flagged() {
    let d = diagnostics_for("<?php function f(int|string $x) { if (is_string($x)) { echo 1; } }");
    assert!(
        !d.iter().any(|x| x.identifier == "function.impossibleType"
            || x.identifier == "function.alreadyNarrowedType"),
        "got: {d:?}"
    );
}

#[test]
fn is_object_on_an_enum_subject_is_flagged_already_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            enum Status { case A; case B; }
            function f(Status $s) {
                if (is_object($s)) { echo 1; }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "function.alreadyNarrowedType"),
        "got: {d:?}"
    );
}

#[test]
fn already_narrowed_type_is_exempted_as_a_trailing_elseif_with_no_else() {
    // Mirrors `elseif.alwaysTrue`'s own "acts as an implicit else" idiom
    // exemption (`reportAlwaysTrueInLastCondition: false`) - `alwaysTrue`
    // only, never `alwaysFalse`.
    let d = diagnostics_for(
        r#"<?php
            function f(int $x) {
                if ($x > 5) {
                    echo 1;
                } elseif (is_int($x)) {
                    echo 2;
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "function.alreadyNarrowedType"),
        "got: {d:?}"
    );
}

#[test]
fn impossible_type_is_never_exempted_as_a_trailing_elseif() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x) {
                if ($x > 5) {
                    echo 1;
                } elseif (is_string($x)) {
                    echo 2;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "function.impossibleType"),
        "got: {d:?}"
    );
}

#[test]
fn haystack_values_the_needle_can_never_match_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit { case Hearts; case Spades; }
            enum Color { case Red; }
            function f(Suit $s, string $name, int $n): void {
                in_array($s, [Suit::Hearts, Color::Red], true);
                in_array($s, [Suit::Hearts, Suit::Spades], true);
                array_search($name, ['a', 1], true);
                in_array($n, ['x', 'y'], true);
                array_keys(['a' => 1], 'one', true);
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "function.impossibleHaystackValue")
        .map(|x| x.message.as_str())
        .collect();
    assert_eq!(
        found,
        vec![
            "Value Color::Red in the haystack passed to in_array() can never be identical to the needle type Suit.",
            "Value 1 in the haystack passed to array_search() can never be identical to the needle type string.",
            "Value 1 in the haystack passed to array_keys() can never be identical to the needle type 'one'.",
        ],
        "got: {d:?}"
    );
}

#[test]
fn using_the_result_of_an_all_void_match_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function a(): void {}
            function b(): void {}
            function g(): int { return 1; }
            function f(int $x): void {
                $r = match ($x) { 1 => a(), default => b() };
                match ($x) { 1 => a(), default => b() };
                $s = match ($x) { 1 => a(), default => throw new \Exception() };
                $t = match ($x) { 1 => a(), default => g() };
            }
        "#,
    );
    assert_eq!(
        d.iter().filter(|x| x.identifier == "match.void").count(),
        2,
        "got: {d:?}"
    );
}
