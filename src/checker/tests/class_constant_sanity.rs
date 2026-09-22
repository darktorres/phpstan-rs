//! Class/enum constant declaration checks: naming, typed-constant value-vs-
//! declared-type consistency, and PHP-version-gated modifiers.

use super::helpers::*;

#[test]
fn a_class_constant_named_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const class = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.class"),
        "got: {d:?}"
    );
}

#[test]
fn an_ordinarily_named_class_constant_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const BAR = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.class"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_enum_constant_named_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                const class = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.class"),
        "got: {d:?}"
    );
}

// `ClassConstantRule`'s own typed-constant half (PHP 8.3's `const TYPE
// NAME = value;`) - a real, unconditional fatal error ("Cannot use
// string as value for class constant Foo::BAD of type int").

#[test]
fn a_typed_constant_whose_value_mismatches_its_declared_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const int BAD = "not an int";
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.value" && x.message.contains("Foo::BAD")),
        "got: {d:?}"
    );
}

#[test]
fn a_typed_constant_whose_value_matches_its_declared_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const int GOOD = 1;
                const string ALSO_GOOD = "hi";
                const array LIST_OK = [1, 2, 3];
                const ?int NULLABLE_OK = null;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.value"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_untyped_constant_is_never_flagged_by_this_check() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const BAD = "any value is fine here";
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.value"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_typed_enum_constant_whose_value_mismatches_its_declared_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            enum Status {
                case Active;
                const int BAD = "not an int";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.value"),
        "got: {d:?}"
    );
}

#[test]
fn a_typed_constant_assigned_an_unresolved_global_constant_is_never_flagged() {
    // `DEBUG_BACKTRACE_IGNORE_ARGS` (a real PHP builtin int constant
    // this project has no global-constant table for) reads as `Mixed`,
    // not a genuinely `mixed`-typed value - don't guess wrong on it, the
    // same leniency established elsewhere.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private const int OPTIONS = DEBUG_BACKTRACE_IGNORE_ARGS;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.value"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_typed_constant_referencing_another_constant_is_checked_via_full_inference() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                const int BASE = 1;
                const int DERIVED = self::BASE;
                const string WRONG = self::BASE;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.value" && x.message.contains("DERIVED")),
        "unexpected diagnostics: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.value" && x.message.contains("WRONG")),
        "got: {d:?}"
    );
}

// `final const`/typed-constant PHP-version gating, plus `final private
// const`'s own unconditional modifier conflict.

#[test]
fn a_final_constant_below_php_8_1_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                final const X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.finalNotSupported"),
        "got: {d:?}"
    );
}

#[test]
fn a_final_constant_on_php_8_1_or_later_is_never_flagged_for_version_support() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                final const X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.finalNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_final_constant_with_no_configured_php_version_is_never_flagged_for_version_support() {
    // With no configured version, this project has nothing to check
    // against - matches every other `PhpFeature`-gated rule's own
    // "unset means unconfigured, stay silent" convention.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final const X = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.finalNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_final_private_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final private const X = 1;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.finalPrivate"),
        "got: {d:?}"
    );
}

#[test]
fn a_final_public_constant_is_never_flagged_as_final_private() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                final public const X = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.finalPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_non_final_private_constant_is_never_flagged_as_final_private() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private const X = 1;
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.finalPrivate"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_typed_constant_below_php_8_3_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                const int X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 2)),
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "classConstant.nativeTypeNotSupported"),
        "got: {d:?}"
    );
}

#[test]
fn a_typed_constant_on_php_8_3_or_later_is_never_flagged_for_version_support() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                const int X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 3)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.nativeTypeNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_untyped_constant_is_never_flagged_for_native_type_version_support() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                const X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "classConstant.nativeTypeNotSupported"),
        "unexpected diagnostics: {d:?}"
    );
}

// A trait's own `const` declaration (`ConstantsInTraitsRule`) needs PHP
// 8.2+.

#[test]
fn a_constant_declared_inside_a_trait_below_php_8_2_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            trait Foo {
                const X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "classConstant.inTrait"),
        "got: {d:?}"
    );
}

#[test]
fn a_constant_declared_inside_a_trait_on_php_8_2_or_later_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            trait Foo {
                const X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 2)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.inTrait"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_constant_declared_inside_a_trait_with_no_configured_php_version_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait Foo {
                const X = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.inTrait"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_constant_declared_inside_a_class_is_never_flagged_as_in_trait() {
    let d = diagnostics_for_with(
        r#"<?php
            class Foo {
                const X = 1;
            }
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.inTrait"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn self_referencing_a_trait_constant_from_inside_the_trait_is_never_flagged_notfound() {
    // A used trait's own constants weren't merged into a using class's
    // registry entry at all (only methods/properties were) - `self::`
    // inside the trait's own method body, checked in the host's context
    // (`check_trait_methods` runs each trait method once per host), had
    // nowhere to resolve the constant against, since it was never
    // reachable via the host's own `consts` map. Real shape: PHP 8.2+
    // trait constants, referenced via `self::` from a method physically
    // declared inside the trait, on two separate hosts.
    let d = diagnostics_for(
        r#"<?php
            trait Walker {
                private const string STOP = "stop";
                public function walk(): string {
                    return self::STOP;
                }
            }
            class HostA {
                use Walker;
            }
            class HostB {
                use Walker;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "classConstant.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}
