//! `parameter.defaultValue` - real PHPStan's own `Rules/Functions/
//! IncompatibleDefaultParameterTypeRule.php`/`Rules/Methods/
//! IncompatibleDefaultParameterTypeRule.php`: a parameter's own default
//! value expression's type must be assignable to its declared type.

use super::helpers::*;

#[test]
fn an_incompatible_scalar_default_on_a_function_is_flagged() {
    let d = diagnostics_for(r#"<?php function f(int $x = "hello") {}"#);
    assert!(
        d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_compatible_scalar_default_on_a_function_is_never_flagged() {
    let d = diagnostics_for(r#"<?php function f(int $x = 5) {}"#);
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_null_default_on_an_implicitly_nullable_native_type_is_never_flagged() {
    // `int $x = null` is implicitly nullable (real PHP, pre-8.4) -
    // `FunctionSig::from_decl`'s own `apply_implicit_nullable_default`
    // already widens the resolved param type to `int|null` before this
    // check ever sees it.
    let d = diagnostics_for(r#"<?php function f(int $x = null) {}"#);
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn an_incompatible_default_on_a_method_is_flagged_with_the_method_wording() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function bar(string $s = 5) {}
            }
        "#,
    );
    let hit = d
        .iter()
        .find(|x| x.identifier == "parameter.defaultValue")
        .unwrap_or_else(|| panic!("got: {d:?}"));
    assert!(
        hit.message.contains("method Foo::bar()"),
        "got: {}",
        hit.message
    );
}

#[test]
fn an_untyped_parameter_is_never_flagged_regardless_of_its_default() {
    let d = diagnostics_for(r#"<?php function f($x = "anything") {}"#);
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_variadic_parameter_is_never_flagged() {
    let d = diagnostics_for(r#"<?php function f(int ...$x) {}"#);
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_matching_enum_case_default_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status { case A; case B; }
            function f(Status $s = Status::A) {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn an_empty_array_default_on_an_int_parameter_is_flagged() {
    let d = diagnostics_for(r#"<?php function f(int $x = []) {}"#);
    assert!(
        d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_default_satisfying_a_templates_own_bound_is_never_flagged() {
    // `resolveToBounds` (real PHPStan) / the same-named local step here -
    // a bare `@template T of int` parameter's own default is checked
    // against `T`'s bound (`int`), not the raw, unresolved `T` itself.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T of int
             * @param T $x
             */
            function f($x = 5) {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_default_violating_a_templates_own_bound_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T of int
             * @param T $x
             */
            function f($x = "oops") {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn a_default_of_unknown_type_is_never_reported() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $a = UNDEFINED_CONSTANT_XYZ): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "parameter.defaultValue"),
        "got: {d:?}"
    );
}

#[test]
fn closure_and_arrow_function_default_values_are_checked_against_their_hints() {
    let d = diagnostics_for(
        r#"<?php
            class Holder {
                const NAME = 'n';
                public function make(): void {
                    $a = function (int $x = 'str', ?int $y = null, int $z = null, $u = 1) {};
                    $b = fn (string $s = self::NAME, bool $flag = 0) => 1;
                }
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "parameter.defaultValue")
        .map(|x| x.message.as_str())
        .collect();
    assert_eq!(
        found,
        vec![
            "Default value of the parameter #1 $x ('str') of anonymous function is incompatible with type int.",
            "Default value of the parameter #2 $flag (0) of arrow function is incompatible with type bool.",
        ],
        "got: {d:?}"
    );
}

#[test]
fn a_self_constant_default_resolves_against_the_declaring_class() {
    let d = diagnostics_for(
        r#"<?php
            interface IO {
                public const NORMAL = 2;
                public function write(string $m, int $v = self::NORMAL): void;
            }
            class C {
                const X = 1;
                const NAME = 'n';
                public function f(int $a = self::X, int $b = self::NAME): void {}
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier == "parameter.defaultValue")
        .map(|x| x.message.as_str())
        .collect();
    assert_eq!(
        found,
        vec![
            "Default value of the parameter #2 $b ('n') of method C::f() is incompatible with type int."
        ],
        "got: {d:?}"
    );
}
