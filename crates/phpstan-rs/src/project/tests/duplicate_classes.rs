//! Duplicate class-like declarations across an incremental `update_file`
//! edit. The check itself is covered by the fixtures under
//! `tests/fixtures/project/duplicate_classes`.

use super::super::*;
use crate::project::session::load_test_file;

#[test]
fn an_update_file_edit_introduces_then_removes_a_duplicate() {
    let mut project = Project::new();
    project.apply_settings(ProjectSettings {
        rules: crate::rules_settings::RulesSettings {
            check_duplicate_class_declarations: true,
            ..Default::default()
        },
        ..ProjectSettings::default()
    });
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php class Foo {}",
        false,
    );
    project.finish_loading();

    let results = project
        .update_file(&PathBuf::from("b.php"), "<?php class Foo {}")
        .unwrap();
    assert!(
        results
            .values()
            .flatten()
            .any(|d| d.identifier == "class.duplicate"),
        "got: {results:?}"
    );

    let results = project
        .update_file(&PathBuf::from("b.php"), "<?php class Bar {}")
        .unwrap();
    assert!(
        !results
            .values()
            .flatten()
            .any(|d| d.identifier == "class.duplicate"),
        "got: {results:?}"
    );
}
