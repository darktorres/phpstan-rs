//! Opt-in `constructor.missingParentCall` (ROADMAP Phase 101's
//! E3, mirroring real `phpstan-strict-rules`'
//! `RequireParentConstructCallRule`).

use super::helpers::*;

#[test]
fn off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            class Base { public function __construct() {} }
            class Child extends Base {
                public function __construct() {}
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}

#[test]
fn a_missing_parent_call_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            class Base { public function __construct() {} }
            class Child extends Base {
                public function __construct() {}
            }
        "#,
        |r| r.check_parent_constructor_call = true,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}

#[test]
fn a_direct_parent_call_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Base { public function __construct() {} }
            class Child extends Base {
                public function __construct() {
                    parent::__construct();
                }
            }
        "#,
        |r| r.check_parent_constructor_call = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}

#[test]
fn a_parent_call_nested_inside_an_if_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Base { public function __construct() {} }
            class Child extends Base {
                public function __construct(bool $flag) {
                    if ($flag) {
                        parent::__construct();
                    } else {
                        parent::__construct();
                    }
                }
            }
        "#,
        |r| r.check_parent_constructor_call = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}

#[test]
fn no_parent_class_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Standalone {
                public function __construct() {}
            }
        "#,
        |r| r.check_parent_constructor_call = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}

#[test]
fn a_parent_with_no_own_constructor_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Base {}
            class Child extends Base {
                public function __construct() {}
            }
        "#,
        |r| r.check_parent_constructor_call = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}

#[test]
fn a_private_parent_constructor_is_not_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            class Base { private function __construct() {} }
            class Child extends Base {
                public function __construct() {}
            }
        "#,
        |r| r.check_parent_constructor_call = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}

#[test]
fn a_trait_constructor_is_never_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            trait T {
                public function __construct() {}
            }
        "#,
        |r| r.check_parent_constructor_call = true,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "constructor.missingParentCall"),
        "got: {d:?}"
    );
}
