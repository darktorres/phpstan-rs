//! Phase 62: `property.uninitialized` - a typed, no-default property
//! that nothing anywhere in the class body ever assigns. Deliberately
//! permissive (see `uninitialized.rs`'s own doc comment): "assigned
//! *somewhere*, by *any* method, however conditionally" is enough to
//! clear it - a real corpus false positive (amphp's own deferred-init
//! `Http1Driver::$client`, assigned only in a later `handleClient()`
//! call, not the constructor) is exactly why.

use super::helpers::*;

/// This whole family is off by default (`Registry::check_uninitialized_
/// properties`, mirroring real PHPStan's own opt-in
/// `checkUninitializedProperties`) - every test here needs it turned on
/// to see anything.
fn diagnostics_for(src: &str) -> Vec<super::super::Diagnostic> {
    diagnostics_for_with(src, |r| r.check_uninitialized_properties = true)
}

#[test]
fn a_typed_property_with_no_constructor_at_all_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

#[test]
fn a_typed_property_no_method_anywhere_assigns_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    echo "hi";
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

#[test]
fn a_typed_property_the_constructor_unconditionally_assigns_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->x = 5;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_only_assigned_inside_a_conditional_branch_is_not_flagged() {
    // Deliberately permissive - see this module's own doc comment for
    // why a real CFG guarantee isn't required here.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct(bool $c) {
                    if ($c) {
                        $this->x = 1;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_assigned_only_in_a_switch_case_with_no_default_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct(int $c) {
                    switch ($c) {
                        case 1:
                            $this->x = 1;
                            break;
                    }
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_assigned_only_inside_a_try_body_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    try {
                        $this->x = riskyCompute();
                    } catch (\Exception $e) {
                    }
                }
            }
            function riskyCompute(): int { return 1; }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

// Real corpus find (amphp's own `Http1Driver::$client`): a property
// meant to be set by a separate method called sometime *after*
// construction, not the constructor itself - a real, common "deferred/
// pooled-object initialization" idiom, confirmed genuinely *not*
// flagged by real PHPStan itself (`vendor/bin/phpstan` directly on that
// file). The whole reason this check settles for "assigned anywhere in
// the class, by any method" instead of "guaranteed by the constructor."
#[test]
fn a_property_assigned_only_by_a_separate_non_constructor_method_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                }
                public function handle(int $value): void {
                    $this->x = $value;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

// Real corpus find (Symfony's own `SymfonyStyle::$questionHelper`,
// touched nowhere except this exact shape): `??=`'s own left-hand check
// is `isset()`-based, not a raw read - and `isset()` is specially
// exempted from the "must not be accessed before initialization" error
// a raw read of an uninitialized typed property throws, unlike every
// *other* compound operator (confirmed directly against real `php`).
#[test]
fn a_property_only_ever_touched_via_coalesce_assign_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private string $x;
                public function get(): string {
                    $this->x ??= "default";
                    return $this->x;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_only_ever_touched_via_a_non_coalesce_compound_assign_is_still_flagged() {
    // The exemption is specific to `??=` - `+=`/`.=`/etc. all genuinely
    // require a real prior value and would themselves be the fatal
    // error on a still-uninitialized property, not a valid initializer.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->x += 1;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

// Real corpus find (Latte's own `IncludeFileNode::create()`): a static
// "named constructor" populates a fresh instance held in a *local*
// variable, not `$this` - a real, common PHP OOP idiom.
#[test]
fn a_property_assigned_through_a_local_var_holding_a_fresh_instance_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Node {
                public int $x;
                public static function create(int $val): static {
                    $node = new static;
                    $node->x = $val;
                    return $node;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

// Real corpus find (Latte's own `IterateWhileNode::create()`): the
// fresh instance is held via a *chained* assignment (`$node = $tag->node
// = new static;`), not a bare `$node = new static;` - the right-hand
// side of the outer assign is itself another `Assign`, not directly a
// `New`.
#[test]
fn a_property_assigned_through_a_chained_assignment_to_a_fresh_instance_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Tag { public $node; }
            class Node {
                public bool $postTest;
                public static function create(Tag $tag) {
                    $node = $tag->node = new static;
                    $node->postTest = true;
                    return $node;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_assigned_through_an_unrelated_local_var_of_a_different_class_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Other {
                public int $x = 0;
            }
            class Node {
                public int $x;
                public function f(): void {
                    $other = new Other();
                    $other->x = 5;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

#[test]
fn a_property_with_a_default_value_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x = 0;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_untyped_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_nullable_typed_property_with_no_default_is_still_flagged() {
    // Real PHP semantics: a *typed* property, nullable or not, is never
    // implicitly defaulted - only an untyped one (or an explicit
    // `= null`) is.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public ?int $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}

#[test]
fn a_readonly_property_is_never_flagged_by_this_rule() {
    // Excluded on purpose, matching real PHPStan's own
    // UninitializedPropertyRule - readonly's own "assign exactly once,
    // only from the constructor" contract is a separate, already-
    // implemented check with its own failure mode.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public readonly int $x;
                public function __construct() {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_promoted_constructor_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public function __construct(public int $x) {}
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn an_abstract_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Foo {
                public int $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_static_property_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public static int $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.uninitialized"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_never_assigned_anywhere_in_the_class_is_flagged_even_with_other_methods() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                public int $x;
                public function __construct() {}
                public function unrelated(): void {
                    echo "hi";
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.uninitialized"),
        "got: {d:?}"
    );
}
