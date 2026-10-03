//! A declaration guarded by what the configured PHP versions rule out never
//! runs, so it is not indexed.

use super::super::*;
use syntax::lexer::Lexer;
use syntax::parser::Parser;
use types::{PhpVersion, PhpVersions};

fn index(src: &str, versions: Option<PhpVersions>) -> Registry {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = Registry {
        php_versions: versions,
        ..Registry::default()
    };
    registry.index(&program, &Origin::Unknown, false);
    registry
}

fn from(major: u8, minor: u8) -> Option<PhpVersions> {
    Some(PhpVersions::new(
        PhpVersion::new(major, minor),
        PhpVersion::new(8, 5),
    ))
}

const POLYFILL: &str = r#"<?php
    if (\PHP_VERSION_ID >= 80000) {
        return;
    }
    if (!function_exists('fdiv')) {
        function fdiv($a, $b) {}
    }
    if (!class_exists('Gone')) {
        class Gone {}
    }
    function hoisted() {}
    class Hoisted {}
"#;

#[test]
fn a_polyfill_that_the_versions_rule_out_is_not_indexed() {
    let r = index(POLYFILL, from(8, 0));
    assert!(r.lookup_function("fdiv").is_none());
    assert!(r.lookup_class("Gone").is_none());
    // PHP declares what is at the top level before it runs the return.
    assert!(r.lookup_function("hoisted").is_some());
    assert!(r.lookup_class("Hoisted").is_some());
}

#[test]
fn a_polyfill_the_versions_allow_is_indexed() {
    let r = index(POLYFILL, from(7, 4));
    assert!(r.lookup_function("fdiv").is_some());
    assert!(r.lookup_class("Gone").is_some());
}

#[test]
fn with_no_versions_configured_every_guard_is_open() {
    let r = index(POLYFILL, None);
    assert!(r.lookup_function("fdiv").is_some());
    assert!(r.lookup_class("Gone").is_some());
}

#[test]
fn a_version_test_picks_its_branch() {
    let src = r#"<?php
        if (PHP_VERSION_ID < 80100) {
            function old() {}
        } elseif (version_compare(PHP_VERSION, '8.3.0', '<')) {
            function middle() {}
        } else {
            function new_() {}
        }
    "#;
    let r = index(src, from(8, 1));
    assert!(r.lookup_function("old").is_none());
    assert!(r.lookup_function("middle").is_some());
    assert!(r.lookup_function("new_").is_some());
    let r = index(
        src,
        Some(PhpVersions::new(
            PhpVersion::new(8, 4),
            PhpVersion::new(8, 5),
        )),
    );
    assert!(r.lookup_function("old").is_none());
    assert!(r.lookup_function("middle").is_none());
    assert!(r.lookup_function("new_").is_some());
}

#[test]
fn a_guard_every_version_satisfies_declares_unconditionally() {
    let src = r#"<?php
        if (PHP_VERSION_ID >= 80000) {
            class Always {}
        }
    "#;
    let r = index(src, from(8, 1));
    assert!(r.unconditionally_declared_classes.contains("Always"));
    let r = index(src, from(7, 4));
    assert!(!r.unconditionally_declared_classes.contains("Always"));
    assert!(r.lookup_class("Always").is_some());
}

#[test]
fn code_that_decides_nothing_leaves_the_guard_open() {
    let src = r#"<?php
        if (PHP_VERSION_ID < 80000) {
            while (true) {
                function in_a_loop() {}
            }
        }
        function outer() {
            if (PHP_VERSION_ID < 80000) { function nested() {} }
        }
    "#;
    let r = index(src, from(8, 0));
    assert!(r.lookup_function("in_a_loop").is_none());
    assert!(r.lookup_function("nested").is_none());
    assert!(r.lookup_function("outer").is_some());
}

#[test]
fn a_declaration_that_changes_nothing_is_not_a_change() {
    let mut r = index("<?php class Held {}", None);
    r.take_changed();
    let tokens =
        Lexer::new("<?php if (!class_exists('Held')) { class Held {} }").tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    r.index(&program, &Origin::Unknown, false);
    let changed = r.take_changed();
    assert!(changed.classes.is_empty());
    assert!(changed.functions.is_empty());
}
