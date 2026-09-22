//! Declaration-site modifier-consistency + constructor-shape checks - a
//! batch of small, independent boolean-flag-combination checks PHP
//! enforces unconditionally at compile time.

use super::helpers::*;

#[test]
fn an_abstract_method_in_a_non_abstract_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.abstract"),
        "got: {d:?}"
    );
}

#[test]
fn an_abstract_method_in_an_abstract_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Foo {
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.abstract"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_abstract_method_in_a_trait_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T {
                abstract public function foo(): void;
            }
            class UsesT {
                use T;
                public function foo(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.abstract"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_abstract_enum_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "method.abstract" && x.message.contains("Enum")),
        "got: {d:?}"
    );
}

#[test]
fn a_private_abstract_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Foo {
                abstract private function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.abstractPrivate"),
        "got: {d:?}"
    );
}

#[test]
fn a_private_abstract_method_in_an_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Foo {
                private function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.abstractPrivate"),
        "got: {d:?}"
    );
}

#[test]
fn a_public_abstract_method_is_never_flagged_as_abstract_private() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Foo {
                abstract public function bar(): void;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.abstractPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_final_private_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final private function bar(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.finalPrivate"),
        "got: {d:?}"
    );
}

#[test]
fn a_final_private_constructor_is_never_flagged() {
    // A real, common singleton-pattern idiom, matching real PHPStan's
    // own explicit exemption.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final private function __construct() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.finalPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_final_public_method_is_never_flagged_as_final_private() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final public function bar(): void {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.finalPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_public_interface_method_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Foo {
                protected function bar(): void;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.visibility"),
        "got: {d:?}"
    );
}

#[test]
fn a_public_interface_method_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Foo {
                public function bar(): void;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.visibility"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_constructor_with_a_return_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __construct(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "constructor.returnType"),
        "got: {d:?}"
    );
}

#[test]
fn a_constructor_with_no_return_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __construct() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "constructor.returnType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn reassigning_this_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(): void {
                    $this = new Foo();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assign.this"),
        "got: {d:?}"
    );
}

#[test]
fn assigning_an_ordinary_variable_is_never_flagged_as_this_reassignment() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $x = 5;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "assign.this"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn global_this_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                global $this;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "global.this"),
        "got: {d:?}"
    );
}

#[test]
fn global_of_an_ordinary_variable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                global $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "global.this"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn static_this_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                static $this;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "static.this"),
        "got: {d:?}"
    );
}

#[test]
fn static_of_an_ordinary_variable_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f() {
                static $x = 0;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "static.this"),
        "unexpected diagnostics: {d:?}"
    );
}

// `FunctionDefinitionCheck`'s PHP-version-gated native declaration-syntax
// checks.

#[test]
fn a_native_union_parameter_type_pre_8_0_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int|string $x): void {}
        "#,
        |r| r.php_version = Some((7, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.unionTypeNotSupported"
                && x.message.contains(
                    "uses native union types but they're supported only on PHP 8.0 and later"
                )),
        "got: {d:?}"
    );
}

#[test]
fn a_native_union_return_type_pre_8_0_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): int|string { return 1; }
        "#,
        |r| r.php_version = Some((7, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "return.unionTypeNotSupported"),
        "got: {d:?}"
    );
}

#[test]
fn a_native_union_parameter_type_on_8_0_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int|string $x): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.unionTypeNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn with_no_configured_php_version_the_union_type_check_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function f(int|string $x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "parameter.unionTypeNotSupported" | "return.unionTypeNotSupported"
        )),
        "unexpected diagnostics: {d:?}"
    );
}

// `ExistingClassesInPropertyHookTypehintsRule`'s native-union-pre-8.0
// half, reached from a property hook - a hook has no `FunctionDecl` to
// reuse `check_function`'s own registry lookup, so this needs its own
// call site into the shared raw-text core.

