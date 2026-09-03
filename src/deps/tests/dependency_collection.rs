//! Tests that `referenced_names` collects dependencies from class relationships, calls, type hints, and closures.
use super::helpers::*;

#[test]
fn collects_extends_implements_and_trait_use() {
    let names = refs("<?php class Foo extends Base implements Iface { use SomeTrait; }");
    assert!(names.contains("Base"));
    assert!(names.contains("Iface"));
    assert!(names.contains("SomeTrait"));
}

#[test]
fn collects_calls_new_static_access_and_instanceof() {
    let names = refs(
        r#"<?php
            function f($x) {
                helper();
                new Thing();
                Other::bar();
                Other::CONST;
                if ($x instanceof Checked) {}
            }
        "#,
    );
    assert!(names.contains("helper"));
    assert!(names.contains("Thing"));
    assert!(names.contains("Other"));
    assert!(names.contains("Checked"));
}

#[test]
fn collects_type_hints_but_skips_primitives() {
    let names = refs("<?php function f(int $a, ?Foo $b, Bar|Baz $c): Qux {}");
    assert!(names.contains("Foo"));
    assert!(names.contains("Bar"));
    assert!(names.contains("Baz"));
    assert!(names.contains("Qux"));
    assert!(!names.contains("int"));
}

#[test]
fn collects_from_closures_and_arrow_functions() {
    let names = refs(
        r#"<?php
            $f = function () { return new InClosure(); };
            $g = fn() => new InArrow();
        "#,
    );
    assert!(names.contains("InClosure"));
    assert!(names.contains("InArrow"));
}
