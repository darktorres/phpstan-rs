//! General regressions and checks that don't fit another category.

use super::helpers::*;
use crate::token::Position;

#[test]
fn compound_assignment_to_an_undefined_property_is_flagged_as_a_read() {
    // `+=` reads the current value before writing the new one, so
    // (unlike a plain `=`) the property really does need to already
    // exist - this should NOT get the write side's leniency.
    let d = diagnostics_for(
        r#"<?php
            class Box {}
            function f(Box $box) {
                $box->counter += 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("Box::$counter")),
        "got: {d:?}"
    );
}

#[test]
fn property_read_flows_into_argument_type_check() {
    let d = diagnostics_for(
        r#"<?php
            class Box { public string $label = ""; }
            function needsInt(int $x): int { return $x; }
            $box = new Box();
            needsInt($box->label);
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

// Phase 13 Finding C: verified against real PHPStan's own dumpType() -
// `/` gives `int|float` unconditionally whenever it can't prove
// `float` is involved, `float` whenever it can.
#[test]
fn division_of_two_ints_infers_int_or_float() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $a, int $b): int|float {
                return $a / $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn division_involving_a_float_infers_plain_float() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $a, float $b): float {
                return $a / $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn division_of_two_mixed_values_infers_int_or_float_not_mixed() {
    // Real PHPStan's own unconditional `int|float` applies even for
    // two fully unknown operands - a declared `int` return must
    // still be rejected (proving this isn't just falling back to the
    // old, universally-compatible Mixed).
    let d = diagnostics_for(
        r#"<?php
            function f($a, $b): int {
                return $a / $b;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn static_call_arguments_are_still_checked_for_undefined_variables() {
    // ExprKind::StaticCall had no arm in check_expr's match at all at the
    // time this was written - it fell into the wildcard `_ => {}`,
    // so its argument expressions were never visited by any
    // diagnostic. StaticCall gained its own existence checking later
    // (Phase 9); this test predates that and still just proves the
    // narrower claim - its args are real code checked like any other
    // call's - independent of whichever existence check runs later.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public static function bar($x) {}
            }
            function f() {
                Foo::bar($undefinedVar);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined variable")),
        "expected an undefined-variable diagnostic, got: {d:?}"
    );
}

#[test]
fn check_expr_recurses_into_every_simple_wrapper_expression() {
    // Coverage measurement found none of these check_expr arms had
    // ever run in the test suite - Grouping, Clone, (expression-
    // position) Throw, Yield, DynamicVariable, string interpolation,
    // and Cast. Given that the exact same "the wildcard arm never
    // recurses into this" failure mode was a real bug for StaticCall
    // earlier in this audit, each is independently verified here
    // rather than assumed correct from reading the source.
    let d = diagnostics_for(
        r#"<?php
            function f() {
                ($undefinedInGrouping);
                clone $undefinedInClone;
                $x = fn() => throw $undefinedInThrowExpr;
                $y = (int)$undefinedInCast;
                $$undefinedDynVarName;
                "interpolated $undefinedInInterp";
            }
        "#,
    );
    for name in [
        "undefinedInGrouping",
        "undefinedInClone",
        "undefinedInThrowExpr",
        "undefinedInCast",
        "undefinedDynVarName",
        "undefinedInInterp",
    ] {
        assert!(
            d.iter()
                .any(|x| x.message.contains(&format!("undefined variable ${name}"))),
            "expected ${name} to be flagged as undefined, got: {d:?}"
        );
    }
}

#[test]
fn check_expr_recurses_into_dynamic_property_and_method_name_expressions() {
    // $obj->$prop (a single plain variable) is deliberately treated
    // as a literal property name, not the variable's value (see the
    // parser's own "best-effort" comment on that arm) - it does NOT
    // produce a DynamicPropertyAccess/DynamicMethodCall, so it can't
    // exercise this. The brace and double-dollar forms are the ones
    // that actually do, and those are what this checks.
    let d = diagnostics_for(
        r#"<?php
            function f($obj) {
                $obj->{$undefinedInDynPropBrace};
                $obj->$$undefinedInDynMethodDollarDollar();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("undefined variable $undefinedInDynPropBrace")),
        "got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("undefined variable $undefinedInDynMethodDollarDollar")),
        "got: {d:?}"
    );
}

#[test]
fn flags_a_genuinely_undefined_variable() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                return $neverDefined;
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("$neverDefined"));
    // A real, non-line-1 position - the whole point of this phase.
    assert_eq!(d[0].span.start, Position { line: 3, col: 24 });
    assert_eq!(d[0].span.end, Position { line: 3, col: 37 });
    assert_eq!(d[0].level, 0);
}

#[test]
fn diagnostics_are_reported_in_source_order_not_internal_check_order() {
    // ExprKind::Assign checks its right-hand side before its left
    // (so a bare-variable assignment target isn't flagged as its own
    // undefined read) - a real, internal traversal order that
    // doesn't match source order here: `$rhs` (the later column) is
    // pushed before `$obj` (the earlier one). Without sorting,
    // `$rhs`'s diagnostic would come out first despite reading
    // second in the file.
    let d = diagnostics_for(r#"<?php function f() { $obj->prop = $rhs; }"#);
    assert_eq!(d.len(), 2, "got: {d:?}");
    assert!(d[0].message.contains("$obj"), "got: {d:?}");
    assert!(d[1].message.contains("$rhs"), "got: {d:?}");
    assert!(d[0].span.start.col < d[1].span.start.col);
}

#[test]
fn superglobals_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                return $_GET['id'] . $_SERVER['REQUEST_METHOD'] . $GLOBALS['x'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn global_statement_binds_its_names() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                global $config;
                return $config;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn chained_assignment_binds_both_targets() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                $a = $b = 5;
                return $a + $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn accessing_a_non_static_property_through_double_colon_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                public int $instanceOnly = 0;
            }
            function test(): int { return Config::$instanceOnly; }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("access to undefined static property Config::$instanceOnly")),
        "expected an undefined-static-property diagnostic, got: {d:?}"
    );
}