#[test]
fn a_set_hooks_native_union_parameter_type_pre_8_0_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                public int $bar {
                    set(int|string $value) {
                        $this->bar = 1;
                    }
                }
            }
        "#,
        |r| r.php_version = Some((7, 4)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.unionTypeNotSupported"
                && x.message.contains("Set hook for property Foo::$bar")),
        "got: {d:?}"
    );
}

#[test]
fn a_set_hooks_native_union_parameter_type_on_8_0_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                public int $bar {
                    set(int|string $value) {
                        $this->bar = 1;
                    }
                }
            }
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.unionTypeNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_set_hook_with_the_implicit_untyped_value_parameter_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                public int $bar {
                    set {
                        $this->bar = $value;
                    }
                }
            }
        "#,
        |r| r.php_version = Some((7, 4)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.unionTypeNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

// `FunctionDefinitionCheck`'s `parameter.unresolvableNativeType`/`return.
// unresolvableNativeType` (PHP 8.1 pure-intersection-type support) - a
// native intersection type-hint whose members can never be satisfied by
// any real value.

#[test]
fn an_impossible_native_intersection_parameter_type_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            final class Foo {}
            final class Bar {}
            function f(Foo&Bar $x): void {}
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.unresolvableNativeType"
                && x.message.contains("has unresolvable native type")),
        "got: {d:?}"
    );
}

#[test]
fn an_impossible_native_intersection_return_type_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            final class Foo {}
            final class Bar {}
            function f(): Foo&Bar { throw new \Exception(); }
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "return.unresolvableNativeType"
                && x.message.contains("has unresolvable native return type")),
        "got: {d:?}"
    );
}

#[test]
fn a_possible_native_intersection_of_two_interfaces_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            interface Foo {}
            interface Bar {}
            function f(Foo&Bar $x): void {}
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.unresolvableNativeType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_impossible_native_intersection_before_php_8_1_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            final class Foo {}
            final class Bar {}
            function f(Foo&Bar $x): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.unresolvableNativeType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn with_no_configured_php_version_the_unresolvable_intersection_check_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            final class Foo {}
            final class Bar {}
            function f(Foo&Bar $x): void {}
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.unresolvableNativeType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_set_hooks_impossible_native_intersection_parameter_type_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            final class Foo {}
            final class Bar {}
            class Baz {
                public mixed $bar {
                    set(Foo&Bar $value) {
                        $this->bar = $value;
                    }
                }
            }
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "parameter.unresolvableNativeType"
                && x.message.contains("set hook for property Baz::$bar")),
        "got: {d:?}"
    );
}

#[test]
fn a_plain_typed_parameter_defaulting_to_null_on_8_4_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(Foo $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.implicitlyNullable"
            && x.message
                == "Deprecated in PHP 8.4: Parameter #1 $x (Foo) is implicitly nullable via default value null."),
        "got: {d:?}"
    );
}

#[test]
fn a_nullable_typed_parameter_defaulting_to_null_is_never_flagged_implicitly_nullable() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(?Foo $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.implicitlyNullable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_union_with_null_defaulting_to_null_is_never_flagged_implicitly_nullable() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(Foo|null $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 4)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.implicitlyNullable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_plain_typed_parameter_defaulting_to_null_before_8_4_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(Foo $x = null): void {}
        "#,
        |r| r.php_version = Some((8, 3)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.implicitlyNullable"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_required_parameter_after_an_optional_one_on_8_0_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($a = 1, $b): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.requiredAfterOptional"
            && x.message
                == "Deprecated in PHP 8.0: Required parameter $b follows optional parameter $a."),
        "got: {d:?}"
    );
}

#[test]
fn a_variadic_parameter_after_an_optional_one_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($a = 1, ...$rest): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.requiredAfterOptional"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn ordinary_parameter_ordering_is_never_flagged_required_after_optional() {
    let d = diagnostics_for_with(
        r#"<?php
            function f($a, $b = 1): void {}
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "parameter.requiredAfterOptional"),
        "unexpected diagnostics: {d:?}"
    );
}

