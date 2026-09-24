//! Closures, arrow fns, first-class callables, callable type hint, Pest-style DSLs.

use super::helpers::*;

#[test]
fn pest_it_closure_this_is_not_flagged_as_undefined_variable() {
    // Real shape: tests/Browser/AboutControllerTest.php-style Pest
    // test files - `it('...', function (): void { ... $this ... })`
    // at true top-level file scope, no enclosing class/method. No
    // `uses()` call at all here, so `$this` resolves to the default
    // fallback (`Type::Mixed(false)`, since `PHPUnit\Framework\TestCase`
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

// PHP's other "callable" shapes (an invokable object, a string function
// name, a two-element method array), not just a literal Closure, must
// satisfy the `callable` type hint.

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
    // Splitting `closure`/`callable` into distinct types must not leave
    // `infer_expr` still inferring a closure/arrow-fn literal (and
    // first-class-callable syntax) as the old, undifferentiated
    // Type::Callable, or a genuine closure literal stops satisfying the
    // stricter native `Closure` type hint.
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
