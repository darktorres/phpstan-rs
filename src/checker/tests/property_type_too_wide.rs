//! `property.unusedType`/`staticProperty.unusedType` -
//! `check_too_wide_property_types`'s own port of the return-type sibling
//! (`return_type_too_wide.rs`).

use super::helpers::*;

#[test]
fn a_declared_union_member_never_assigned_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                private int|string $x;
                public function __construct() {
                    $this->x = 1;
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "property.unusedType" && x.message.contains("string")),
        "got: {d:?}"
    );
}

#[test]
fn a_null_default_counts_as_a_real_assignment() {
    // The property is never explicitly written to `null` anywhere in the
    // class body, but its own `= null` default is a real, reachable
    // value (every instance starts there until `setX()` runs) -
    // `property_assign_facts` only ever records an explicit
    // `ExprKind::Assign` node, so without this the `null` member looks
    // unused even though removing it would make the property's own
    // initial state impossible to declare.
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                private ?string $x = null;
                public function setX(string $x): void {
                    $this->x = $x;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn every_declared_member_actually_assigned_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                private int|string $x;
                public function __construct(bool $b) {
                    $this->x = $b ? 1 : 'y';
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.unusedType"),
        "got: {d:?}"
    );
}

#[test]
fn a_never_written_property_is_not_this_checks_concern() {
    // No real write anywhere at all (just the default) - dead code or a
    // forever-default property, neither of which is "declares a member
    // nothing uses" in the sense this check means.
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                private ?string $x = null;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "property.unusedType"),
        "got: {d:?}"
    );
}
