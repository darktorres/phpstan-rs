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

#[test]
fn readonly_class_is_gated_on_php_8_2_and_anonymous_on_8_3() {
    let src = r#"<?php
        readonly class Frozen {}
        function make(): object {
            return new readonly class {};
        }
    "#;
    let gate = |v| {
        let d = diagnostics_for_with(src, |r| r.php_version = Some(v));
        d.into_iter()
            .filter(|x| x.identifier == "classConstant.nativeTypeNotSupported")
            .map(|x| x.message)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        gate((8, 1)),
        vec![
            "Readonly classes are supported only on PHP 8.2 and later.",
            "Anonymous readonly classes are supported only on PHP 8.3 and later.",
        ]
    );
    assert_eq!(
        gate((8, 2)),
        vec!["Anonymous readonly classes are supported only on PHP 8.3 and later."]
    );
    assert!(gate((8, 3)).is_empty());
}

#[test]
fn serializable_class_without_magic_serialization_methods_is_flagged() {
    let src = r#"<?php
        class OldSchool implements Serializable {
            public function serialize() { return ''; }
            public function unserialize($data) {}
        }
        class Modern implements Serializable {
            public function serialize() { return ''; }
            public function unserialize($data) {}
            public function __serialize(): array { return []; }
            public function __unserialize(array $data): void {}
        }
        class ModernChild extends Modern {}
        abstract class AbstractOld implements Serializable {}
    "#;
    let found = |v| {
        diagnostics_for_with(src, |r| r.php_version = Some(v))
            .into_iter()
            .filter(|x| x.identifier == "class.serializable")
            .map(|x| x.message)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        found((8, 1)),
        vec![
            "non-abstract class OldSchool implements the Serializable interface, but does not implement __serialize()",
            "non-abstract class OldSchool implements the Serializable interface, but does not implement __unserialize()",
        ]
    );
    assert!(found((8, 0)).is_empty());
}

#[test]
fn class_constant_overrides_must_stay_compatible() {
    let d = diagnostics_for(
        r#"<?php
            interface HasLimit {
                const LIMIT = 10;
                final const FIXED = 1;
            }
            class Base {
                public const OPEN = 'a';
                protected const GUARDED = 'b';
                private const HIDDEN = 'c';
                const int TYPED = 1;
                /** @var positive-int */
                const DOC = 5;
            }
            class Child extends Base implements HasLimit {
                const LIMIT = 20;
                const FIXED = 2;
                protected const OPEN = 'x';
                private const GUARDED = 'y';
                private const HIDDEN = 'z';
                const TYPED = 2;
                /** @var int */
                const DOC = -1;
            }
            class TypedChild extends Base {
                const string TYPED = 'no';
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| {
            x.identifier.starts_with("classConstant.") && x.identifier != "classConstant.unused"
        })
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                "classConstant.final",
                "Constant Child::FIXED overrides final constant HasLimit::FIXED."
            ),
            (
                "classConstant.visibility",
                "Protected constant Child::OPEN overriding public constant Base::OPEN should also be public."
            ),
            (
                "classConstant.visibility",
                "Private constant Child::GUARDED overriding protected constant Base::GUARDED should be protected or public."
            ),
            (
                "classConstant.missingNativeType",
                "Constant Child::TYPED overriding constant Base::TYPED (int) should also have native type int."
            ),
            (
                "classConstant.type",
                "Type int of constant Child::DOC is not covariant with type int<1, max> of constant Base::DOC."
            ),
            (
                "classConstant.nativeType",
                "Native type string of constant TypedChild::TYPED is not covariant with native type int of constant Base::TYPED."
            ),
        ],
        "got: {d:?}"
    );
}

#[test]
fn a_constant_var_tag_must_describe_its_value() {
    let d = diagnostics_for(
        r#"<?php
            class Conf {
                /** @var positive-int */
                const BAD = -1;
                /** @var positive-int */
                const GOOD = 5;
                /** @var list<string> */
                const NAMES = ['a', 'b'];
                /** @var non-empty-string */
                const string TYPED = '';
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| {
            x.identifier == "classConstant.phpDocType" || x.identifier == "classConstant.value"
        })
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                "classConstant.phpDocType",
                "PHPDoc tag @var for constant Conf::BAD with type int<1, max> is incompatible with value -1."
            ),
            (
                "classConstant.value",
                "constant Conf::TYPED (non-empty-string) does not accept value ''"
            ),
        ],
        "got: {d:?}"
    );
}

#[test]
fn dynamic_class_constant_fetches_are_version_gated_and_need_a_string_name() {
    let src = r#"<?php
        class Box { const A = 1; }
        function f(string $s, int $i, int|string $m): void {
            echo Box::{$s};
            echo Box::{$i};
            echo Box::{$m};
            echo Box::{$undefinedName};
        }
    "#;
    let found = |v| {
        diagnostics_for_with(src, |r| r.php_version = Some(v))
            .into_iter()
            .filter(|x| {
                x.identifier.starts_with("classConstant.") || x.identifier == "variable.undefined"
            })
            .map(|x| (x.identifier, x.level))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        found((8, 3)),
        vec![
            ("classConstant.nameType".to_string(), 0),
            ("classConstant.nameType".to_string(), 7),
            ("variable.undefined".to_string(), 0),
        ]
    );
    assert_eq!(
        found((8, 2))
            .iter()
            .filter(|(id, _)| id == "classConstant.dynamicFetch")
            .count(),
        4
    );
}
