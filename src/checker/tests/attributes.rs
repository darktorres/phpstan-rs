//! Attribute semantic validation: target checking and constructor-argument
//! arity/type/named-argument checking.

use super::helpers::*;

// Phase 37: `AttributesCheck`'s existence half - `Checker::check_attributes`,
// wired at every declaration site an `Attribute` list can now appear on.

#[test]
fn an_attribute_naming_an_undefined_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[TotallyUndefinedAttribute]
            class Foo {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "attribute.notFound");
}

#[test]
fn an_attribute_naming_a_real_declared_class_is_not_flagged() {
    // Negative case for the above: a genuinely known, genuinely
    // `#[\Attribute]`-marked class (declared right there in the same
    // file, targeting classes) must not be flagged just because it's
    // being used as an attribute.
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class MyAttr {}

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_on_a_method_parameter_naming_an_undefined_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                function f(#[TotallyUndefinedAttribute] string $x) {}
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "attribute.notFound");
}

#[test]
fn an_attribute_naming_a_real_builtin_attribute_class_is_not_flagged() {
    // `Attribute` itself is a real PHP builtin (from the vendored
    // phpstorm-stubs corpus, not user code) - a genuinely known class,
    // not just a plausible-looking name.
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 37 follow-up (this session): constructor-argument validation -
// `resolve_call_args`/`check_args` reused directly for an attribute's own
// constructor call, the same machinery `ExprKind::New` already uses.

#[test]
fn an_attribute_with_no_constructor_given_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {}

            #[MyAttr(1, 2)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.noConstructor"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_class_inheriting_its_constructor_from_an_ancestor_is_not_flagged() {
    // Regression test for a real corpus false positive (PHPBench's own
    // `#[BeforeMethods]`, `class BeforeMethods extends
    // AbstractMethodsAttribute {}`) - the constructor lookup must walk
    // the ancestor chain (`Registry::lookup_method`), not just this
    // class's own declared methods.
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class BaseAttr {
                public function __construct(public string $name) {}
            }

            #[Attribute]
            class MyAttr extends BaseAttr {}

            #[MyAttr('reset')]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_with_no_constructor_given_no_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {}

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_missing_a_required_constructor_argument_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b) {}
            }

            #[MyAttr(1)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_with_too_many_constructor_arguments_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a) {}
            }

            #[MyAttr(1, 2)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_constructor_argument_of_the_wrong_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a) {}
            }

            #[MyAttr("not an int")]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_constructor_called_with_the_correct_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b) {}
            }

            #[MyAttr(1, "x")]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_attribute_constructor_called_with_named_arguments_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b = "default") {}
            }

            #[MyAttr(a: 1)]
            class Foo {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_abstract_attribute_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            abstract class MyAttr {}

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.abstract"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_class_with_a_non_public_constructor_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[Attribute]
            class MyAttr {
                private function __construct() {}
            }

            #[MyAttr]
            class Foo {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "attribute.constructorNotPublic"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_argument_referencing_an_undefined_class_constant_is_checked() {
    // Confirms attribute argument expressions are now actually visited by
    // `check_expr` (previously never checked at all - only the attribute
    // name itself was) - a real class's own genuinely undefined constant
    // (attribute arguments are constant expressions in real PHP, so a
    // plain `SomeClass::CONST` is exactly the shape that appears there)
    // should be caught the same way it would anywhere else.
    let d = diagnostics_for(
        r#"<?php
            class Real {}

            #[Attribute]
            class MyAttr {
                public function __construct(public $a) {}
            }

            #[MyAttr(Real::UNDEFINED_CONST)]
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.notFound"),
        "got: {d:?}"
    );
}

// Phase 37 follow-up: `attribute.notAttribute`/`attribute.target` - the
// vendored real `#[\Override]` stub declaration (`Attribute::
// TARGET_METHOD|TARGET_PROPERTY|TARGET_CLASS_CONSTANT`, confirmed
// directly against `vendor/phpstorm-stubs/Core/Core_c.php`) is real,
// already-registered data these tests exercise directly rather than
// needing a synthetic attribute class.

#[test]
fn using_override_on_a_plain_function_is_flagged_wrong_target() {
    // `Override`'s own real flags don't include `TARGET_FUNCTION`.
    let d = all_diagnostics_for(
        r#"<?php
            #[Override]
            function f(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class Override does not have the function target."),
        "got: {d:?}"
    );
}

#[test]
fn using_override_on_an_ordinary_method_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                #[Override]
                function f(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn using_override_on_an_ordinary_property_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                #[Override]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_attribute_on_a_property_naming_a_class_with_no_attribute_meta_is_flagged_not_an_attribute() {
    let d = all_diagnostics_for(
        r#"<?php
            class MyAttr {}

            class C {
                #[MyAttr]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.notAttribute"
            && x.message == "Class MyAttr is not an Attribute class."),
        "got: {d:?}"
    );
}

#[test]
fn a_user_defined_attribute_class_targeting_only_properties_used_on_a_method_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                #[PropOnly]
                function f(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class PropOnly does not have the method target."),
        "got: {d:?}"
    );
}

#[test]
fn a_user_defined_attribute_class_targeting_only_properties_used_on_a_property_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                #[PropOnly]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.target" || x.identifier == "attribute.notAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 37 follow-up: target-validity for the remaining declaration
// kinds - class, class constant, enum case, and closure/arrow-fn (a
// property/method/function/param already had this).

#[test]
fn a_method_only_attribute_used_on_a_class_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_METHOD)]
            class MethodOnly {}

            #[MethodOnly]
            class C {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class MethodOnly does not have the class target."),
        "got: {d:?}"
    );
}

