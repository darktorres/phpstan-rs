//! Phase 43: PHP 8.4 property hooks - virtual-property detection,
//! `propertyGetHook.noRead`/`propertySetHook.noAssign`,
//! `propertySetHook.nativeParameterType` contravariance,
//! `class.notFound` existence-checking for a set hook's own parameter
//! type, real function-like body checking (undefined variables,
//! `return.missing`, generator-return-type compatibility), and
//! `PropertyVarianceRule` for a property's own single type.

use super::helpers::*;

#[test]
fn a_get_hook_that_reads_its_own_backing_storage_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Temperature {
                private float $celsius = 0.0;
                public float $fahrenheit {
                    get => $this->celsius * 9 / 5 + 32;
                    set(float $value) {
                        $this->celsius = ($value - 32) * 5 / 9;
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_get_hook_that_never_reads_its_own_backing_storage_is_flagged() {
    // The set hook DOES reference `$this->y`, so the property is not
    // virtual (see the module-level empirical note in ROADMAP.md's Phase
    // 43 entry - either hook referencing it is enough) - real PHPStan's
    // own `GetNonVirtualPropertyHookReadRule` fires here.
    let d = diagnostics_for(
        r#"<?php
            class BadGet {
                public string $y {
                    get => "constant";
                    set(string $value) {
                        $this->y = $value;
                    }
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "propertyGetHook.noRead"));
}

#[test]
fn a_set_hook_that_never_assigns_its_own_backing_storage_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class BadSet {
                public string $z {
                    get => $this->z;
                    set(string $value) {
                        echo $value;
                    }
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "propertySetHook.noAssign"));
}

#[test]
fn a_fully_virtual_property_gets_neither_no_read_nor_no_assign() {
    // Neither hook references `$this->name` at all - real PHP makes this
    // a virtual property (no backing storage allocated), which the two
    // non-virtual-hook rules deliberately don't apply to at all.
    let d = diagnostics_for(
        r#"<?php
            class FullyVirtual {
                public string $name {
                    get => "constant";
                    set(string $value) {
                        echo $value;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "propertyGetHook.noRead"
            || x.identifier == "propertySetHook.noAssign"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_reference_inside_a_nested_closure_does_not_count_as_writing_backing_storage() {
    // Empirically verified against real PHP 8.5's own
    // `ReflectionProperty::isVirtual()` (see ROADMAP.md's Phase 43
    // entry) - a nested closure/arrow-fn is its own function scope, not
    // counted by real PHP's own virtual-property algorithm either. The
    // `get` hook reads `$this->k` directly, so the property is genuinely
    // non-virtual (this rule's gate) - isolating whether `set`'s own
    // nested-closure reference is (wrongly) counted as a real assignment.
    let d = diagnostics_for(
        r#"<?php
            class NestedClosureWrite {
                public string $k {
                    get => $this->k;
                    set(string $value) {
                        (function () use ($value) {
                            $this->k = $value;
                        })();
                    }
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "propertySetHook.noAssign"));
}

#[test]
fn a_bare_property_fetch_statement_with_no_use_still_counts_as_a_reference() {
    // Empirically verified: `$this->o;` as its own unused statement still
    // makes the property non-virtual in real PHP - purely syntactic, not
    // about whether the value is actually used.
    let d = diagnostics_for(
        r#"<?php
            class BareRead {
                public string $o {
                    get {
                        $this->o;
                        return "x";
                    }
                    set(string $value) {}
                }
            }
        "#,
    );
    assert!(!d.iter().any(|x| x.identifier == "propertyGetHook.noRead"));
}

#[test]
fn set_hook_parameter_missing_a_native_type_the_property_has_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class MissingParamType {
                public string $w {
                    set($value) {
                        echo $value;
                    }
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "propertySetHook.nativeParameterType")
    );
}

#[test]
fn set_hook_parameter_type_not_contravariant_with_the_property_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class ContravarianceBad {
                public string $w {
                    set(int $value) {
                        echo $value;
                    }
                }
            }
        "#,
    );
    let hit = d
        .iter()
        .find(|x| x.identifier == "propertySetHook.nativeParameterType");
    assert!(hit.is_some(), "unexpected diagnostics: {d:?}");
    assert!(hit.unwrap().message.contains("not contravariant"));
}

#[test]
fn set_hook_parameter_wider_than_the_property_type_is_not_flagged() {
    // Real PHP: an explicit `set(Type $value)` may legally accept a
    // WIDER type than the property's own (the hook does its own
    // coercion inside the body before the real assignment) - only a
    // narrower-than-property param type is a contravariance violation.
    let d = diagnostics_for(
        r#"<?php
            class WiderParamOk {
                public int $w {
                    set(int|string $value) {
                        echo $value;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "propertySetHook.nativeParameterType"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn set_hook_parameter_type_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class UnknownParamType {
                public ?Missing $w {
                    set(?Missing $value) {}
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "class.notFound"));
}

#[test]
fn get_hook_body_is_checked_as_a_real_function_scope() {
    // Confirms the "no function-like registry entry to check hook
    // bodies against" gap is closed - an ordinary undefined-variable
    // read inside a get hook's block body is now caught, the same way
    // it would be inside any other method body.
    let d = diagnostics_for(
        r#"<?php
            class UndefinedInHook {
                public string $u {
                    get {
                        return $undefinedVar;
                    }
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "variable.undefined"));
}

#[test]
fn get_hook_block_body_falling_off_the_end_is_a_missing_return() {
    let d = diagnostics_for(
        r#"<?php
            class MissingReturn {
                public string $m {
                    get {
                        if (true) {
                            $x = $this->m;
                        }
                    }
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "return.missing"));
}

#[test]
fn get_hook_returning_the_wrong_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class WrongReturnType {
                public string $r {
                    get => 123;
                }
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "return.type"));
}

#[test]
fn set_hook_body_can_legally_access_a_private_sibling_property_on_the_same_class() {
    // Regression test for a real bug found while implementing this
    // phase: `check_get_hook`/`check_set_hook` initially never saved/set/
    // restored `Checker::current_class`, so an entirely legal same-class
    // private property access from inside a hook body was wrongly
    // flagged `property.private` (visibility checks read `current_class`
    // to allow it).
    let d = diagnostics_for(
        r#"<?php
            class SameClassPrivateAccess {
                private float $celsius = 0.0;
                public float $fahrenheit {
                    get => $this->celsius;
                    set(float $value) {
                        $this->celsius = $value;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.private"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_readonly_property_with_a_covariant_template_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class Box {
                /**
                 * @var T
                 */
                public readonly mixed $ro;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generics.variance"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_plain_readwrite_property_with_a_covariant_template_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class Box {
                /**
                 * @var T
                 */
                public mixed $value;
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "generics.variance"));
}

#[test]
fn a_get_only_hooked_property_is_treated_as_restricted_write_access_for_variance() {
    // Real PHPStan's own `hasRestrictedWriteAccess`: a property with
    // hooks where none of them is `set` can never be written through at
    // all, so a covariant template there is exactly as sound as a
    // readonly property's is.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class Box {
                /**
                 * @var T
                 */
                public mixed $value {
                    get => $this->value;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generics.variance"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_promoted_property_get_hook_that_never_reads_backing_storage_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class BadGetPromoted {
                public function __construct(
                    public string $y {
                        get => "constant";
                        set(string $value) { $this->y = $value; }
                    }
                ) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "propertyGetHook.noRead"),
        "got: {d:?}"
    );
}

#[test]
fn a_promoted_property_set_hook_that_never_assigns_backing_storage_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class BadSetPromoted {
                public function __construct(
                    public string $z {
                        get => $this->z;
                        set(string $value) { echo $value; }
                    }
                ) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "propertySetHook.noAssign"),
        "got: {d:?}"
    );
}

#[test]
fn a_promoted_property_fully_virtual_hooks_are_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class FullyVirtualPromoted {
                public function __construct(
                    public string $name {
                        get => "constant";
                        set(string $value) { echo $value; }
                    }
                ) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "propertyGetHook.noRead"
            || x.identifier == "propertySetHook.noAssign"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_promoted_property_set_hook_parameter_missing_native_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class PromotedNativeMismatch {
                public function __construct(
                    public string $w {
                        set($value) { $this->w = $value; }
                    }
                ) {}
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "propertySetHook.nativeParameterType"),
        "got: {d:?}"
    );
}

#[test]
fn a_promoted_property_get_hook_body_is_checked_as_a_real_function_scope() {
    let d = diagnostics_for(
        r#"<?php
            class PromotedBodyChecked {
                public function __construct(
                    public string $v {
                        get => $undefinedVar;
                    }
                ) {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined variable")),
        "got: {d:?}"
    );
}

#[test]
fn a_promoted_readonly_property_with_a_covariant_template_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class BoxPromoted {
                /**
                 * @param T $value
                 */
                public function __construct(
                    public readonly mixed $value,
                ) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "generics.variance"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_promoted_plain_readwrite_property_with_a_covariant_template_type_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            class BoxPromotedMutable {
                /**
                 * @param T $value
                 */
                public function __construct(
                    public mixed $value,
                ) {}
            }
        "#,
    );
    assert!(d.iter().any(|x| x.identifier == "generics.variance"));
}

// Phase 64 Item 1: property hooks - `notReadable`/`notWritable`/
// `abstractOverridingNonAbstractHook`. Confirmed live via `php -r`: the
// engine itself raises no error for any of these - a write through a
// `notWritable` override silently no-ops instead (real footgun this
// check exists to catch statically).

#[test]
fn overriding_a_readable_hooked_property_with_a_set_only_hook_is_flagged_not_readable() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                private int $backing = 0;
                public int $x {
                    get => $this->backing;
                    set(int $v) { $this->backing = $v; }
                }
            }
            class Child extends Base {
                public int $x {
                    set(int $v) {}
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.notReadable"),
        "got: {d:?}"
    );
}

#[test]
fn overriding_a_writable_hooked_property_with_a_get_only_hook_is_flagged_not_writable() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                private int $backing = 0;
                public int $x {
                    get => $this->backing;
                    set(int $v) { $this->backing = $v; }
                }
            }
            class Child extends Base {
                public int $x {
                    get => 5;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.notWritable"),
        "got: {d:?}"
    );
}

#[test]
fn overriding_a_fully_hooked_property_with_the_same_hooks_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                private int $backing = 0;
                public int $x {
                    get => $this->backing;
                    set(int $v) { $this->backing = $v; }
                }
            }
            class Child extends Base {
                private int $backing2 = 0;
                public int $x {
                    get => $this->backing2;
                    set(int $v) { $this->backing2 = $v; }
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "property.notReadable"
                || x.identifier == "property.notWritable"),
        "got: {d:?}"
    );
}

#[test]
fn making_a_non_abstract_hook_abstract_on_override_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                private int $backing = 0;
                public int $x {
                    get => $this->backing;
                }
            }
            abstract class Child extends Base {
                abstract public int $x { get; }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "property.abstractOverridingNonAbstractHook"),
        "got: {d:?}"
    );
}

#[test]
fn an_abstract_hook_overriding_an_ancestors_own_abstract_hook_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Base {
                abstract public int $x { get; }
            }
            abstract class Child extends Base {
                abstract public int $x { get; }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "property.abstractOverridingNonAbstractHook"),
        "got: {d:?}"
    );
}

#[test]
fn a_plain_non_hooked_property_override_is_never_checked_for_hook_readability() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public int $x = 1; }
            class Child extends Base { public int $x = 2; }
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "property.notReadable"
                | "property.notWritable"
                | "property.abstractOverridingNonAbstractHook"
        )),
        "got: {d:?}"
    );
}
