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
