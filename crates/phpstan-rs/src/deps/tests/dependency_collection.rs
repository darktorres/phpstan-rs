//! Tests that `referenced_names` collects dependencies from class relationships, calls, type hints, closures and PHPDoc tags, resolved against the file's namespace and imports.
use super::helpers::*;

#[test]
fn collects_extends_implements_and_trait_use() {
    let names = refs("<?php class Foo extends Base implements Iface { use SomeTrait; }");
    assert!(names.contains("base"));
    assert!(names.contains("iface"));
    assert!(names.contains("sometrait"));
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
    assert!(names.contains("thing"));
    assert!(names.contains("other"));
    assert!(names.contains("checked"));
}

#[test]
fn collects_type_hints_but_skips_primitives() {
    let names = refs("<?php function f(int $a, ?Foo $b, Bar|Baz $c): Qux {}");
    assert!(names.contains("foo"));
    assert!(names.contains("bar"));
    assert!(names.contains("baz"));
    assert!(names.contains("qux"));
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
    assert!(names.contains("inclosure"));
    assert!(names.contains("inarrow"));
}

#[test]
fn collects_attribute_names_and_their_argument_expressions() {
    let names = refs(
        r#"<?php
            #[OnClass(Thing::class)]
            class Foo {
                #[OnProp]
                public $x;

                #[OnConst]
                const C = 1;

                #[OnMethod]
                public function m(#[OnParam] $p) {}
            }
        "#,
    );
    assert!(names.contains("onclass"));
    assert!(names.contains("thing"));
    assert!(names.contains("onprop"));
    assert!(names.contains("onconst"));
    assert!(names.contains("onmethod"));
    assert!(names.contains("onparam"));
}

#[test]
fn resolves_names_against_the_namespace_and_imports() {
    let names = refs(
        r#"<?php
            namespace App;
            use Lib\Imported;
            use Lib\Pkg as Alias;
            function f() {
                new Imported();
                new Local();
                new \Root();
                new Alias\Inner();
            }
        "#,
    );
    assert!(names.contains("lib\\imported"));
    assert!(names.contains("app\\local"));
    assert!(names.contains("root"));
    assert!(names.contains("lib\\pkg\\inner"));
    assert!(!names.contains("local"));
}

#[test]
fn an_unqualified_function_call_in_a_namespace_collects_both_candidates() {
    let names = refs("<?php namespace App; function f() { g(); \\h(); }");
    assert!(names.contains("app\\g"));
    assert!(names.contains("g"));
    assert!(names.contains("h"));
    assert!(!names.contains("app\\h"));
}

#[test]
fn collects_class_names_in_phpdoc_tags() {
    let names = refs(
        r#"<?php
            namespace App;
            use Lib\Imported;
            /**
             * @mixin Mixed
             * @template T of Bound
             */
            class Foo {
                /** @var list<Imported> */
                public $xs;

                /**
                 * Prose that mentions Ignored is skipped.
                 * @param \Lib\Other $other
                 * @return array<string, Returned>
                 */
                public function m($other) {
                    /** @var Inline $x */
                    $x = g();
                }
            }
        "#,
    );
    assert!(names.contains("app\\mixed"));
    assert!(names.contains("app\\bound"));
    assert!(names.contains("lib\\imported"));
    assert!(names.contains("lib\\other"));
    assert!(names.contains("app\\returned"));
    assert!(names.contains("app\\inline"));
    assert!(!names.contains("app\\ignored"));
    assert!(!names.contains("other"));
    assert!(!names.contains("app\\param"));
}
