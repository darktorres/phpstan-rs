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
             * @mixin Mixture
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
    assert!(names.contains("app\\mixture"));
    assert!(names.contains("app\\bound"));
    assert!(names.contains("lib\\imported"));
    assert!(names.contains("lib\\other"));
    assert!(names.contains("app\\returned"));
    assert!(names.contains("app\\inline"));
    assert!(!names.contains("app\\ignored"));
    assert!(!names.contains("other"));
    assert!(!names.contains("app\\param"));
}

#[test]
fn a_keyword_in_a_phpdoc_type_is_not_a_name() {
    let names = refs(
        r#"<?php
            namespace App;
            class Foo {
                /**
                 * @param int $a
                 * @param non-empty-string|null $b
                 * @param list<positive-int>|array{list: Kept} $c
                 * @param \String $d
                 * @return Closure|callable-string|Other-Thing
                 */
                public function m($a, $b, $c, $d) {}
            }
        "#,
    );
    for keyword in [
        "int", "string", "null", "list", "non", "empty", "positive", "callable",
    ] {
        assert!(!names.contains(keyword), "{keyword}");
        assert!(
            !names.contains(&format!("app\\{keyword}")),
            "app\\{keyword}"
        );
    }
    assert!(names.contains("app\\kept"));
    // A class is still a class: `Closure`, and both halves of a name that
    // only looks hyphenated.
    assert!(names.contains("app\\closure"));
    assert!(names.contains("app\\other"));
    assert!(names.contains("app\\thing"));
}

#[test]
fn collects_use_import_paths() {
    let names = refs("<?php namespace App; use Lib\\Imported; use function Lib\\helper;");
    assert!(names.contains("lib\\imported"));
    assert!(names.contains("lib\\helper"));
}

#[test]
fn a_tags_description_and_the_templates_in_scope_are_not_references() {
    let names = refs(
        r#"<?php
            /**
             * @template T
             * @template-covariant K of Key
             */
            class Box {
                /**
                 * @param T $value a thing of Prose that mentions Words
                 * @param callable(K): Closure $f called with the value
                 * @return array{a: T, b: Shape} the result
                 * @throws Failure when it fails
                 */
                public function m($value, $f) {}

                /**
                 * @template U
                 * @param U|Other $u
                 * @param Typed $x   what $x is about
                 */
                public function n($u, $x) {}
            }

            /** @param T $a A Class named like a template elsewhere */
            function outside($a) {}
        "#,
    );
    for wanted in ["key", "closure", "shape", "failure", "other", "typed", "t"] {
        assert!(names.contains(wanted), "{wanted} is a type in a tag");
    }
    for ignored in [
        "k", "u", "a", "prose", "words", "thing", "fails", "called", "result",
    ] {
        assert!(!names.contains(ignored), "{ignored} is not a reference");
    }
}

#[test]
fn the_one_walk_for_both_sets_is_the_two_walks() {
    let interner = std::sync::Arc::new(syntax::name::Interner::new());
    let sources = [
        "<?php",
        "<?php use Lib\\Imported; global $g; class A extends B implements C { use T; }",
        "<?php namespace N; /** @param Doc $d */ function f(Param $p, ?Other $o): Ret { new InBody(); helper(); return fn() => Closure::bar(); }",
        "<?php namespace N; $f = function (Top $t) { new InTopClosure(); }; new TopLevel(); top_call();",
        "<?php class K { #[Attr(Arg::X)] public function m(#[P] Typed $t = Dflt::V): Back { global $inner; return new InMethod; } public Prop $p = Init::C; const C = Other::D; }",
        "<?php function a() { function nested() { new Deep(); } class InFn extends Base {} } class After {}",
        "<?php enum E: int implements I { case A = Foo::X; public function f() { return Bar::baz(); } }",
        "<?php interface I { public function m(Hint $h): Result; } trait T { public function t() { return Used::x(); } }",
        "<?php /** @template T of Bound */ class G { /** @var list<Elem> */ public $xs; /** @return Ret */ public function m() { /** @var Inline $x */ $x = 1; } }",
    ];
    for (i, source) in sources.iter().enumerate() {
        let parsed = syntax::parse(source.as_bytes(), &interner).expect("parses");
        let (all, declarations) = super::super::both_referenced_names(&parsed.stmts);
        assert_eq!(
            all,
            super::super::referenced_names(&parsed.stmts),
            "all names of source {i}"
        );
        assert_eq!(
            declarations,
            super::super::declaration_referenced_names(&parsed.stmts),
            "declaration names of source {i}"
        );
    }
}

#[test]
fn doc_names_are_each_comments_types_less_the_templates_in_scope() {
    let names = refs(
        r#"<?php
            namespace App;
            use Lib\Imported;

            /**
             * @template T of Bound
             * @extends Base<T, Imported>
             * Prose that mentions Ignored is skipped.
             */
            class Box extends Base {
                /** @var list<T|Held> */
                public $items;

                /**
                 * @template U
                 * @param T|U|Arg $x
                 * @param callable(Cb): Ret $f
                 * @return array{key: Out, other?: T}
                 */
                public function m($x, $f) {}
            }

            /**
             * @template F
             * @param F|Free $v
             * @return Result
             */
            function standalone($v) {}

            /** @return non-empty-string|positive-int|Hyphen-Name */
            function keywords() {}
        "#,
    );
    let expected = [
        "app\\bound",
        "app\\base",
        "lib\\imported",
        "app\\held",
        "app\\arg",
        "app\\cb",
        "app\\ret",
        "app\\out",
        "app\\free",
        "app\\result",
        "app\\hyphen",
        "app\\name",
        // As written too, for a constant-named type's global fallback.
        "bound",
        "base",
        "imported",
        "held",
        "arg",
        "cb",
        "ret",
        "out",
        "free",
        "result",
        "hyphen",
        "name",
    ];
    for name in expected {
        assert!(names.contains(name), "{name} is collected");
    }
    for not in [
        "t",
        "u",
        "f",
        "app\\t",
        "app\\u",
        "app\\f",
        "app\\ignored",
        "ignored",
        "app\\non",
        "app\\empty",
        "app\\positive",
        "int",
        "string",
        "array",
        "list",
    ] {
        assert!(!names.contains(not), "{not} is not collected");
    }
}

#[test]
fn a_walk_that_collects_strings_finds_the_same_names_and_the_namespaced_class_strings() {
    let interner = std::sync::Arc::new(syntax::name::Interner::new());
    let source = r#"<?php
        namespace App;
        use Lib\Imported;
        new Local();
        $a = 'Acme\Lib\Foo';
        $b = '\Acme\Lib\Bar::make';
        $c = ['Acme\Lib\Baz', 'plain', 'Not A\Class', 'Trailing\\', 'x::y'];
        function f() { return 'Deep\Name'; }
    "#;
    let parsed = syntax::parse(source.as_bytes(), &interner).expect("parses");
    let all = super::super::all_referenced_names(&parsed.stmts);
    let (names, outside) = super::super::both_referenced_names(&parsed.stmts);
    assert_eq!(all.names, names);
    assert_eq!(all.outside, outside);
    let mut strings: Vec<&str> = all.strings.iter().map(String::as_str).collect();
    strings.sort();
    assert_eq!(
        strings,
        [
            "acme\\lib\\bar",
            "acme\\lib\\baz",
            "acme\\lib\\foo",
            "deep\\name"
        ]
    );
}
