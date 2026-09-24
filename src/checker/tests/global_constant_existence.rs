//! `constant.notFound` for undefined global constants.

use super::helpers::*;

#[test]
fn dynamic_constant_names_read_as_their_general_or_configured_type() {
    let d = diagnostics_for_with(
        r#"<?php
            const DEBUG = false;
            define('MODE', 'dev');
            class Settings { const LEVEL = 3; }
            function f(): void {
                \PHPStan\Testing\assertType('bool', DEBUG);
                \PHPStan\Testing\assertType("'dev'|'prod'", MODE);
                \PHPStan\Testing\assertType('int', Settings::LEVEL);
                \PHPStan\Testing\assertType('string', FROM_ENV);
            }
        "#,
        |r| {
            r.set_dynamic_constant_names(&[
                ("DEBUG".to_string(), None),
                ("MODE".to_string(), Some("'dev'|'prod'".to_string())),
                ("Settings::LEVEL".to_string(), None),
                ("FROM_ENV".to_string(), Some("string".to_string())),
            ])
        },
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "phpstanRs.assertType" || x.identifier == "constant.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_declared_value_its_configured_type_rejects_is_flagged() {
    let d = diagnostics_for_with(
        r#"<?php
            const DEBUG = 'yes';
            define('MODE', 'staging');
            class Settings { const LEVEL = 'high'; }
        "#,
        |r| {
            r.set_dynamic_constant_names(&[
                ("DEBUG".to_string(), Some("bool".to_string())),
                ("MODE".to_string(), Some("'dev'|'prod'".to_string())),
                ("Settings::LEVEL".to_string(), Some("int".to_string())),
            ])
        },
    );
    let found: Vec<_> = d
        .iter()
        .filter(|x| x.message.starts_with("Configuration defined type"))
        .map(|x| (x.identifier.as_str(), x.message.as_str()))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                "constant.value",
                "Configuration defined type for constant DEBUG (bool) does not accept value 'yes'."
            ),
            (
                "constant.defineValue",
                "Configuration defined type for constant MODE ('dev'|'prod') does not accept value 'staging'."
            ),
            (
                "classConstant.value",
                "Configuration defined type for constant Settings::LEVEL (int) does not accept value 'high'."
            ),
        ],
        "got: {d:?}"
    );
}
