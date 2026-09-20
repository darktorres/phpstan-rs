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
