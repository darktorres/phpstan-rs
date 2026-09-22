//! Property declaration legality: interface/class hook and modifier
//! rules, native and PHPDoc `readonly`, and `callable` native types.

use super::helpers::*;

fn findings<'d>(d: &'d [crate::Diagnostic], prefix: &str) -> Vec<(&'d str, &'d str)> {
    d.iter()
        .filter(|x| x.identifier.starts_with(prefix))
        .filter(|x| {
            !matches!(
                x.identifier.as_str(),
                "property.unused" | "property.onlyWritten"
            )
        })
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect()
}

#[test]
fn interface_property_rules() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {
                public int $plain;
                protected int $hidden { get; }
                public int $ok { get; }
                public readonly int $ro { get; }
                public static int $st { get; }
                public final int $fin { get; }
                public int $body { get => 1; }
                public int $finalHook { final get; }
            }
        "#,
    );
    let ids: Vec<_> = findings(&d, "property.")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "property.nonHookedInInterface",
            "property.nonPublicInInterface",
            "property.readOnlyInInterface",
            "property.hookedStatic",
            "property.finalInInterface",
            "property.hookBodyInInterface",
            "property.abstractFinalHook",
        ],
        "got: {d:?}"
    );
}

#[test]
fn interface_properties_are_gated_on_php_8_4() {
    let d = diagnostics_for_with(
        r#"<?php
            interface Shape { public int $ok { get; } }
        "#,
        |r| r.php_version = Some((8, 3)),
    );
    assert_eq!(
        findings(&d, "property.inInterface"),
        vec![(
            "property.inInterface",
            "Interfaces can include properties only on PHP 8.4 and later."
        )]
    );
}

#[test]
fn class_property_modifier_rules() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Base {
                abstract public int $noHooks;
                abstract public int $allBodies { get => 1; }
                public int $bodiless { get; }
                abstract private int $absPriv { get; }
                final private int $finPriv = 1;
                private int $finHook { final get => 1; }
                abstract public int $absFinalHook { final get; }
                public readonly int $roHooked { get => 1; }
                public static int $staticHooked { get => 1; }
                public int $virtual = 3 { get => 1; }
                public ?int $fine { get => $this->fine; set => $value; }
            }
            class Concrete {
                abstract public int $abs { get; }
            }
        "#,
    );
    let ids: Vec<_> = findings(&d, "property.")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "property.abstractNonHooked",
            "property.abstractWithoutAbstractHook",
            "property.hookWithoutBody",
            "property.abstractPrivate",
            "property.finalPrivate",
            "property.finalPrivateHook",
            "property.abstractFinal",
            "property.hookReadOnly",
            "property.hookedStatic",
            "property.virtualDefault",
            "property.abstract",
        ],
        "got: {d:?}"
    );
}

#[test]
fn class_property_version_gates() {
    let src = r#"<?php
        class Gated {
            final public int $a = 1;
            public int $b { get => 1; }
            public static private(set) int $c = 1;
        }
    "#;
    let ids = |v| {
        diagnostics_for_with(src, |r| r.php_version = Some(v))
            .into_iter()
            .filter(|x| {
                matches!(
                    x.identifier.as_str(),
                    "property.final"
                        | "property.hooksNotSupported"
                        | "property.staticAsymmetricVisibility"
                )
            })
            .map(|x| x.identifier)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids((8, 3)),
        vec![
            "property.final",
            "property.hooksNotSupported",
            "property.staticAsymmetricVisibility",
        ]
    );
    assert_eq!(ids((8, 4)), vec!["property.staticAsymmetricVisibility"]);
    assert!(ids((8, 5)).is_empty());
}

#[test]
fn readonly_property_declaration_rules() {
    let d = diagnostics_for(
        r#"<?php
            class Ro {
                public readonly $untyped;
                public readonly int $defaulted = 1;
                public static readonly int $st;
                public function __construct(public readonly $promotedUntyped) {}
            }
            class ByDoc {
                /** @readonly */
                public int $withDefault = 1;
                /** @phpstan-readonly-allow-private-mutation */
                public int $allowed = 1;
                /** @readonly */
                public int $noDefault;
            }
        "#,
    );
    assert_eq!(
        findings(&d, "property.readOnly"),
        vec![
            (
                "property.readOnlyNoNativeType",
                "Readonly property must have a native type."
            ),
            (
                "property.readOnlyDefaultValue",
                "Readonly property cannot have a default value."
            ),
            (
                "property.readOnlyStatic",
                "Readonly property cannot be static."
            ),
            (
                "property.readOnlyNoNativeType",
                "Readonly property must have a native type."
            ),
            (
                "property.readOnlyByPhpDocDefaultValue",
                "@readonly property cannot have a default value."
            ),
        ],
        "got: {d:?}"
    );
}

