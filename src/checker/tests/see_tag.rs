//! `@see` PHPDoc tag target validation (`phpDoc.seeTag`, PHASE_BACKLOG
//! Phase 96 D5) - `check_doc_see_tag_targets` in `checker/oop.rs`.

use super::helpers::*;

#[test]
fn an_unknown_class_target_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @see TotallyMadeUpClass */
            function f(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn a_real_class_target_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Real {}
            /** @see Real */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn an_unknown_method_target_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Real {}
            /** @see Real::missing() */
            function f(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn a_real_method_target_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Real {
                public function bar(): void {}
            }
            /** @see Real::bar() */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn a_real_property_target_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Real {
                public int $x = 1;
            }
            /** @see Real::$x */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn an_unknown_property_target_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Real {}
            /** @see Real::$missing */
            function f(): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn a_real_constant_target_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Real {
                const FOO = 1;
            }
            /** @see Real::FOO */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn a_url_target_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @see https://example.com/docs */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn free_text_prose_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @see the return value of parseFoo for details */
            function f(): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}

#[test]
fn a_method_target_on_a_class_declaration_is_checked_too() {
    let d = diagnostics_for(
        r#"<?php
            class Other {}
            class Real {
                /** @see Other::missing() */
                public function bar(): void {}
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "phpDoc.seeTag"),
        "got: {d:?}"
    );
}
