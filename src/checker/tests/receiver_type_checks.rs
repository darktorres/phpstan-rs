//! `method.nonObject`/`property.nonObject`/`offsetAccess.nonOffsetAccessible`/
//! `foreach.nonIterable`/`offsetAccess.nonArray`/`clone.nonObject`/
//! `callable.nonCallable`/`arrayUnpacking.nonIterable`/
//! `argument.unpackNonIterable` - flags a receiver/operand whose type is
//! confirmed to never support the attempted operation at all, closing the
//! gap the existing `method.notFound`/`property.notFound`/
//! `shapeAccess.notFound` family leaves for a non-object/non-iterable/
//! non-array-like value (see `registry::global_vars`'s own sibling
//! feature for the "already resolved to a real class" half of this
//! story).
use super::helpers::*;

mod method_and_property_non_object {
    use super::*;

    #[test]
    fn a_method_call_on_each_closed_scalar_type_is_flagged() {
        for (hint, expr) in [
            ("string", "$x"),
            ("int", "$x"),
            ("array", "$x"),
            ("bool", "$x"),
        ] {
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({hint} {expr}) {{
                        {expr}->foo();
                    }}
                "#
            ));
            assert_eq!(d.len(), 1, "hint {hint}: got {d:?}");
            assert_eq!(
                d[0].identifier, "method.nonObject",
                "hint {hint}: got {d:?}"
            );
        }
    }

    #[test]
    fn a_property_access_on_each_closed_scalar_type_is_flagged() {
        for hint in ["string", "int", "array", "bool"] {
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({hint} $x) {{
                        echo $x->foo;
                    }}
                "#
            ));
            assert_eq!(d.len(), 1, "hint {hint}: got {d:?}");
            assert_eq!(
                d[0].identifier, "property.nonObject",
                "hint {hint}: got {d:?}"
            );
        }
    }

    #[test]
    fn a_bare_null_receiver_is_flagged_for_method_and_property() {
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    $x->foo();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "method.nonObject");
    }

    #[test]
    fn a_void_returning_call_s_result_used_as_a_receiver_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function voidFn(): void {}
                function f() {
                    $x = voidFn();
                    $x->foo();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "method.nonObject");
    }

    #[test]
    fn a_mixed_typed_receiver_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f($x) {
                    $x->foo();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_union_mixing_a_real_class_with_a_closed_scalar_member_is_flagged() {
        // The corrected "any member closed" `Union` rule - a `Foo|string`
        // receiver is flagged even though `Foo` itself has the method,
        // since the `string` branch never can.
        let d = diagnostics_for(
            r#"<?php
                class Foo {
                    function bar() {}
                }
                /** @param Foo|string $x */
                function f($x) {
                    $x->bar();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "method.nonObject");
    }

    #[test]
    fn a_union_of_two_real_classes_both_having_the_method_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Foo {
                    function bar() {}
                }
                class Baz {
                    function bar() {}
                }
                /** @param Foo|Baz $x */
                function f($x) {
                    $x->bar();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_union_with_a_null_member_is_not_flagged_unlike_a_closed_scalar_member() {
        // Real PHPStan strips `Null` out of any non-pure-`Null` type
        // before running this whole check family at all
        // (`checkNullables`, off by default through level 7) - a
        // `Foo|null` receiver is checked as plain `Foo`, never as a
        // `Union` with a closed `Null` member. Caught for real by
        // `closures_and_callables.rs`'s own pre-existing
        // `this_is_available_inside_a_closure_literal_passed_to_closure_bind`
        // test (`Closure::bind()`'s genuine `Closure|null` return type)
        // failing against the first implementation of this rule.
        let d = diagnostics_for(
            r#"<?php
                class Foo {
                    function bar() {}
                }
                /** @param Foo|null $x */
                function f($x) {
                    $x->bar();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn nullsafe_method_call_on_a_null_only_receiver_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    $x?->foo();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn plain_method_call_on_the_same_null_only_receiver_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    $x->foo();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "method.nonObject");
    }

    #[test]
    fn a_class_string_typed_value_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                /** @param class-string $x */
                function f($x) {
                    $x->foo();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "method.nonObject");
    }

    #[test]
    fn a_template_param_bound_to_a_closed_type_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                /**
                 * @template T of string
                 * @param T $x
                 */
                function f($x) {
                    $x->foo();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "method.nonObject");
    }

    #[test]
    fn an_unbounded_template_param_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                /**
                 * @template T
                 * @param T $x
                 */
                function f($x) {
                    $x->foo();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn stdclass_method_calls_are_flagged_as_nonobject_not_notfound() {
        // Mirrors real PHPStan's own `ObjectType::canCallMethods()`
        // special case for exactly this class.
        let d = diagnostics_for(
            r#"<?php
                function f(): void {
                    $c = new stdClass();
                    $c->someMethod();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "method.nonObject");
        assert!(d[0].message.contains("someMethod"), "got: {d:?}");
    }

    #[test]
    fn a_real_unrelated_class_s_method_call_is_unaffected() {
        let d = diagnostics_for(
            r#"<?php
                class Template {
                    function assign($name, $value) {}
                }
                function f(Template $t) {
                    $t->assign('x', 1);
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
}

mod offset_non_accessible {
    use super::*;

    #[test]
    fn indexing_each_closed_scalar_type_is_flagged() {
        for hint in ["int", "bool", "float"] {
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({hint} $x) {{
                        echo $x['a'];
                    }}
                "#
            ));
            assert_eq!(d.len(), 1, "hint {hint}: got {d:?}");
            assert_eq!(
                d[0].identifier, "offsetAccess.nonOffsetAccessible",
                "hint {hint}: got {d:?}"
            );
        }
    }

    #[test]
    fn indexing_a_void_returning_call_s_result_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function voidFn(): void {}
                function f() {
                    $x = voidFn();
                    echo $x['a'];
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "offsetAccess.nonOffsetAccessible");
    }

    #[test]
    fn indexing_a_null_or_string_base_is_not_flagged() {
        // The two verified exceptions: `NullType`/`StringType` both
        // return "yes" from real PHPStan's own `isOffsetAccessible()`.
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    echo $x['a'];
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let d = diagnostics_for(
            r#"<?php
                function f(string $x) {
                    echo $x['a'];
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn indexing_an_arrayaccess_implementing_class_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Container implements ArrayAccess {
                    function offsetExists($offset): bool { return true; }
                    function offsetGet($offset): mixed { return null; }
                    function offsetSet($offset, $value): void {}
                    function offsetUnset($offset): void {}
                }
                function f(Container $c) {
                    echo $c['a'];
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn indexing_a_plain_class_not_implementing_arrayaccess_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Plain {}
                function f(Plain $p) {
                    echo $p['a'];
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "offsetAccess.nonOffsetAccessible");
    }

    #[test]
    fn indexing_an_interface_typed_receiver_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                interface Countable2 {}
                function f(Countable2 $c) {
                    echo $c['a'];
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_write_onto_a_null_or_scalar_base_is_not_flagged_by_this_check() {
        // Real PHP auto-vivifies `null` into an array on write - this
        // check is deliberately read-only, mirroring
        // `check_shape_key_access`'s own identical write-side exclusion.
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    $x['a'] = 1;
                }
            "#,
        );
        assert!(
            !d.iter()
                .any(|x| x.identifier == "offsetAccess.nonOffsetAccessible"),
            "unexpected diagnostics: {d:?}"
        );
    }
}

mod foreach_non_iterable {
    use super::*;

    #[test]
    fn foreach_over_each_closed_scalar_type_is_flagged() {
        for hint in ["int", "string", "bool"] {
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({hint} $x) {{
                        foreach ($x as $v) {{
                        }}
                    }}
                "#
            ));
            assert_eq!(d.len(), 1, "hint {hint}: got {d:?}");
            assert_eq!(
                d[0].identifier, "foreach.nonIterable",
                "hint {hint}: got {d:?}"
            );
        }
    }

    #[test]
    fn foreach_over_null_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    foreach ($x as $v) {
                    }
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "foreach.nonIterable");
    }

    #[test]
    fn foreach_over_a_void_returning_call_s_result_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function voidFn(): void {}
                function f() {
                    foreach (voidFn() as $v) {
                    }
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "foreach.nonIterable");
    }

    #[test]
    fn foreach_over_a_plain_non_traversable_class_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Plain {}
                function f(Plain $p) {
                    foreach ($p as $v) {
                    }
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "foreach.nonIterable");
    }

    #[test]
    fn foreach_over_an_iterator_implementing_class_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                /** @implements \Iterator<int, string> */
                class StringIterator implements \Iterator {
                    private array $items = ['a', 'b'];
                    private int $pos = 0;
                    public function current(): mixed { return $this->items[$this->pos]; }
                    public function next(): void { $this->pos++; }
                    public function key(): mixed { return $this->pos; }
                    public function valid(): bool { return isset($this->items[$this->pos]); }
                    public function rewind(): void { $this->pos = 0; }
                }
                function f(StringIterator $it): void {
                    foreach ($it as $k => $v) {
                    }
                }
            "#,
        );
        assert!(
            !d.iter().any(|x| x.identifier == "foreach.nonIterable"),
            "unexpected diagnostics: {d:?}"
        );
    }

    #[test]
    fn foreach_over_an_array_or_mixed_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f(array $a) {
                    foreach ($a as $v) {
                    }
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let d = diagnostics_for(
            r#"<?php
                function f($x) {
                    foreach ($x as $v) {
                    }
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn the_loop_body_is_still_checked_against_mixed_after_the_diagnostic_fires() {
        let d = diagnostics_for(
            r#"<?php
                function f(int $x) {
                    foreach ($x as $v) {
                        $v->realMethodThatDoesNotMatterHere();
                    }
                }
            "#,
        );
        // Just the one `foreach.nonIterable` - `$v` is bound to `Mixed`
        // inside the (still-checked) body, so the call on it isn't
        // separately flagged as `method.nonObject`/`method.notFound`.
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "foreach.nonIterable");
    }
}

mod destructuring_non_array {
    use super::*;

    #[test]
    fn destructuring_each_closed_scalar_type_including_string_is_flagged() {
        // `String` is flagged here, unlike `offsetAccess.nonOffsetAccessible`'s
        // own `$str[0]` exemption - destructuring a string was never
        // given PHP language support.
        for hint in ["int", "bool", "string"] {
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({hint} $x) {{
                        [$a, $b] = $x;
                    }}
                "#
            ));
            assert_eq!(d.len(), 1, "hint {hint}: got {d:?}");
            assert_eq!(
                d[0].identifier, "offsetAccess.nonArray",
                "hint {hint}: got {d:?}"
            );
        }
    }

    #[test]
    fn destructuring_null_or_a_void_returning_call_s_result_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    [$a, $b] = $x;
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "offsetAccess.nonArray");

        let d = diagnostics_for(
            r#"<?php
                function voidFn(): void {}
                function f() {
                    [$a, $b] = voidFn();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "offsetAccess.nonArray");
    }

    #[test]
    fn destructuring_a_real_array_or_array_shape_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f(array $x) {
                    [$a, $b] = $x;
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let d = diagnostics_for(
            r#"<?php
                function f() {
                    [$a, $b] = [1, 2];
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn destructuring_an_arrayaccess_implementing_class_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Container implements ArrayAccess {
                    function offsetExists($offset): bool { return true; }
                    function offsetGet($offset): mixed { return null; }
                    function offsetSet($offset, $value): void {}
                    function offsetUnset($offset): void {}
                }
                function f(Container $c) {
                    [$a, $b] = $c;
                }
            "#,
        );
        assert!(
            !d.iter().any(|x| x.identifier == "offsetAccess.nonArray"),
            "unexpected diagnostics: {d:?}"
        );
    }

    #[test]
    fn list_syntax_and_bracket_syntax_produce_the_identical_result() {
        let flagged = diagnostics_for(
            r#"<?php
                function f(int $x) {
                    list($a, $b) = $x;
                }
            "#,
        );
        assert_eq!(flagged.len(), 1, "got: {flagged:?}");
        assert_eq!(flagged[0].identifier, "offsetAccess.nonArray");

        let not_flagged = diagnostics_for(
            r#"<?php
                function f(array $x) {
                    list($a, $b) = $x;
                }
            "#,
        );
        assert!(
            not_flagged.is_empty(),
            "unexpected diagnostics: {not_flagged:?}"
        );
    }
}

