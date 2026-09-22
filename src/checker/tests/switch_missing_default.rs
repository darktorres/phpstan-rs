//! Opt-in `switch` missing-`default:`-arm detection (ROADMAP
//! Phase 146 147f).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): void {
                switch ($x) {
                    case 1:
                        break;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "switch.missingDefault"),
        "got: {d:?}"
    );
}

#[test]
fn a_switch_with_no_default_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): void {
                switch ($x) {
                    case 1:
                        break;
                }
            }
        "#,
        |r| r.check_switch_missing_default = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "switch.missingDefault"),
        "got: {d:?}"
    );
}

#[test]
fn a_switch_with_a_default_arm_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x): void {
                switch ($x) {
                    case 1:
                        break;
                    default:
                        break;
                }
            }
        "#,
        |r| r.check_switch_missing_default = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "switch.missingDefault"),
        "got: {d:?}"
    );
}