#[test]
fn a_class_only_attribute_used_on_a_class_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CLASS)]
            class ClassOnly {}

            #[ClassOnly]
            class C {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_class_only_attribute_used_on_a_class_constant_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CLASS)]
            class ClassOnly {}

            class C {
                #[ClassOnly]
                const X = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class ClassOnly does not have the class constant target."),
        "got: {d:?}"
    );
}

#[test]
fn a_class_constant_only_attribute_used_on_a_class_constant_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CLASS_CONSTANT)]
            class ConstOnly {}

            class C {
                #[ConstOnly]
                const X = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_class_constant_only_attribute_used_on_an_enum_case_is_never_flagged() {
    // Real PHP: an enum case's own attribute target is TARGET_CLASS_CONSTANT,
    // not a separate "enum case" target.
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CLASS_CONSTANT)]
            class ConstOnly {}

            enum Suit {
                #[ConstOnly]
                case Hearts;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_method_only_attribute_used_on_an_enum_case_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_METHOD)]
            class MethodOnly {}

            enum Suit {
                #[MethodOnly]
                case Hearts;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"),
        "got: {d:?}"
    );
}

#[test]
fn a_function_only_attribute_used_on_a_closure_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_FUNCTION)]
            class FunctionOnly {}

            function test(): void {
                $f = #[FunctionOnly] function () {};
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_only_attribute_used_on_a_closure_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            function test(): void {
                $f = #[PropOnly] function () {};
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"),
        "got: {d:?}"
    );
}

#[test]
fn a_trait_marked_as_an_attribute_class_itself_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute]
            trait Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.trait"),
        "got: {d:?}"
    );
}

#[test]
fn an_ordinary_class_marked_as_an_attribute_class_is_never_flagged_for_attribute_trait() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute]
            class Foo {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.trait"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_bare_attribute_meta_with_no_flags_argument_defaults_to_target_all() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute]
            class AnyTarget {}

            class C {
                #[AnyTarget]
                function f(): void {}

                #[AnyTarget]
                public int $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.target" || x.identifier == "attribute.notAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_promoted_constructor_property_accepts_either_the_parameter_or_property_target() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                function __construct(#[PropOnly] public int $x) {}
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.target" || x.identifier == "attribute.notAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_promoted_parameter_rejects_a_property_only_attribute() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                function f(#[PropOnly] int $x) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class PropOnly does not have the parameter target."),
        "got: {d:?}"
    );
}

#[test]
fn a_global_const_attribute_naming_an_unknown_class_is_flagged_not_found() {
    let d = diagnostics_for_with(
        r#"<?php
            #[NoSuchAttr]
            const X = 1;
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_global_const_attribute_with_wrong_target_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CLASS)]
            class OnlyClass {}

            #[OnlyClass]
            const X = 1;
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class OnlyClass does not have the constant target."),
        "got: {d:?}"
    );
}

#[test]
fn a_global_const_attribute_with_constant_target_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CONSTANT)]
            class MyAttr {}

            #[MyAttr]
            const X = 1;
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.target" || x.identifier == "attribute.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn attributes_on_global_constants_below_php_8_5_are_flagged_unsupported() {
    let d = diagnostics_for_with(
        r#"<?php
            #[NoSuchAttr]
            const X = 1;
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "constant.attributesNotSupported"),
        "got: {d:?}"
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "attribute.notFound" || x.identifier == "attribute.target"),
        "expected the early return to skip the rest, got: {d:?}"
    );
}