mod clone_non_object {
    use super::*;

    #[test]
    fn cloning_each_closed_scalar_type_is_flagged() {
        for hint in ["int", "string", "array"] {
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({hint} $x) {{
                        $y = clone $x;
                    }}
                "#
            ));
            assert_eq!(d.len(), 1, "hint {hint}: got {d:?}");
            assert_eq!(d[0].identifier, "clone.nonObject", "hint {hint}: got {d:?}");
        }
    }

    #[test]
    fn cloning_null_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    $y = clone $x;
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "clone.nonObject");
    }

    #[test]
    fn cloning_a_real_class_or_mixed_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Foo {}
                function f(Foo $x) {
                    $y = clone $x;
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let d = diagnostics_for(
            r#"<?php
                function f($x) {
                    $y = clone $x;
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn cloning_stdclass_is_not_flagged_unlike_calling_a_method_on_it() {
        // `isCloneable()` has no `stdClass` exception, unlike
        // `canCallMethods()` - the two must not share a classifier.
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = new stdClass();
                    $y = clone $x;
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
}

mod callable_non_callable {
    use super::*;

    #[test]
    fn invoking_each_closed_scalar_type_is_flagged() {
        for hint in ["int", "bool"] {
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({hint} $x) {{
                        $x();
                    }}
                "#
            ));
            assert_eq!(d.len(), 1, "hint {hint}: got {d:?}");
            assert_eq!(
                d[0].identifier, "callable.nonCallable",
                "hint {hint}: got {d:?}"
            );
        }
    }

    #[test]
    fn invoking_null_or_a_void_returning_call_s_result_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f() {
                    $x = null;
                    $x();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "callable.nonCallable");

        let d = diagnostics_for(
            r#"<?php
                function voidFn(): void {}
                function f() {
                    $x = voidFn();
                    $x();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "callable.nonCallable");
    }

    #[test]
    fn invoking_a_plain_class_with_no_invoke_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Plain {}
                function f(Plain $x) {
                    $x();
                }
            "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].identifier, "callable.nonCallable");
    }

    #[test]
    fn invoking_mixed_string_or_array_is_not_flagged() {
        for hint in ["", "string", "array"] {
            let param = if hint.is_empty() {
                "$x".to_string()
            } else {
                format!("{hint} $x")
            };
            let d = diagnostics_for(&format!(
                r#"<?php
                    function f({param}) {{
                        $x();
                    }}
                "#
            ));
            assert!(d.is_empty(), "hint {hint:?}: unexpected diagnostics: {d:?}");
        }
    }

    #[test]
    fn invoking_a_closure_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                function f(Closure $x) {
                    $x();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn invoking_a_nullable_closure_is_not_flagged() {
        // The real shape `Closure::bind()`'s own `?Closure` return type
        // exercises - `Null` is dropped from the `Union` before
        // evaluation, not treated as a normal closed member.
        let d = diagnostics_for(
            r#"<?php
                /** @param Closure|null $x */
                function f($x) {
                    $x();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn invoking_an_invoke_bearing_object_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
                class Invokable {
                    function __invoke() {}
                }
                function f(Invokable $x) {
                    $x();
                }
            "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
}
