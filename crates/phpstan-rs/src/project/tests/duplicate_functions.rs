//! Duplicate function declarations across an incremental `update_file`
//! edit. The check itself is covered by the fixtures under
//! `tests/fixtures/project/duplicate_functions`.

use super::super::test_support::Files;
use super::super::*;

#[test]
fn an_update_file_edit_introduces_then_removes_a_duplicate() {
    let mut project = Files::new()
        .settings(ProjectSettings {
            rules: crate::rules_settings::RulesSettings {
                check_duplicate_function_declarations: true,
                ..Default::default()
            },
            ..ProjectSettings::default()
        })
        .context("a.php", "<?php function foo() {}")
        .load();

    let results = project
        .update_file(&PathBuf::from("b.php"), "<?php function foo() {}")
        .unwrap();
    assert!(
        results
            .values()
            .flatten()
            .any(|d| d.identifier == "function.duplicate"),
        "got: {results:?}"
    );

    let results = project
        .update_file(&PathBuf::from("b.php"), "<?php function bar() {}")
        .unwrap();
    assert!(
        !results
            .values()
            .flatten()
            .any(|d| d.identifier == "function.duplicate"),
        "got: {results:?}"
    );
}