#[test]
fn accessing_a_real_static_property_through_double_colon_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Config {
                public static int $staticOne = 1;
            }
            function test(): int { return Config::$staticOne; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn use_import_resolves_to_its_fully_qualified_name_before_lookup() {
    let d = diagnostics_for(
        r#"<?php
            use App\Domain\Widget;
            function f() { return new Widget(); }
        "#,
    );
    // App\Domain\Widget isn't actually declared anywhere in this
    // snippet, so this documents the known, accepted limitation (no
    // `use` target is ever really loaded here) rather than a false
    // negative in the resolution logic itself - what's under test is
    // that `use`-import resolution runs (Widget -> App\Domain\Widget)
    // before the registry lookup, without panicking and without also
    // flagging an unrelated spurious diagnostic; a real project
    // supplying the actual declaration file is what makes this
    // resolve cleanly, verified separately against Piwigo/vendor.
    assert!(
        d.iter()
            .any(|x| x.message.contains("App\\Domain\\Widget not found")),
        "expected the fully-qualified name in the diagnostic, got: {d:?}"
    );
}

#[test]
fn instantiating_a_class_via_a_leading_backslash_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f() { return new \Foo(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 23 Finding 6, real shape: Piwigo\Core\Logger declares eight
// non-negative severity constants plus `OFF = -1` - the one negative
// one. `const int OFF = -1;` parses as a literal `1` wrapped in a
// unary negation, which `infer_const_type` used to fall back to
// `Mixed` for, widening `['severity' => Logger::OFF]` to
// `array{severity: mixed}` and failing the constructor's declared
// `array{severity?: int|string, ...}` shape.
#[test]
fn a_negative_typed_class_constant_infers_its_real_int_type() {
    let d = diagnostics_for(
        r#"<?php
            final class Logger {
                public const int EMERGENCY = 0;
                public const int OFF = -1;
                /** @param array{severity?: int|string} $options */
                public function __construct(array $options) {}
            }
            function f(): void {
                new Logger(['severity' => Logger::OFF]);
                new Logger(['severity' => Logger::EMERGENCY]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real Piwigo corpus bug (Core\AppInfo::URL = 'https://' . self::DOMAIN):
// a class const's own value referencing another const via a
// concatenation fell to Mixed in infer_const_type - real PHP string
// concatenation always produces a `string` regardless of either
// operand's own type, information available without resolving what
// self::DOMAIN's own value actually is.
#[test]
fn a_class_constant_whose_value_is_a_concatenation_infers_as_string() {
    let d = diagnostics_for(
        r#"<?php
            final class AppInfo {
                public const string DOMAIN = 'example.com';
                public const string URL = 'https://' . self::DOMAIN;
            }
            /** @param array<string> $urls */
            function wantsStringArray(array $urls): void {}
            function f(): void {
                wantsStringArray([
                    'HOME' => AppInfo::URL,
                    'WIKI' => AppInfo::URL . '/doc',
                ]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real Piwigo corpus bug (Bootstrap\RequestPipeline::DEFAULT_MIDDLEWARE):
// ClassConst never carried its own doc comment at all, so a `@var
// list<...>` directly above a `public const array X = [...]` was
// silently dropped, falling back to whatever infer_const_type could
// read off the literal alone (a plain, unshaped `array`).
#[test]
fn a_var_tag_on_a_class_const_pins_its_declared_type() {
    let d = diagnostics_for(
        r#"<?php
            final class Registry {
                /**
                 * @var list<string>
                 */
                public const array NAMES = ['a', 'b'];
            }
            /** @param list<string> $x */
            function wantsList(array $x): void {}
            wantsList(Registry::NAMES);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_an_undefined_function_is_flagged() {
    let d = diagnostics_for("<?php totallyMadeUpFunction();");
    assert!(
        d.iter().any(|x| x
            .message
            .contains("undefined function totallyMadeUpFunction")),
        "expected an undefined-function diagnostic, got: {d:?}"
    );
}

#[test]
fn calling_a_known_function_is_not_flagged() {
    let d = diagnostics_for("<?php function greet() {} greet();");
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_empty_unset_eval_exit_die_are_never_flagged_as_undefined() {
    let d = diagnostics_for(
        r#"<?php
            $x = 1;
            isset($x);
            empty($x);
            unset($x);
            if (false) { exit(); }
            if (false) { die(); }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("undefined function")),
        "unexpected undefined-function diagnostic: {d:?}"
    );
}

#[test]
fn require_include_and_print_are_never_flagged_as_undefined() {
    // These parse as a synthetic ExprKind::Call (see parser.rs's own
    // comment on that choice) - real language constructs, not
    // functions, so they must be exempted the same way isset/empty
    // are, or every real codebase would light up on every include.
    let d = diagnostics_for(
        r#"<?php
            require 'a.php';
            require_once 'b.php';
            include 'c.php';
            include_once 'd.php';
            print 'hello';
        "#,
    );
    assert!(
        !d.iter().any(|x| x.message.contains("undefined function")),
        "unexpected undefined-function diagnostic: {d:?}"
    );
}

#[test]
fn calling_a_function_via_its_use_imported_short_name_resolves_before_lookup() {
    // Mirrors the analogous `new` test: what's under test is that
    // `use function` resolution runs (format_date ->
    // App\Helpers\format_date) before the registry lookup, not that
    // the target actually exists (it isn't declared in this snippet,
    // so a real project supplying the real declaration is what makes
    // this resolve cleanly - verified separately against Piwigo/vendor).
    let d = diagnostics_for(
        r#"<?php
            use function App\Helpers\format_date;
            function f() { return format_date(); }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("undefined function App\\Helpers\\format_date")),
        "expected the fully-qualified name in the diagnostic, got: {d:?}"
    );
}

#[test]
fn calling_a_function_via_a_leading_backslash_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function greet() {}
            function f() { return \greet(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn conditionally_declared_function_is_visible_to_undefined_function_checks() {
    // The real, common `if (!function_exists('foo')) { function foo()
    // {} }` polyfill idiom (symfony/deprecation-contracts and every
    // symfony/polyfill-* package use exactly this) - Registry::index
    // recurses into if-bodies specifically so this isn't flagged.
    let d = diagnostics_for(
        r#"<?php
            if (!function_exists('polyfilled')) {
                function polyfilled() {}
            }
            function f() { return polyfilled(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Findings 12/16/18/19 from the architecture audit - infer_binary
// grouped several operators by superficial similarity rather than by
// each one's real PHP return-type rule; the Unary handling never
// rebound `++`/`--`'s own variable, and never modeled numeric-string
// coercion for unary `-`/`+`.

#[test]
fn spaceship_operator_infers_as_int_not_bool() {
    let d = diagnostics_for(
        r#"<?php
            function compare(int $a, int $b): int {
                return $a <=> $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let wrong = diagnostics_for(
        r#"<?php
            function compareWrong(int $a, int $b): string {
                return $a <=> $b;
            }
        "#,
    );
    assert_eq!(wrong.len(), 1, "got: {wrong:?}");
}

#[test]
fn modulo_and_bitwise_and_shift_operators_always_infer_as_int() {
    for src in [
        r#"<?php function f(float $a, int $b): int { return $a % $b; }"#,
        r#"<?php function f(float $a, int $b): int { return $a << $b; }"#,
        r#"<?php function f(float $a, int $b): int { return $a >> $b; }"#,
        r#"<?php function f(float $a, int $b): int { return $a & $b; }"#,
        r#"<?php function f(float $a, int $b): int { return $a | $b; }"#,
        r#"<?php function f(float $a, int $b): int { return $a ^ $b; }"#,
    ] {
        let d = diagnostics_for(src);
        assert!(d.is_empty(), "unexpected diagnostics for {src}: {d:?}");
    }
}

#[test]
fn increment_on_a_null_variable_rebinds_it_to_int() {
    let d = diagnostics_for(
        r#"<?php
            function counter(): int {
                $x = null;
                $x++;
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn decrement_on_a_null_variable_leaves_it_null() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                $x = null;
                $x--;
                return $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("got null"), "got: {d:?}");
}

#[test]
fn increment_on_a_bool_variable_leaves_it_a_bool() {
    let d = diagnostics_for(
        r#"<?php
            function f(): bool {
                $x = true;
                $x++;
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 52: a plain, unconstrained `string` is genuinely not proven
// numeric (real PHPStan's own `StringType::toNumber()` is
// unconditionally `ErrorType` too - only a `numeric-string`/literal
// numeric value is legal here), so `-$s` is now correctly flagged
// (`unaryOp.invalid`) alongside still inferring as `int` for the
// purposes of `negate`'s own declared return type -
// `unary_minus_on_a_numeric_string_infers_as_int_and_is_never_flagged`
// below is this same test's legal counterpart.
#[test]
fn unary_minus_on_a_non_numeric_string_infers_as_int_but_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function negate(string $s): int { return -$s; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "unaryOp.invalid");
}

#[test]
fn unary_minus_on_a_numeric_string_infers_as_int_and_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function negate(string $s): int { return -$s; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn union_typed_receiver_requires_every_member_to_have_the_method() {
    let d = diagnostics_for(
        r#"<?php
            class A { public function foo(): void {} }
            class B {}
            function f(A|B $x): void { $x->foo(); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefined method"), "got: {d:?}");
}

#[test]
fn union_typed_receiver_requires_every_member_to_have_the_property() {
    let d = diagnostics_for(
        r#"<?php
            class A { public int $x = 0; }
            class B {}
            function f(A|B $obj): int { return $obj->x; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefined property"), "got: {d:?}");
}

#[test]
fn intersection_typed_receiver_accepts_a_method_found_on_any_member() {
    let d = diagnostics_for(
        r#"<?php
            interface Countable2 { public function count2(): int; }
            interface Nameable { public function name(): string; }
            function f(Countable2&Nameable $x): void {
                echo $x->count2();
                echo $x->name();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn intersection_typed_receiver_still_rejects_a_method_missing_from_every_member() {
    let d = diagnostics_for(
        r#"<?php
            interface Countable2 { public function count2(): int; }
            interface Nameable { public function name(): string; }
            function f(Countable2&Nameable $x): void { $x->totallyMissingMethod(); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Item 2: a small initial port of real, ground-truth-labeled fixtures
// from `phpstan/phpstan-src` (MIT-licensed - see `vendor/phpstan-src/LICENSE`),
// adapted from the specific `tests/PHPStan/Rules/**/data/*.php` scenarios
// that motivated Group E's own findings above. Each fixture is trimmed to
// just the class/enum shapes this project's own (simpler) checker models -
// PHPStan-specific assertions this project doesn't attempt (class-case-
// sensitivity, named-constructor-argument checks, generic/template
// variance, ...) are deliberately left out rather than chased for
// byte-for-byte parity. These replace this session's own hand-written
// repros with real, independently-authored ground truth for the same
// gaps, per the audit plan's own Item 2.

#[test]
fn assert_type_is_silent_when_the_inferred_type_matches() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function test(int $x): void { assertType('int', $x); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_type_flags_a_real_mismatch() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function test(int $x): void { assertType('string', $x); }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(
        d[0].message
            .contains("assertType() mismatch: expected type \"string\", actual type \"int\"")
    );
}

#[test]
fn assert_type_also_recognizes_the_fully_qualified_call_form() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $x): void { \PHPStan\Testing\assertType('string', $x); }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("assertType() mismatch"));
}

// The `assertType()` special case in `check_expr`'s `Call` handling
// (see its own doc comment) turns any of phpstan-src's real
// `tests/PHPStan/Analyser/nsrt/*.php` fixtures into a directly
// runnable regression test: `diagnostics_for` returning empty means
// this project's own `infer_expr` matches PHPStan's own labeled
// ground truth exactly, at that exact call site. These two are
// copied verbatim (not trimmed) from real, small fixtures whose
// expected type happens to already line up with this project's own
// (coarser) `Type` representation - most `assertType` calls in the
// corpus assert a precision (integer ranges, literal array shapes,
// ...) this project doesn't attempt, so exact matches are the
// minority, not the norm; these were found by direct trial against
// the vendored corpus, not assumed.
#[test]
fn phpstan_nsrt_bool_cast_matches_real_assert_type() {
    // Verbatim from vendor/phpstan-src/tests/PHPStan/Analyser/nsrt/bug-2997.php.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function test(\SimpleXMLElement $xml): void {
                assertType('bool', (bool) $xml->Exists);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn phpstan_nsrt_float_increment_decrement_matches_real_assert_type() {
    // Derived from vendor/phpstan-src/tests/PHPStan/Analyser/nsrt/bug-10122.php's
    // `float $f` block - real PHP's `++`/`--` on a float stays a
    // float, unlike Finding 16's own `null`/string special cases.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function test(float $f): void { assertType('float', ++$f); }
            function test2(float $f): void { assertType('float', --$f); }
            function test3(float $f): void { assertType('float', $f++); }
            function test4(float $f): void { assertType('float', $f--); }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}
