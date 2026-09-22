//! Opt-in constructor-property-promotion suggestion
//! (`constructor.promotionCandidate`, ROADMAP Phase 97 AA3).

use super::helpers::*;
use crate::registry::Registry;

fn enable(r: &mut Registry) {
    r.check_promoted_property_suggestions = true;
    r.php_version = Some((8, 0));
}

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            class Point {
                public int $x;
                public function __construct(int $x) {
                    $this->x = $x;
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn a_trivially_forwarding_constructor_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            class Point {
                public int $x;
                public int $y;
                public function __construct(int $x, int $y) {
                    $this->x = $x;
                    $this->y = $y;
                }
            }
        "#,
        enable,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn requires_php_8_configured() {
    let d = diagnostics_for_with(
        r#"<?php
            class Point {
                public int $x;
                public function __construct(int $x) {
                    $this->x = $x;
                }
            }
        "#,
        |r| r.check_promoted_property_suggestions = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn an_already_partially_promoted_constructor_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Point {
                public int $y;
                public function __construct(public int $x, int $y) {
                    $this->y = $y;
                }
            }
        "#,
        enable,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn extra_logic_in_the_body_suppresses_the_suggestion() {
    let d = diagnostics_for_with(
        r#"<?php
            class Point {
                public int $x;
                public function __construct(int $x) {
                    $this->x = $x;
                    error_log("constructed");
                }
            }
        "#,
        enable,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn a_mismatched_order_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Point {
                public int $x;
                public int $y;
                public function __construct(int $x, int $y) {
                    $this->y = $y;
                    $this->x = $x;
                }
            }
        "#,
        enable,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn a_mismatched_type_between_param_and_property_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Point {
                public float $x;
                public function __construct(int $x) {
                    $this->x = $x;
                }
            }
        "#,
        enable,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}

#[test]
fn a_missing_property_declaration_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            /**
             * @property int $x
             */
            class Point {
                public function __construct(int $x) {
                    $this->x = $x;
                }
            }
        "#,
        enable,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.promotionCandidate"),
        "got: {d:?}"
    );
}