fn promotion_errors(d: &[crate::Diagnostic]) -> Vec<&str> {
    d.iter()
        .filter(|x| {
            x.identifier == "property.invalidPromoted"
                || x.identifier == "property.promotedNotSupported"
        })
        .map(|x| x.message.as_str())
        .collect()
}

#[test]
fn promoted_parameter_outside_a_constructor_is_flagged_everywhere() {
    let d = diagnostics_for(
        r#"<?php
            function plain(private int $a) {}
            class Host {
                public function method(public int $b) {}
                public function make() {
                    $f = function (protected int $c) {};
                    $g = fn (readonly int $d) => 1;
                    return new class {
                        public function inner(private int $e) {}
                    };
                }
            }
        "#,
    );
    assert_eq!(
        promotion_errors(&d),
        vec!["Promoted properties can be in constructor only."; 5],
        "got: {d:?}"
    );
}

#[test]
fn promoted_parameter_in_abstract_or_interface_constructor_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class AbsHost {
                abstract public function __construct(private int $a);
            }
            interface IfaceHost {
                public function __construct(public int $b);
            }
        "#,
    );
    assert_eq!(
        promotion_errors(&d),
        vec!["Promoted properties are not allowed in abstract constructors."; 2],
        "got: {d:?}"
    );
}

#[test]
fn variadic_promoted_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class VarHost {
                public function __construct(private int $a, public int ...$rest) {}
            }
        "#,
    );
    assert_eq!(
        promotion_errors(&d),
        vec!["Promoted property parameter $rest can not be variadic."],
        "got: {d:?}"
    );
}

#[test]
fn well_formed_promotion_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class OkHost {
                public function __construct(
                    private int $a,
                    readonly int $b,
                    private(set) string $c,
                    int ...$plain,
                ) {}
            }
            trait InitTrait {
                public function init(private int $x) {}
            }
            class AliasHost {
                use InitTrait { init as __construct; }
            }
        "#,
    );
    assert!(promotion_errors(&d).is_empty(), "got: {d:?}");
}

#[test]
fn promotion_is_gated_on_the_configured_php_version() {
    let src = r#"<?php
        class GateHost {
            public function __construct(private int $a, final public int $b) {}
        }
    "#;
    let d = diagnostics_for_with(src, |r| r.php_version = Some((7, 4)));
    assert_eq!(
        promotion_errors(&d),
        vec!["Promoted properties are supported only on PHP 8.0 and later."],
        "got: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "property.promotedNotSupported")
    );

    let d = diagnostics_for_with(src, |r| r.php_version = Some((8, 4)));
    assert_eq!(
        promotion_errors(&d),
        vec!["Final promoted properties are supported only on PHP 8.5 and later."],
        "got: {d:?}"
    );

    let d = diagnostics_for_with(src, |r| r.php_version = Some((8, 5)));
    assert!(promotion_errors(&d).is_empty(), "got: {d:?}");
}

#[test]
fn bodiless_non_abstract_method_is_flagged_outside_interfaces() {
    let d = diagnostics_for(
        r#"<?php
            class NoBody {
                public function run();
                public function fine() {}
            }
            abstract class AbsNoBody {
                abstract public function ok();
                public function missing();
            }
            interface Contract {
                public function run();
            }
            trait NoBodyTrait {
                public function run();
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "method.nonAbstract")
        .map(|x| x.message.as_str())
        .collect();
    assert_eq!(
        found,
        vec![
            "Non-abstract method NoBody::run() must contain a body",
            "Non-abstract method AbsNoBody::missing() must contain a body",
            "Non-abstract method NoBodyTrait::run() must contain a body",
        ],
        "got: {d:?}"
    );
}

#[test]
fn promoted_parameter_on_a_closure_inside_a_property_hook_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Hooked {
                public int $n { get => (fn (private int $x) => $x)(1); }
            }
        "#,
    );
    assert_eq!(
        promotion_errors(&d),
        vec!["Promoted properties can be in constructor only."],
        "got: {d:?}"
    );
}
