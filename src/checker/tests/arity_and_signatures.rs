//! Argument count, parameter/return type checks, spreads, named args, variadics.

use super::helpers::*;
use crate::token::Position;

#[test]
fn flags_wrong_argument_type() {
    let d = diagnostics_for(r#"<?php strlen(42);"#);
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("expects string"));
    // Points at the offending argument `42` itself, not the whole
    // call or the statement.
    assert_eq!(d[0].span.start, Position { line: 1, col: 14 });
    assert_eq!(d[0].span.end, Position { line: 1, col: 16 });
    assert_eq!(d[0].level, 5);
}

#[test]
fn flags_too_few_arguments() {
    let d = diagnostics_for(
        r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            needsTwo(1);
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("expects at least 2"));
    // No single argument represents "the whole call" for an arity
    // mismatch - points at the whole call expression instead.
    assert_eq!(d[0].span.start, Position { line: 3, col: 13 });
    assert_eq!(d[0].span.end, Position { line: 3, col: 24 });
    assert_eq!(d[0].level, 0);
}

#[test]
fn flags_wrong_return_type() {
    let d = diagnostics_for(
        r#"<?php
            function addOne(int $x): int { return $x . "1"; }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("return type mismatch"));
    // Points at the returned expression, not the `return` keyword or
    // the enclosing function declaration.
    assert_eq!(d[0].span.start, Position { line: 2, col: 51 });
    assert_eq!(d[0].span.end, Position { line: 2, col: 59 });
    assert_eq!(d[0].level, 3);
}

