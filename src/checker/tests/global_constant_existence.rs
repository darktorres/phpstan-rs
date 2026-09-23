//! `constant.notFound` for undefined global constants.

use super::helpers::*;

fn unknown(d: &[crate::Diagnostic]) -> Vec<&str> {
    d.iter()
        .filter(|x| x.identifier == "constant.notFound")
        .map(|x| x.message.as_str())
        .collect()
}

#[test]
fn an_undefined_global_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            const DECLARED = 1;
            define('DEFINED_TOP', 2);
            function setup(): void { define('DEFINED_IN_FUNCTION', 3); }
            function f(): void {
                echo DECLARED, DEFINED_TOP, DEFINED_IN_FUNCTION, PHP_EOL, __DIR__, __CLASS__;
                echo NOPE;
                if (defined('GUARDED')) { echo GUARDED; }
                exit;
            }
        "#,
    );
    assert_eq!(unknown(&d), vec!["Constant NOPE not found."], "got: {d:?}");
}

#[test]
fn a_namespaced_reference_falls_back_to_the_global_constant() {
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            function f(): void {
                echo PHP_EOL;
                echo \PHP_EOL;
                echo MISSING_HERE;
            }
        "#,
    );
    assert_eq!(
        unknown(&d),
        vec!["Constant MISSING_HERE not found."],
        "got: {d:?}"
    );
}

#[test]
fn magic_constants_used_where_they_are_always_empty_are_flagged() {
    let d = diagnostics_for(
        r#"<?php
            echo __CLASS__, __TRAIT__, __FUNCTION__, __METHOD__, __NAMESPACE__;
            $c = function () { return __FUNCTION__; };
            function f(): string { return __FUNCTION__ . __CLASS__; }
            trait T { public function m(): string { return __TRAIT__ . __CLASS__; } }
            class K { public function m(): string { return __CLASS__ . __METHOD__ . __TRAIT__; } }
        "#,
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.identifier.starts_with("magicConstant."))
        .map(|x| x.message.as_str())
        .collect();
    assert_eq!(
        found,
        vec![
            "Magic constant __CLASS__ is always empty outside a class.",
            "Magic constant __TRAIT__ is always empty outside a trait.",
            "Magic constant __FUNCTION__ is always empty outside a function.",
            "Magic constant __METHOD__ is always empty outside a function.",
            "Magic constant __NAMESPACE__ is always empty in global namespace.",
            "Magic constant __CLASS__ is always empty outside a class.",
            "Magic constant __TRAIT__ is always empty outside a trait.",
        ],
        "got: {d:?}"
    );
}