#[test]
fn attributes_on_global_constants_on_php_8_5_or_later_are_never_flagged_for_version_support() {
    let d = diagnostics_for_with(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CONSTANT)]
            class MyAttr {}

            #[MyAttr]
            const X = 1;
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constant.attributesNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_global_const_with_no_configured_php_version_is_never_flagged_for_version_support() {
    let d = diagnostics_for(
        r#"<?php
            #[NoSuchAttr]
            const X = 1;
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constant.attributesNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.notFound"),
        "expected the existence check to still run, got: {d:?}"
    );
}

#[test]
fn a_multi_const_statement_with_a_shared_attribute_reports_the_finding_only_once() {
    let d = diagnostics_for_with(
        r#"<?php
            #[NoSuchAttr]
            const A = 1, B = 2;
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    let count = d
        .iter()
        .filter(|x| x.identifier == "attribute.notFound")
        .count();
    assert_eq!(count, 1, "got: {d:?}");
}

#[test]
fn an_attribute_on_a_get_hook_with_wrong_target_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_CLASS)]
            class OnlyClass {}

            class C {
                public int $x {
                    #[OnlyClass]
                    get => 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class OnlyClass does not have the method target."),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_on_a_set_hook_naming_an_unknown_class_is_flagged_not_found() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public int $x {
                    #[NoSuchAttr]
                    set(int $v) { $this->x = $v; }
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_on_a_property_hook_with_matching_target_is_never_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public int $x {
                    #[Override]
                    get => 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.target"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_nodiscard_attribute_on_a_property_hook_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public int $x {
                    #[\NoDiscard]
                    get => 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.target"
            && x.message == "Attribute class NoDiscard cannot be used on property hooks."),
        "got: {d:?}"
    );
}

#[test]
fn a_nodiscard_attribute_on_an_ordinary_method_is_never_flagged_via_the_hook_special_case() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                #[\NoDiscard]
                function f(): int { return 1; }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.message.contains("cannot be used on property hooks")),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 37, item 1: `attribute.nonRepeatable` - a non-repeatable
// attribute class used more than once at the same declaration site.

#[test]
fn a_non_repeatable_attribute_used_twice_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class Foo {}
            #[Foo]
            #[Foo]
            class Bar {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.nonRepeatable"),
        "got: {d:?}"
    );
}

#[test]
fn a_repeatable_attribute_used_twice_is_never_flagged_nonrepeatable() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute(\Attribute::TARGET_ALL | \Attribute::IS_REPEATABLE)]
            class Foo {}
            #[Foo]
            #[Foo]
            class Bar {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.nonRepeatable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_repeatable_attribute_used_once_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class Foo {}
            #[Foo]
            class Bar {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.nonRepeatable"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 37, item 2: `attribute.deprecated` - the missing eighth usage-
// shape of the already-shipped `@deprecated` family, gated on
// `check_deprecated_usage` like every other one.

#[test]
fn a_deprecated_attribute_class_is_flagged_when_check_deprecated_usage_is_on() {
    let d = diagnostics_for_with(
        r#"<?php
            /** @deprecated */
            #[\Attribute]
            class Foo {}
            #[Foo]
            class Bar {}
        "#,
        |r| r.check_deprecated_usage = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.deprecated"),
        "got: {d:?}"
    );
}

#[test]
fn a_deprecated_attribute_class_is_never_flagged_by_default() {
    let d = diagnostics_for(
        r#"<?php
            /** @deprecated */
            #[\Attribute]
            class Foo {}
            #[Foo]
            class Bar {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "attribute.deprecated"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 37, item 3: `<kind>.deprecatedAttribute` - PHP's native
// `#[\Deprecated]` misused on a class-like declaration itself (a real,
// unconditional PHP fatal error for class/interface/enum, confirmed
// directly against real `php`; PHP-8.5-gated for a trait).

#[test]
fn native_deprecated_attribute_on_a_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Deprecated]
            class Foo {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.deprecatedAttribute"),
        "got: {d:?}"
    );
}

#[test]
fn native_deprecated_attribute_on_an_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Deprecated]
            interface Foo {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "interface.deprecatedAttribute"),
        "got: {d:?}"
    );
}

#[test]
fn native_deprecated_attribute_on_an_enum_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Deprecated]
            enum Foo { case A; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "enum.deprecatedAttribute"),
        "got: {d:?}"
    );
}

