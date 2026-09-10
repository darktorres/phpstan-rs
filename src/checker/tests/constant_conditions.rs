//! `if`/`while`/ternary/`elseif`/boolean-and-or-xor constant-condition detection.

use super::helpers::*;

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

// Phase 15 continuation: a bare function/method/static call condition -
// real PHPStan's own `FunctionCallConstantConditionRule`/`*Helper`
// (confirmed directly against `vendor/phpstan-src`, not assumed - see
// `should_skip_constant_condition`'s own doc comment for the full
// finding): it isn't a separate rule at all, just this exact same
// if/while/elseif/ternary/&&/|| family deferring *their own* reporting
// when their own condition happens to be a call - no Collector, no new
// diagnostic identifier, only the pre-existing exclusion needed removing.
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

// Phase 15 follow-up: `NumberComparisonOperatorsConstantConditionRule` -
// a `</>/<=/>=` comparison whose own outcome is provable from both
// operands' known int ranges joins the same constant-condition family,
// with zero new checker code (`infer_binary`'s own comparison arm now
// returns a literal `Type::True`/`Type::False` when provable).

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

// Real corpus find (`Piwigo\Site\SiteRepository::findGalleriesUrlById`):
// unary `-` on an int literal used to leave its own *value* unnegated
// (`-32768` inferred as `IntLiteral(32768)`, dropping the sign entirely -
// only `+`'s numeric-string coercion was actually applied, `-` fell
// through to the identical "return unchanged" branch). Narrowing `$id`
// by the *false* branch of `$id < -32768` then wrongly produced
// `int<32768, max>` instead of `int<-32768, max>`, making the second half
// of `$id < -32768 || $id > 32767` look "always true" against the real
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

// Real corpus find: `++$x`/`--$x` on an `IntLiteral`/`IntRange`-typed
// operand used to return the operand's own type completely unchanged -
// `$n = 0; foreach (...) { ...; $n++; } if ($n > 0)` (a real, extremely
// common accumulator idiom) looked "always false" against a check that
// only fires once anything actually consumes the literal's own exact
// value, since `$n` never actually advanced past `IntLiteral(0)` no
// matter how many times `$n++` ran.
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

// Real corpus find (Symfony's own `LoggerTest`): a do-while's own
// trailing condition, checked once more right after the loop exits, is
// *itself* still consulting a loop-carried variable's post-convergence
// (bounded-pass-approximated) type - `do { ...; $attempts++; } while
// ($attempts < 150);` looked "always true" because this project's own
// 3-pass convergence never actually reaches anywhere near 150, even
// though real semantics genuinely allow it. `loop_depth`'s own guard on
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

// Real, serious corpus find: the vendored `phpstorm-stubs`' own
// `define('PHP_VERSION_ID', 50306)` (a placeholder baked in from
// whatever PHP version generated that stub snapshot) used to be trusted
// as `PHP_VERSION_ID`'s own literal type everywhere - every real
// `PHP_VERSION_ID >= 80300`-style portability check throughout the
// entire vendor ecosystem looked unconditionally, wrongly *false*, the
// moment anything consumed the literal value for a "provably constant"
// purpose. `checker::expr::Checker::global_constant_type` now derives
// this from the configured `php_version` instead (an `IntRange`
// spanning every possible patch digit) - or a fully unbounded
// `Type::Int` with none configured, never a specific literal.
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

// Re-examining Phase 15's own disclosed simplification ("the non-last
// [`for` condition] expressions genuinely are only ever side-effecting
// scratch expressions in every real corpus use seen so far, so they
// stay checked once up front... a narrow, disclosed simplification")
// per the user's own explicit "do all those narrow [items], our corpus
// not having them is not a reason not to do them": the *non-last*
// condition slot needed the identical fixed-point-convergence fix the
// test just above already has for the *last* slot - checking it once,
// up front against the pre-loop entry, saw `$stop`'s own pre-loop
// literal `false` and never the body's own later reassignment,
// wrongly flagging the bare `!$stop` (itself its own standalone check,
// fires anywhere, not just as a whole condition) as always `true`.
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