#[test]
fn readonly_properties_are_gated_on_php_8_1() {
    let d = diagnostics_for_with(
        r#"<?php
            class Ro { public readonly int $a; }
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert_eq!(
        findings(&d, "property.readOnlyNotSupported"),
        vec![(
            "property.readOnlyNotSupported",
            "Readonly properties are supported only on PHP 8.1 and later."
        )]
    );
}

#[test]
fn callable_native_property_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Holder {
                public callable $a;
                public ?callable $b;
                public \Closure|callable $c;
                public \Closure $ok;
                public function __construct(private callable $d) {}
            }
        "#,
    );
    let found: Vec<_> = findings(&d, "property.callableType")
        .into_iter()
        .map(|(_, m)| m)
        .collect();
    assert_eq!(
        found,
        vec![
            "Property Holder::$a cannot have callable in its type declaration.",
            "Property Holder::$b cannot have callable in its type declaration.",
            "Property Holder::$c cannot have callable in its type declaration.",
            "Property Holder::$d cannot have callable in its type declaration.",
        ],
        "got: {d:?}"
    );
}

#[test]
fn nested_class_declarations_get_declaration_checks() {
    let d = diagnostics_for(
        r#"<?php
            interface Shape {}
            if (!class_exists('Polyfilled')) {
                class Polyfilled extends Shape {
                    public callable $cb;
                }
            }
        "#,
    );
    let ids: Vec<_> = d.iter().map(|x| x.identifier.as_str()).collect();
    assert!(ids.contains(&"class.extendsInterface"), "got: {d:?}");
    assert!(ids.contains(&"property.callableType"), "got: {d:?}");
}

#[test]
fn unresolvable_native_property_type_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            final class Foo {}
            final class Bar {}
            class Impossible {
                public Foo&Bar $a;
                public \Countable&\Iterator $ok;
                public function __construct(public Bar&Foo $b) {}
            }
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    let found: Vec<_> = findings(&d, "property.unresolvableNativeType")
        .into_iter()
        .map(|(_, m)| m)
        .collect();
    assert_eq!(
        found,
        vec![
            "Property Impossible::$a has unresolvable native type.",
            "Property Impossible::$b has unresolvable native type.",
        ],
        "got: {d:?}"
    );
}

#[test]
fn reading_a_write_only_and_writing_a_read_only_property_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property-read int $readOnlyTag
             * @property-write int $writeOnlyTag
             */
            class Magic {
                public function __get($n) { return 1; }
                public function __set($n, $v) {}
                public int $getOnly { get => 1; }
                public int $setOnly { set { echo $value; } }
            }
            function f(Magic $m): void {
                $m->readOnlyTag = 1;
                echo $m->writeOnlyTag;
                $m->getOnly = 2;
                echo $m->setOnly;
                echo $m->readOnlyTag;
                $m->writeOnlyTag = 3;
            }
        "#,
    );
    let found = |id| {
        d.iter()
            .filter(|x| x.identifier == id)
            .map(|x| x.message.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        found("assign.propertyReadOnly"),
        vec![
            "Property Magic::$readOnlyTag is not writable.",
            "Property Magic::$getOnly is not writable.",
        ],
        "got: {d:?}"
    );
    assert_eq!(
        found("property.writeOnly"),
        vec![
            "Property Magic::$writeOnlyTag is not readable.",
            "Property Magic::$setOnly is not readable.",
        ],
        "got: {d:?}"
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "property.readOnlyAssignNotInConstructor"),
        "got: {d:?}"
    );
}

#[test]
fn taking_a_reference_to_a_protected_or_readonly_property_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Refs {
                public private(set) int $asym = 0;
                protected int $prot = 0;
                public readonly int $ro;
                /** @readonly */
                public int $docRo = 0;
                /** @readonly */
                public static int $docStatic = 0;
                public int $open = 0;
                public function __construct() { $this->ro = 1; }
                public function inside(): void {
                    $a = &$this->ro;
                    $b = &$this->docRo;
                    $c = &self::$docStatic;
                    $d = &$this->asym;
                }
            }
            function f(Refs $r): void {
                $a = &$r->asym;
                $b = &$r->prot;
                $c = &$r->open;
            }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier.ends_with("ssignByRef"))
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                "property.readOnlyAssignByRef",
                "Readonly property Refs::$ro is assigned by reference."
            ),
            (
                "property.readOnlyByPhpDocAssignByRef",
                "@readonly property Refs::$docRo is assigned by reference."
            ),
            (
                "property.readOnlyByPhpDocAssignByRef",
                "@readonly property Refs::$docStatic is assigned by reference."
            ),
            (
                "property.assignByRef",
                "Property Refs::$asym with private(set) visibility is assigned by reference."
            ),
            (
                "property.assignByRef",
                "Property Refs::$prot with protected visibility is assigned by reference."
            ),
        ],
        "got: {d:?}"
    );
}
