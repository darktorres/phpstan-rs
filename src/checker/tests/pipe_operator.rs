//! Real PHPStan's `PipeOperatorRule` (`pipe.byRef`) - PHP 8.5's `|>` pipe
//! always passes by value, so a right-hand callable whose first
//! parameter is declared by-reference can never receive anything.

use super::helpers::*;

#[test]
fn a_closure_literal_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $x) {
                $x |> function (&$y) { return $y; };
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn an_arrow_fn_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $x) {
                $x |> fn (&$y) => $y;
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn a_closure_literal_with_no_byref_params_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $x) {
                $x |> function (int $y) { return $y; };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "pipe.byRef"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_first_class_callable_to_a_function_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function takesByRef(&$y) { return $y; }
            function test(int $x) {
                $x |> takesByRef(...);
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn a_first_class_callable_to_a_function_with_no_byref_params_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $x) {
                $x |> strtoupper(...);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "pipe.byRef"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_first_class_callable_to_a_method_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function takesByRef(&$y) { return $y; }
            }
            function test(int $x, Foo $f) {
                $x |> $f->takesByRef(...);
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn a_first_class_callable_to_a_static_method_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public static function takesByRef(&$y) { return $y; }
            }
            function test(int $x) {
                $x |> Foo::takesByRef(...);
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn a_bare_string_naming_a_function_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function takesByRef(&$y) { return $y; }
            function test(int $x) {
                $x |> 'takesByRef';
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn a_bare_string_naming_a_function_with_no_byref_params_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $x) {
                $x |> 'strtoupper';
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "pipe.byRef"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_array_callable_to_a_method_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function takesByRef(&$y) { return $y; }
            }
            function test(int $x, Foo $f) {
                $x |> [$f, 'takesByRef'];
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn an_array_callable_naming_a_class_by_literal_string_with_a_byref_first_param_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public static function takesByRef(&$y) { return $y; }
            }
            function test(int $x) {
                $x |> ['Foo', 'takesByRef'];
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "pipe.byRef"), "got: {d:?}");
}

#[test]
fn a_plain_variable_holding_a_closure_is_never_flagged_unresolvable() {
    // Can't be resolved to a concrete parameter list from its own
    // literal syntax - silently skipped, matching real PHPStan's own
    // silent skip whenever it can't resolve a confident callable type
    // either.
    let d = diagnostics_for(
        r#"<?php
            function test(int $x, callable $cb) {
                $x |> $cb;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "pipe.byRef"),
        "unexpected diagnostics: {d:?}"
    );
}