#[test]
fn native_deprecated_attribute_on_a_trait_below_php_8_5_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            #[\Deprecated]
            trait Foo {}
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "trait.deprecatedAttribute"),
        "got: {d:?}"
    );
}

#[test]
fn native_deprecated_attribute_on_a_trait_on_php_8_5_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            #[\Deprecated]
            trait Foo {}
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "trait.deprecatedAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn native_deprecated_attribute_on_a_trait_with_no_configured_version_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Deprecated]
            trait Foo {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "trait.deprecatedAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 37, item 4: `<kind>.allowDynamicProperties`/
// `.allowDynamicPropertiesReadonly` - PHP's native
// `#[\AllowDynamicProperties]` on a readonly class, enum, interface, or
// (unconditionally) trait.

#[test]
fn native_allow_dynamic_properties_on_a_readonly_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\AllowDynamicProperties]
            readonly class Foo {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.allowDynamicPropertiesReadonly"),
        "got: {d:?}"
    );
}

#[test]
fn native_allow_dynamic_properties_on_an_ordinary_non_readonly_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\AllowDynamicProperties]
            class Foo {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "class.allowDynamicPropertiesReadonly"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn native_allow_dynamic_properties_on_an_enum_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\AllowDynamicProperties]
            enum Foo { case A; }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "enum.allowDynamicProperties"),
        "got: {d:?}"
    );
}

#[test]
fn native_allow_dynamic_properties_on_an_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\AllowDynamicProperties]
            interface Foo {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "interface.allowDynamicProperties"),
        "got: {d:?}"
    );
}

#[test]
fn native_allow_dynamic_properties_on_a_trait_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\AllowDynamicProperties]
            trait Foo {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "trait.allowDynamicProperties"),
        "got: {d:?}"
    );
}

// Phase 37, item 5: `attribute.interface`/`.enum`/`.abstract`/
// `.constructorNotPublic` - self-declaration-site parity, generalizing
// what was originally just the trait-only `attribute.trait` check.

#[test]
fn an_interface_marked_as_an_attribute_class_itself_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            interface Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.interface"),
        "got: {d:?}"
    );
}

#[test]
fn an_enum_marked_as_an_attribute_class_itself_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            enum Foo { case A; }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.enum"),
        "got: {d:?}"
    );
}

#[test]
fn an_abstract_class_marked_as_an_attribute_class_itself_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            abstract class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "attribute.abstract"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_class_with_a_non_public_constructor_is_flagged_for_self_declaration() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class Foo {
                private function __construct() {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "attribute.constructorNotPublic"),
        "got: {d:?}"
    );
}

#[test]
fn an_ordinary_concrete_attribute_class_with_a_public_constructor_is_never_flagged_for_self_declaration()
 {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class Foo {
                public function __construct() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "attribute.interface"
                | "attribute.enum"
                | "attribute.abstract"
                | "attribute.constructorNotPublic"
                | "attribute.trait"
        )),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 37, item 6: `property.overrideAttribute` - `#[\Override]` on a
// property is only real PHP syntax from 8.5 on.

#[test]
fn override_attribute_on_a_property_below_php_8_5_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Base { public int $x = 1; }
            class Child extends Base {
                #[\Override]
                public int $x = 2;
            }
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "property.overrideAttribute"),
        "got: {d:?}"
    );
}

#[test]
fn override_attribute_on_a_property_on_php_8_5_is_never_flagged_for_version() {
    let d = diagnostics_for_with(
        r#"<?php
            class Base { public int $x = 1; }
            class Child extends Base {
                #[\Override]
                public int $x = 2;
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "property.overrideAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn override_attribute_on_a_property_with_no_configured_version_is_never_flagged_for_version() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public int $x = 1; }
            class Child extends Base {
                #[\Override]
                public int $x = 2;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "property.overrideAttribute"),
        "unexpected diagnostics: {d:?}"
    );
}

// Phase 37, item 7: attribute-reference `<kind>.nameCase` parity.

#[test]
fn an_attribute_reference_with_wrong_case_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class FooAttr {}
            #[fooattr]
            class Bar {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.nameCase"),
        "got: {d:?}"
    );
}

#[test]
fn an_attribute_reference_with_correct_case_is_never_flagged_for_case() {
    let d = diagnostics_for(
        r#"<?php
            #[\Attribute]
            class FooAttr {}
            #[FooAttr]
            class Bar {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.nameCase"),
        "unexpected diagnostics: {d:?}"
    );
}
