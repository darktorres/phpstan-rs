//! Tests for `checker::oop::check_unknown_phpstan_tags_in_class`/
//! `check_unknown_phpstan_tags_on` - real PHPStan's own
//! `InvalidPHPStanDocTagRule` (`phpDoc.phpstanTag`).
use super::helpers::*;

#[test]
fn a_typo_d_tag_on_a_class_docblock_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-consistent-constructr */
            class Foo {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "phpDoc.phpstanTag"
            && x.message.contains("@phpstan-consistent-constructr")),
        "got: {d:?}"
    );
}

#[test]
fn a_typo_d_tag_on_a_method_docblock_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @phpstan-retrn int */
                public function bar() { return 1; }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "phpDoc.phpstanTag" && x.message.contains("@phpstan-retrn")),
        "got: {d:?}"
    );
}

#[test]
fn a_typo_d_tag_on_a_top_level_function_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-parm int $x */
            function f(int $x) {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "phpDoc.phpstanTag" && x.message.contains("@phpstan-parm")),
        "got: {d:?}"
    );
}

#[test]
fn a_real_recognized_tag_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-consistent-constructor */
            class Foo {
                /** @phpstan-var int */
                public $x = 1;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpDoc.phpstanTag"),
        "unexpected diagnostics: {d:?}"
    );
}
