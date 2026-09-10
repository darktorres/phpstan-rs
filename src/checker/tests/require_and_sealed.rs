//! Tests for `@phpstan-require-extends`/`@phpstan-require-implements`
//! (`checker::oop::check_require_tags`) and `@phpstan-sealed`
//! (`checker::oop::check_sealed_hierarchy`).
use super::helpers::*;

#[test]
fn a_class_implementing_a_require_extends_interface_without_extending_its_base_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            /** @phpstan-require-extends Base */
            interface Marker {}
            class Bad implements Marker {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "class.missingExtends");
    assert!(
        d[0].message
            .contains("Interface Marker requires implementing class to extend Base"),
        "got: {d:?}"
    );
}

#[test]
fn a_class_extending_the_required_base_satisfies_require_extends() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            /** @phpstan-require-extends Base */
            interface Marker {}
            class Good extends Base implements Marker {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_trait_require_extends_is_checked_against_the_using_class() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            /** @phpstan-require-extends Base */
            trait Helper {}
            class Bad { use Helper; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "class.missingExtends");
    assert!(
        d[0].message
            .contains("Trait Helper requires using class to extend Base"),
        "got: {d:?}"
    );
}

#[test]
fn require_extends_is_checked_transitively_through_an_intermediate_interface() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            /** @phpstan-require-extends Base */
            interface Marker {}
            interface SubMarker extends Marker {}
            class Bad implements SubMarker {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "class.missingExtends");
}

#[test]
fn a_class_using_a_trait_without_the_required_interface_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            interface Contract {}
            /** @phpstan-require-implements Contract */
            trait Helper {}
            class Bad { use Helper; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "class.missingImplements");
    assert!(
        d[0].message
            .contains("Trait Helper requires using class to implement Contract"),
        "got: {d:?}"
    );
}

#[test]
fn a_class_implementing_the_required_interface_satisfies_require_implements() {
    let d = diagnostics_for(
        r#"<?php
            interface Contract {}
            /** @phpstan-require-implements Contract */
            trait Helper {}
            class Good implements Contract { use Helper; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn require_implements_is_satisfied_via_a_trait_used_only_by_another_trait() {
    let d = diagnostics_for(
        r#"<?php
            interface Contract {}
            /** @phpstan-require-implements Contract */
            trait Inner {}
            trait Outer { use Inner; }
            class Good implements Contract { use Outer; }
            class Bad { use Outer; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].location.contains("Bad"), "got: {d:?}");
}

#[test]
fn a_class_extending_outside_a_sealed_list_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-sealed ChildA|ChildB */
            abstract class Shape {}
            class ChildA extends Shape {}
            class ChildB extends Shape {}
            class ChildC extends Shape {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "class.disallowedSubtype");
    assert!(
        d[0].message
            .contains("Type ChildC is not allowed to be a subtype of Shape"),
        "got: {d:?}"
    );
}

#[test]
fn classes_listed_in_the_sealed_allowlist_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-sealed ChildA|ChildB */
            abstract class Shape {}
            class ChildA extends Shape {}
            class ChildB extends Shape {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sealed_applies_one_level_only_not_transitively() {
    // GrandChild's own immediate parent is ChildA, which isn't itself
    // sealed - Shape's own sealed list is irrelevant at this level.
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-sealed ChildA */
            abstract class Shape {}
            class ChildA extends Shape {}
            class GrandChild extends ChildA {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sealed_on_an_interface_is_enforced_against_implementors() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-sealed Allowed */
            interface Sealed {}
            class Allowed implements Sealed {}
            class NotAllowed implements Sealed {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "class.disallowedSubtype");
    assert!(d[0].location.contains("NotAllowed"), "got: {d:?}");
}

#[test]
fn an_untagged_hierarchy_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            abstract class Shape {}
            class ChildA extends Shape {}
            class ChildB extends Shape {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 48.7: `@phpstan-require-extends`/`@phpstan-require-implements`/
// `@phpstan-sealed`'s own existence half - the RHS class name itself.

#[test]
fn a_require_extends_tag_naming_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-require-extends NoSuchBase */
            interface Marker {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message.contains(
                "PHPDoc tag @phpstan-require-extends contains unknown class NoSuchBase."
            )),
        "got: {d:?}"
    );
}

#[test]
fn a_require_extends_tag_naming_a_known_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            /** @phpstan-require-extends Base */
            interface Marker {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_require_implements_tag_naming_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-require-implements NoSuchInterface */
            trait T {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message.contains(
                "PHPDoc tag @phpstan-require-implements contains unknown class NoSuchInterface."
            )),
        "got: {d:?}"
    );
}

#[test]
fn a_sealed_tag_naming_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-sealed NoSuchSubtype */
            interface Sealed {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message
                .contains("PHPDoc tag @phpstan-sealed contains unknown class NoSuchSubtype.")),
        "got: {d:?}"
    );
}

#[test]
fn a_sealed_tag_naming_a_known_class_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Allowed {}
            /** @phpstan-sealed Allowed */
            interface Sealed {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}
