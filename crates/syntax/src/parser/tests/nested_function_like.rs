//! A file notes whether it builds a function-like or a class-like inside the
//! body of another function-like.

use crate::parse;

fn nested(source: &str) -> bool {
    parse(source).unwrap().facts.has_nested_function_like
}

#[test]
fn only_a_construct_inside_a_body_sets_the_fact() {
    for source in [
        "<?php function f() {} class A { function m() {} public int $p { get => 1; } }",
        "<?php $f = function () { return 1; }; $g = fn($x) => $x; $o = new class { function m() {} };",
        "<?php interface I { function m(); } enum E { case A; function m() {} }",
    ] {
        assert!(!nested(source), "{source}");
    }
    for source in [
        "<?php function f() { return function () {}; }",
        "<?php function f() { return fn() => 1; }",
        "<?php function f() { function g() {} }",
        "<?php function f() { interface I { function m(); } }",
        "<?php class A { function m() { return new class {}; } }",
        "<?php $f = function () { class B {} };",
        "<?php $f = fn() => fn() => 1;",
        "<?php class A { public int $p { get { return (function () {})(); } } }",
        "<?php class A { public int $p { get => (fn() => 1)(); } }",
    ] {
        assert!(nested(source), "{source}");
    }
}