#[test]
fn optional_and_variadic_params_do_not_trigger_arity_errors() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $a, int $b = 1, string ...$rest): int { return $a + $b; }
            f(1);
            f(1, 2, "x", "y");
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn generator_falling_off_the_end_with_no_return_is_not_a_null_mismatch() {
    // Real shape: vendor/composer-unused/symbol-parser's own
    // SymbolList::getIterator(): Generator { yield from
    // $this->items; } - a generator always returns a real Generator
    // object, regardless of whether it has an explicit `return`.
    let d = diagnostics_for(
        r#"<?php
            function gen(): Generator {
                yield 1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn generator_with_a_bare_early_return_guard_is_not_a_null_mismatch() {
    // A bare `return;` inside a generator sets Generator::getReturn(),
    // not the function's own return value - a different call site
    // (check_stmt's StmtKind::Return(None) arm) than the
    // falls-off-the-end test above, and a common real idiom
    // (`if ($done) { return; } yield $x;`).
    let d = diagnostics_for(
        r#"<?php
            function gen(bool $done): Generator {
                if ($done) {
                    return;
                }
                yield 1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_non_generator_falling_off_the_end_is_still_a_real_null_mismatch() {
    // Regression guard: Finding 3's fix must not blanket-suppress the
    // real, pre-existing diagnostic for an ordinary (non-generator)
    // function that falls off the end with no return.
    let d = diagnostics_for(
        r#"<?php
            function notAGenerator(): int {
                $x = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("got null")),
        "expected a return type mismatch diagnostic, got: {d:?}"
    );
}

#[test]
fn phpdoc_only_param_type_catches_a_bug_previously_invisible_as_mixed() {
    // Before PHPDoc types, an untyped native param stayed Mixed forever
    // and this call would have been silently skipped.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
            double("not an int");
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("double"));
}

#[test]
fn phpdoc_only_param_type_does_not_flag_correct_usage() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
            double(5);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpdoc_param_type_is_usable_inside_the_function_own_body() {
    // Regression test for a real bug: check_function used to seed
    // parameter locals from the raw AST's native hint only, so a
    // PHPDoc-only type worked for callers but not for the function's
    // own body analyzing its own parameter.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string $s */
            function f($s) {
                return needsInt($s);
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn an_already_defined_variable_passed_by_ref_is_widened_not_left_stale() {
    // Phase 22 Finding 3, real shape: Piwigo\Admin\Install\
    // InstallService::installDbConnect(array &$infos, array &$errors,
    // ...) called as installDbConnect($infos, $errors, ...) where
    // $errors was already `$errors = [];` - the by-ref leniency above
    // only ever applied to an *undefined* variable
    // (`locals.contains_key(name)` skipped anything already tracked),
    // so `$errors` kept its stale, narrow `array{}` shape through the
    // call even though the callee is known to mutate it by reference,
    // and `$errors[0]` afterward was flagged as "no such key".
    let d = diagnostics_for(
        r#"<?php
            function fillErrors(array &$errors): void {}
            function test(): void {
                $errors = [];
                fillErrors($errors);
                echo $errors[0];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn unknown_function_argument_is_leniently_defined_not_flagged() {
    // Calling an undeclared function is itself now flagged (Phase 9),
    // but the original point of this test still holds: $result isn't
    // known to be by-ref (the callee's signature is itself unknown),
    // so it's leniently treated as defined by the call rather than
    // also producing a spurious "undefined variable" diagnostic.
    let d = diagnostics_for(
        r#"<?php
            function f() {
                some_undeclared_function($result);
                return $result;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("undefined variable")),
        "unexpected undefined-variable diagnostic: {d:?}"
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("undefined function some_undeclared_function")),
        "expected an undefined-function diagnostic, got: {d:?}"
    );
}

#[test]
fn known_function_non_by_ref_argument_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function f() {
                return needsInt($neverDefined);
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("$neverDefined"));
}

#[test]
fn spread_argument_is_not_checked_as_the_first_positional_argument() {
    let d = diagnostics_for(
        r#"<?php
            function needsThree(int $a, int $b, int $c): int { return $a + $b + $c; }
            function test(array $args): int { return needsThree(...$args); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn spread_argument_in_a_constructor_call_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(int $x, int $y) {}
            }
            function test(array $coords): Point { return new Point(...$coords); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn spread_argument_in_sprintf_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(array $args): string { return sprintf("%s %s", ...$args); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn spread_argument_in_a_method_call_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Calc { public function add(int $a, int $b): int { return $a + $b; } }
            function test(Calc $c, array $args): int { return $c->add(...$args); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn spread_argument_in_a_static_call_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Calc { public static function multiply(int $a, int $b): int { return $a * $b; } }
            function test(array $args): int { return Calc::multiply(...$args); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_non_spread_call_with_too_few_arguments_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): int { return needsTwo(1); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("expects at least 2 argument(s)")),
        "expected a too-few-arguments diagnostic, got: {d:?}"
    );
}

#[test]
fn a_call_with_too_many_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): int { return needsTwo(1, 2, 3, 4, 5); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("needsTwo() expects exactly 2 argument(s), 5 given")),
        "expected a too-many-arguments diagnostic, got: {d:?}"
    );
}

#[test]
fn a_constructor_call_with_too_many_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point40 { public function __construct(int $x, int $y) {} }
            function test(): Point40 { return new Point40(1, 2, 3, 4, 5); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("Point40::__construct() expects exactly 2 argument(s), 5 given")),
        "expected a too-many-arguments diagnostic, got: {d:?}"
    );
}

#[test]
fn a_method_call_with_too_many_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Calc40 { public function add(int $a, int $b): int { return $a + $b; } }
            function test(Calc40 $c): int { return $c->add(1, 2, 3, 4); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("Calc40::add() expects exactly 2 argument(s), 4 given")),
        "expected a too-many-arguments diagnostic, got: {d:?}"
    );
}

#[test]
fn a_static_call_with_too_many_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Calc40b { public static function multiply(int $a, int $b): int { return $a * $b; } }
            function test(): int { return Calc40b::multiply(1, 2, 3, 4); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("Calc40b::multiply() expects exactly 2 argument(s), 4 given")),
        "expected a too-many-arguments diagnostic, got: {d:?}"
    );
}

#[test]
fn too_many_arguments_says_at_most_when_a_parameter_has_a_default() {
    let d = diagnostics_for(
        r#"<?php
            function withDefault40(int $a, int $b = 5): int { return $a + $b; }
            function test(): int { return withDefault40(1, 2, 3); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("withDefault40() expects at most 2 argument(s), 3 given")),
        "expected an at-most-N-arguments diagnostic, got: {d:?}"
    );
}

#[test]
fn a_variadic_function_accepting_extra_positional_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function withVariadic40(int $a, int ...$rest): int { return $a; }
            function test(): int { return withVariadic40(1, 2, 3, 4, 5); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_docblock_only_variadic_param_type_is_still_checked_against_call_sites() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int ...$nums */
            function sumDocVariadic(...$nums): void {}
            function test(): void { sumDocVariadic(1, 2, "not an int"); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("sumDocVariadic() argument #3 ($nums) expects int, 'not an int' given")),
        "expected a variadic-argument-type diagnostic, got: {d:?}"
    );
}

#[test]
fn a_named_argument_call_in_declaration_order_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f1(int $a, string $b): void {}
            function test(): void { f1(a: 1, b: "x"); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_named_argument_call_in_a_different_order_than_declared_is_not_misflagged() {
    // The real bug Item 1 fixes: naive positional reasoning would
    // compare "x" against $a's own int type and "1" against $b's
    // own string type, both wrongly.
    let d = diagnostics_for(
        r#"<?php
            function f1(int $a, string $b): void {}
            function test(): void { f1(b: "x", a: 1); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_named_argument_with_the_wrong_type_is_still_flagged_against_its_real_parameter() {
    let d = diagnostics_for(
        r#"<?php
            function f1(int $a, string $b): void {}
            function test(): void { f1(b: "x", a: "not an int"); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("f1() argument #1 ($a) expects int")),
        "expected a wrong-type diagnostic attributed to $a, got: {d:?}"
    );
}

#[test]
fn a_mix_of_positional_then_named_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f3(int $a, int $b, int $c): int { return $a + $b + $c; }
            function test(): int { return f3(1, c: 3, b: 2); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_omitted_defaulted_parameter_filled_by_neither_position_nor_name_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f4(int $a, int $b = 5): int { return $a + $b; }
            function test(): int { return f4(a: 1); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_spread_call_leaves_every_parameter_position_unresolved_rather_than_guessing() {
    let d = diagnostics_for(
        r#"<?php
            function f6(int $a, int $b): void {}
            function test(array $args): void { f6(...$args); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_variadic_call_with_more_positional_arguments_than_declared_params_resolves_correctly() {
    let d = diagnostics_for(
        r#"<?php
            function f7(int $a, string ...$rest): void {}
            function test(): void { f7(1, "x", "y", "z"); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_by_ref_parameter_filled_via_a_reordered_named_argument_binds_the_right_variable() {
    // The specific failure mode that originally motivated Item 1: a
    // by-ref output parameter, called with reordered named arguments,
    // must bind the *correct* variable - not whichever one lands at
    // the naive positional index.
    let d = diagnostics_for(
        r#"<?php
            function f9(string $pattern, string $subject, &$matches): int { $matches = []; return 1; }
            function test(string $s): void {
                f9(subject: $s, matches: $m, pattern: '/x/');
                echo $m;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn constructor_argument_count_is_checked() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(int $x, int $y) {}
            }
            function f() { return new Point(1); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("Point::__construct() expects at least 2")),
        "expected a constructor argument-count diagnostic, got: {d:?}"
    );
}

#[test]
fn constructor_argument_type_is_checked() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(int $x) {}
            }
            function f() { return new Point("not an int"); }
        "#,
    );
    assert!(
        d.iter().any(
            |x| x.message.contains("Point::__construct()") && x.message.contains("expects int")
        ),
        "expected a constructor argument-type diagnostic, got: {d:?}"
    );
}

#[test]
fn constructor_call_with_correct_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public function __construct(int $x, int $y) {}
            }
            function f() { return new Point(1, 2); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_native_object_return_type_accepts_any_class_instance() {
    // The narrower, non-generics-related form of the same bug -
    // caught investigating the Doctrine case above, not hypothetical.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f(): object { return new Foo(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Findings 21/22 from the architecture audit - a plain int wasn't
// accepted wherever a float was declared, and a native type hint
// defaulted to null wasn't treated as implicitly nullable.

#[test]
fn int_widens_to_float_in_arguments_returns_and_property_assignment() {
    let d = diagnostics_for(
        r#"<?php
            function distance(float $x, float $y): float { return $x + $y; }
            function test(): float { return distance(3, 4); }
            class Config { public float $rate = 0; }
            function test2(Config $c): void { $c->rate = 5; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_genuinely_wrong_float_argument_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsFloat(float $x): void {}
            function test(): void { needsFloat("not a number"); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn a_native_type_hint_defaulted_to_null_is_implicitly_nullable() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x = null): void {}
            function test(): void { f(null); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_promoted_constructor_property_defaulted_to_null_is_implicitly_nullable() {
    let d = diagnostics_for(
        r#"<?php
            class Widget {
                public function __construct(public int $size = null) {}
            }
            function test(): void { new Widget(null); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 14 from the architecture audit - a PHPDoc type completely
// replaced the native hint, silently dropping nullability the native
// signature already stated.

#[test]
fn docblock_type_merges_the_native_hints_nullability_when_not_repeated() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @param int $x */
                public function bar(?int $x = null): void {}
            }
            function test(Foo $f): void { $f->bar(null); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn docblock_type_still_rejects_a_genuinely_wrong_type() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @param int $x */
                public function bar(?int $x = null): void {}
            }
            function test(Foo $f): void { $f->bar("not an int"); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Finding 23 from the architecture audit - a variadic parameter was
// bound inside its own function/closure/arrow-fn body as the element
// type, not an array of it, in three independent binding sites.

#[test]
fn variadic_parameter_is_an_array_inside_a_top_level_function_body() {
    let d = diagnostics_for(
        r#"<?php
            function f(int ...$nums): int { return count($nums); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn variadic_parameter_is_an_array_inside_a_closure_body() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $f = function (int ...$nums): int { return count($nums); };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn variadic_parameter_is_an_array_inside_an_arrow_fn_body() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $g = fn(int ...$nums): int => count($nums);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 49: named-argument legality (unknown/duplicate/positional-after-
// named, and the unpack-ordering rules real PHP's own parser enforces at
// compile time that this project's own parser doesn't).

#[test]
fn an_unknown_named_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f20(int $a): void {}
            function test(): void { f20(a: 1, x: 2); }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.unknown"
            && x.message.contains("$x")
            && x.message.contains("f20()")),
        "expected an unknown-named-parameter diagnostic, got: {d:?}"
    );
}

#[test]
fn a_duplicate_named_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f21(int $a): void {}
            function test(): void { f21(a: 1, a: 2); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.duplicate" && x.message.contains("$a")),
        "expected a duplicate-named-argument diagnostic, got: {d:?}"
    );
}

#[test]
fn a_named_argument_targeting_an_already_filled_positional_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f22(int $a, int $b): void {}
            function test(): void { f22(1, a: 2); }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.duplicate"
            && x.message.contains("already been passed")
            && x.message.contains("$a")),
        "expected a duplicate-argument diagnostic for $a, got: {d:?}"
    );
}

#[test]
fn a_positional_argument_after_a_named_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f23(int $a, string $b): void {}
            function test(): void { f23(a: 1, "x"); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.positionalAfterNamed"),
        "expected a positional-after-named diagnostic, got: {d:?}"
    );
}

#[test]
fn an_unpacked_argument_after_a_named_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f24(int $a, string $b): void {}
            function test(array $rest): void { f24(a: 1, ...$rest); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.unpackAfterNamed"),
        "expected an unpack-after-named diagnostic, got: {d:?}"
    );
}

#[test]
fn a_non_unpacked_argument_after_an_unpacked_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f25(int $a, string $b): void {}
            function test(array $rest): void { f25(...$rest, 2); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.nonUnpackAfterUnpacked"),
        "expected a non-unpack-after-unpack diagnostic, got: {d:?}"
    );
}

#[test]
fn an_unmatched_named_argument_into_a_trailing_variadic_parameter_is_not_flagged() {
    // Real, legal PHP: any named argument not matching a declared
    // parameter is collected into a trailing variadic parameter's own
    // array, keyed by that name - only a function/method with *no*
    // variadic parameter can have a genuinely "unknown" named argument.
    let d = diagnostics_for(
        r#"<?php
            function f26(int $a, ...$rest): void {}
            function test(): void { f26(1, x: 2); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_duplicate_unmatched_named_argument_into_a_trailing_variadic_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f27(int $a, ...$rest): void {}
            function test(): void { f27(1, x: 2, x: 3); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.duplicate" && x.message.contains("$x")),
        "expected a duplicate-named-argument-into-variadic diagnostic, got: {d:?}"
    );
}

#[test]
fn a_named_argument_written_after_a_spread_argument_is_not_flagged() {
    // Real, legal PHP: `f(...$args, name: $value)` - a named argument
    // may still follow a spread/unpack argument (only a further
    // *positional* one may not).
    let d = diagnostics_for(
        r#"<?php
            function f28(int $a, int $b, int $extra = 0): void {}
            function test(array $args): void { f28(...$args, extra: 5); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 49: by-reference argument legality - a nullsafe access chain
// (`NullsafeCheck::containsNullSafe`'s own equivalent walk) or any other
// non-lvalue expression can never be a real storage location to write a
// by-ref parameter's mutation back into.

#[test]
fn passing_a_nullsafe_property_access_by_reference_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Box30 { public int $value = 0; }
            function setValue30(int &$x): void { $x = 1; }
            function test(?Box30 $b): void { setValue30($b?->value); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.byRef" && x.message.contains("nullsafe")),
        "expected a nullsafe-by-ref diagnostic, got: {d:?}"
    );
}

#[test]
fn passing_a_plain_property_access_by_reference_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Box31 { public int $value = 0; }
            function setValue31(int &$x): void { $x = 1; }
            function test(Box31 $b): void { setValue31($b->value); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn passing_a_method_call_result_by_reference_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Box32 { public function getValue(): int { return 1; } }
            function setValue32(int &$x): void { $x = 1; }
            function test(Box32 $b): void { setValue32($b->getValue()); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.byRef" && x.message.contains("expects a variable")),
        "expected a not-an-lvalue by-ref diagnostic, got: {d:?}"
    );
}

#[test]
fn passing_an_array_index_by_reference_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function setValue33(int &$x): void { $x = 1; }
            function test(array $arr): void { setValue33($arr[0]); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn passing_a_deeply_nested_nullsafe_chain_by_reference_is_still_flagged() {
    // The chain-walk needs to look past the immediate node - the
    // nullsafe operator here is two levels down from the argument's own
    // outermost PropertyAccess.
    let d = diagnostics_for(
        r#"<?php
            class Inner34 { public int $value = 0; }
            class Outer34 { public ?Inner34 $inner = null; }
            function setValue34(int &$x): void { $x = 1; }
            function test(Outer34 $o): void { setValue34($o->inner?->value); }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "argument.byRef" && x.message.contains("nullsafe")),
        "expected a nullsafe-by-ref diagnostic, got: {d:?}"
    );
}
