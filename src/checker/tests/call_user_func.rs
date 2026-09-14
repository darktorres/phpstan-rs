//! `call_user_func(_array)` argument re-checking, scoped to the two
//! statically-resolvable callable shapes - a string-literal function
//! name, and a two-element array literal naming a class/object plus a
//! literal method name.

use super::helpers::*;

#[test]
fn call_user_func_string_callable_wrong_arg_count() {
    let d = diagnostics_for(
        r#"<?php
            function needsTwo(int $a, int $b): void {}
            function test(): void {
                call_user_func('needsTwo', 1);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_string_callable_wrong_arg_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $a): void {}
            function test(): void {
                call_user_func('needsInt', "not an int");
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_array_callable_string_method_literal_array() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                public function needsInt(int $a): void {}
            }
            function test(): void {
                call_user_func_array(['C', 'needsInt'], ["not an int"]);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_array_class_string_method() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                public function needsInt(int $a): void {}
            }
            function test(): void {
                call_user_func_array([C::class, 'needsInt'], ["not an int"]);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_array_instance_method() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                public function needsInt(int $a): void {}
            }
            function test(C $c): void {
                call_user_func_array([$c, 'needsInt'], ["not an int"]);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_matching_types_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $a): void {}
            function test(): void {
                call_user_func('needsInt', 5);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.type" || x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_unresolvable_callable_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function test(callable $cb): void {
                call_user_func($cb, "anything");
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.type" || x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_array_non_literal_args_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $a): void {}
            function test(array $args): void {
                call_user_func_array('needsInt', $args);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.type" || x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn call_user_func_spread_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $a): void {}
            function test(array $rest): void {
                call_user_func('needsInt', ...$rest);
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "argument.type" || x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}
