//! Closures, arrow fns, first-class callables, callable type hint, Pest-style DSLs.

use super::helpers::*;

#[test]
fn an_array_callable_shaped_as_a_literal_still_satisfies_callable() {
    let d = diagnostics_for(
        r#"<?php
            class Handler { public function handle(int $x): int { return $x; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(Handler $h): int { return apply([$h, 'handle'], 5); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_it_closure_this_is_not_flagged_as_undefined_variable() {
    // Real shape: tests/Browser/AboutControllerTest.php-style Pest
    // test files - `it('...', function (): void { ... $this ... })`
    // at true top-level file scope, no enclosing class/method. No
    // `uses()` call at all here, so `$this` resolves to the default
    // fallback (`Type::Mixed`, since `PHPUnit\Framework\TestCase`
    // isn't registered in this bare snippet) - the core fix under
    // test is that it's recognized as *defined* at all, not flagged
    // as an undefined variable the way it would be with no special
    // handling.
    let d = diagnostics_for(
        r#"<?php
            function it($description, $callback) {}
            it('does something', function (): void {
                $this->assertTrue(true);
            });
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_file_local_uses_resolves_this_to_the_bound_class() {
    // Real Pest's own file-local `uses(X::class);` (no `->in(...)`) -
    // checked first, ahead of any directory-scoped Pest.php config.
    // Calling a real method on the bound class (not just tolerated
    // via Mixed) proves `$this` actually resolved to `RealTestBase`.
    let d = diagnostics_for(
        r#"<?php
            function uses($class) {}
            function it($description, $callback) {}
            class RealTestBase {
                public function realMethod(): int { return 1; }
            }
            uses(RealTestBase::class);
            it('does something', function (): int {
                return $this->realMethod();
            });
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_file_local_uses_still_flags_a_real_undefined_method() {
    // Regression guard: Finding 1's fix must not blanket-suppress
    // every diagnostic inside a Pest closure - only the "undefined
    // variable $this" one. A genuinely undefined method call on the
    // bound class should still be flagged.
    let d = diagnostics_for(
        r#"<?php
            function uses($class) {}
            function it($description, $callback) {}
            class RealTestBase {
                public function realMethod(): int { return 1; }
            }
            uses(RealTestBase::class);
            it('does something', function (): int {
                return $this->totallyUndefinedMethod();
            });
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined method")),
        "expected an undefined-method diagnostic, got: {d:?}"
    );
}

#[test]
fn pest_before_each_property_is_readable_from_another_closure_in_the_same_file() {
    let d = diagnostics_for(
        r#"<?php
            function uses($class) {}
            function beforeEach($callback) {}
            function it($description, $callback) {}
            class RealTestBase {}
            uses(RealTestBase::class);
            beforeEach(function (): void { $this->root = 'x'; });
            it('does something', function (): void {
                echo $this->root;
            });
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pest_a_property_never_assigned_in_any_before_each_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function uses($class) {}
            function beforeEach($callback) {}
            function it($description, $callback) {}
            class RealTestBase {}
            uses(RealTestBase::class);
            beforeEach(function (): void { $this->root = 'x'; });
            it('does something', function (): void {
                echo $this->totallyUndefinedProp;
            });
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("totallyUndefinedProp")),
        "expected an undefined-property diagnostic, got: {d:?}"
    );
}

#[test]
fn pest_before_each_property_assigned_inside_a_nested_if_is_not_recognized() {
    // Matches the real `PestHookPropertyReader`'s own narrower-than-
    // "anywhere in the closure" scan (top-level statements only) -
    // see `pest_config::collect_hook_closure_properties`'s own doc
    // comment. Deliberately still flagged, not a bug.
    let d = diagnostics_for(
        r#"<?php
            function uses($class) {}
            function beforeEach($callback) {}
            function it($description, $callback) {}
            class RealTestBase {}
            uses(RealTestBase::class);
            beforeEach(function (): void { if (true) { $this->root = 'x'; } });
            it('does something', function (): void {
                echo $this->root;
            });
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("$root")),
        "expected an undefined-property diagnostic, got: {d:?}"
    );
}

#[test]
fn generator_closure_with_a_bare_early_return_guard_is_not_a_null_mismatch() {
    // Same idiom as above, but for a closure literal - check_block
    // never routes through check_function, so this needs its own
    // save/set/restore of current_function_is_generator.
    let d = diagnostics_for(
        r#"<?php
            $gen = function (bool $done): Generator {
                if ($done) {
                    return;
                }
                yield 1;
            };
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn closure_bodies_are_not_checked_so_captures_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $base) {
                $fn = function (int $x) use ($base) {
                    return $base + $x;
                };
                return $fn(5);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn this_is_available_inside_a_closure_literal_passed_to_closure_bind() {
    let d = diagnostics_for(
        r#"<?php
            class Secret3 { private int $value = 42; }
            function test(Secret3 $s): int {
                $getter = Closure::bind(function () { return $this->value; }, $s, Secret3::class);
                return $getter();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn closure_bind_still_flags_a_real_bug_inside_the_bound_closure() {
    // A plain empty class, not `stdClass` - Phase 34 Finding D
    // exempts `stdClass` specifically from undefined-property
    // checking (real PHP's own "arbitrary property bag" class), so
    // this test needs an ordinary class to still prove `$this`-
    // rebinding is actually checked.
    let d = diagnostics_for(
        r#"<?php
            class Empty4841 {}
            function test(): void {
                Closure::bind(function () { return $this->totallyUndefinedVar; }, new Empty4841());
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined property")),
        "expected the real bug inside the bound closure to still be caught, got: {d:?}"
    );
}

#[test]
fn this_is_available_inside_an_inline_closure_bound_via_bind_to() {
    let d = diagnostics_for(
        r#"<?php
            class Secret4 { private int $value = 99; }
            function test(Secret4 $s): int {
                $bound = (function () { return $this->value; })->bindTo($s, Secret4::class);
                return $bound();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn first_class_callable_of_an_undefined_function_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(): void {
                $f = totallyUndefinedFunction(...);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("call to undefined function totallyUndefinedFunction")),
        "expected an undefined-function diagnostic, got: {d:?}"
    );
}

#[test]
fn first_class_callable_of_a_real_function_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function real(int $a, int $b): int { return $a + $b; }
            function test(): void {
                $f = real(...);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn first_class_callable_of_an_undefined_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public function bar(): void {} }
            function test(Foo $foo): void {
                $g = $foo->totallyUndefinedMethod(...);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("call to undefined method Foo::totallyUndefinedMethod")),
        "expected an undefined-method diagnostic, got: {d:?}"
    );
}

#[test]
fn first_class_callable_of_a_real_method_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public function bar(): void {} }
            function test(Foo $foo): void {
                $g = $foo->bar(...);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn first_class_callable_of_an_undefined_static_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(): void {
                $h = Foo::totallyUndefinedStatic(...);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x
            .message
            .contains("call to undefined static method Foo::totallyUndefinedStatic")),
        "expected an undefined-static-method diagnostic, got: {d:?}"
    );
}

#[test]
fn first_class_callable_does_not_flag_too_few_arguments() {
    // A real, previously-plausible regression: naively dispatching
    // the wrapped call through its own ordinary arm (which also runs
    // check_args) would misread "no arguments were written" as
    // "zero arguments were passed."
    let d = diagnostics_for(
        r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): void {
                $f = needsTwo(...);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 35 from the architecture audit - none of PHP's real
// "callable" shapes other than a literal Closure satisfied the
// `callable` type hint.

#[test]
fn callable_accepts_an_invokable_object() {
    let d = diagnostics_for(
        r#"<?php
            class Multiplier { public function __invoke(int $x): int { return $x * 2; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(): int { return apply(new Multiplier(), 5); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn callable_accepts_a_string_function_name() {
    let d = diagnostics_for(
        r#"<?php
            function apply(callable $fn, string $s): int { return $fn($s); }
            function test(): int { return apply('strlen', "hello"); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn callable_accepts_a_two_element_method_array() {
    let d = diagnostics_for(
        r#"<?php
            class Handler { public function handle(int $x): int { return $x; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(Handler $h): int { return apply([$h, 'handle'], 5); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn callable_still_rejects_a_non_invokable_object() {
    let d = diagnostics_for(
        r#"<?php
            class NotInvokable {}
            function apply(callable $fn): void {}
            function test(NotInvokable $n): void { apply($n); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn closure_type_hint_is_distinct_from_callable_and_still_rejects_a_string() {
    // Regression-guard for the fix's own caveat: applying callable's
    // new leniency to an undifferentiated Type::Callable would make
    // a strictly Closure-typed parameter wrongly start accepting a
    // string/array/invokable-object too.
    let d = diagnostics_for(
        r#"<?php
            function needsClosure(Closure $fn): void {}
            function test(): void { needsClosure('strlen'); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn a_real_closure_arrow_fn_and_first_class_callable_all_satisfy_the_closure_type_hint() {
    // Regression test for a real bug caught while implementing this
    // fix: splitting `closure`/`callable` into distinct types left
    // infer_expr still inferring a closure/arrow-fn literal (and
    // first-class-callable syntax) as the old, undifferentiated
    // Type::Callable, so a genuine closure literal stopped
    // satisfying the now-stricter native `Closure` type hint.
    let d = diagnostics_for(
        r#"<?php
            function needsClosure(Closure $fn): void {}
            function test(): void {
                needsClosure(fn($x) => $x + 1);
                needsClosure(function ($x) { return $x + 1; });
                needsClosure(strlen(...));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 53: array_map's own contextual parameter-type injection - an
// unhinted callback parameter is checked against the mapped array's
// own real element type, not left as an always-lenient `Mixed`, so a
// genuine bug inside the callback body itself is now caught.
#[test]
fn array_map_arrow_fn_callback_catches_an_undefined_method_on_the_injected_element_type() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            /** @param Foo[] $items */
            function test(array $items) {
                return array_map(fn($x) => $x->undefinedMethod(), $items);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefinedMethod"), "got: {d:?}");
}

#[test]
fn array_map_closure_callback_catches_an_undefined_method_on_the_injected_element_type() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            /** @param Foo[] $items */
            function test(array $items) {
                return array_map(function ($x) {
                    return $x->undefinedMethod();
                }, $items);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("undefinedMethod"), "got: {d:?}");
}

#[test]
fn array_map_callback_using_the_real_injected_method_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            /** @param Foo[] $items */
            function test(array $items) {
                return array_map(fn($x) => $x->bar(), $items);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A parameter's own declared type hint stays authoritative - contextual
// injection only fills in an otherwise-`Mixed` parameter, mirroring
// real PHPStan's own precedence (a declared type always wins over an
// inferred one).
#[test]
fn array_map_callback_with_its_own_declared_param_type_is_checked_against_that_type_not_the_injected_one()
 {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            class Bar {
                public function baz(): int { return 1; }
            }
            /** @param Foo[] $items */
            function test(array $items) {
                return array_map(fn(Bar $x) => $x->baz(), $items);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_map_with_a_by_ref_use_capture_still_widens_the_outer_variable() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $items */
            function test(array $items): int {
                $seen = [];
                array_map(function ($x) use (&$seen) {
                    $seen[] = $x;
                    return $x;
                }, $items);
                return count($seen);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
