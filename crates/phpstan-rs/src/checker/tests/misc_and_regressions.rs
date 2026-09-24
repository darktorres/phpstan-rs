//! General regressions and checks that don't fit another category.

use super::helpers::*;

#[test]
fn conditionally_declared_function_is_visible_to_undefined_function_checks() {
    // The real, common `if (!function_exists('foo')) { function foo()
    // {} }` polyfill idiom (symfony/deprecation-contracts and every
    // symfony/polyfill-* package use exactly this) - Registry::index
    // recurses into if-bodies specifically so this isn't flagged.
    let d = diagnostics_for(
        r#"<?php
            if (!function_exists('polyfilled')) {
                function polyfilled() {}
            }
            function f() { return polyfilled(); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
