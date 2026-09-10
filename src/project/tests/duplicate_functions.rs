//! Phase 63: `checker::duplicate_functions::check_duplicate_function_declarations`,
//! wired unconditionally into `Project::check_file` - a top-level function
//! declared more than once, unconditionally, across the whole project.

use super::super::*;

#[test]
fn two_files_each_unconditionally_declaring_the_same_function_are_both_flagged() {
    let mut project = Project::new();
    project
        .load_file(PathBuf::from("a.php"), "<?php function foo() {}")
        .unwrap();
    project
        .load_file(PathBuf::from("b.php"), "<?php function foo() {}")
        .unwrap();
    project.finish_loading();

    let a_diags = project.check_file(&PathBuf::from("a.php"));
    let b_diags = project.check_file(&PathBuf::from("b.php"));
    assert!(
        a_diags.iter().any(|d| d.identifier == "function.duplicate"),
        "got: {a_diags:?}"
    );
    assert!(
        b_diags.iter().any(|d| d.identifier == "function.duplicate"),
        "got: {b_diags:?}"
    );
    let msg = &a_diags
        .iter()
        .find(|d| d.identifier == "function.duplicate")
        .unwrap()
        .message;
    assert!(msg.contains("a.php") && msg.contains("b.php"), "got: {msg}");
}

#[test]
fn a_same_file_unconditional_duplicate_is_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function foo() {} function foo() {}",
        )
        .unwrap();
    project.finish_loading();

    let diags = project.check_file(&PathBuf::from("a.php"));
    let count = diags
        .iter()
        .filter(|d| d.identifier == "function.duplicate")
        .count();
    assert_eq!(count, 2, "expected both sites flagged, got: {diags:?}");
}

#[test]
fn a_phpstorm_stubs_shaped_signature_split_is_not_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php function strtr(string $s, array $r) {} function strtr(string $s, string $f, string $t) {}",
        )
        .unwrap();
    project.finish_loading();

    let diags = project.check_file(&PathBuf::from("a.php"));
    assert!(
        !diags.iter().any(|d| d.identifier == "function.duplicate"),
        "got: {diags:?}"
    );
}

#[test]
fn a_function_exists_guarded_polyfill_duplicated_across_two_files_is_not_flagged() {
    let mut project = Project::new();
    let src = "<?php if (!function_exists('foo')) { function foo() {} }";
    project.load_file(PathBuf::from("a.php"), src).unwrap();
    project.load_file(PathBuf::from("b.php"), src).unwrap();
    project.finish_loading();

    let a_diags = project.check_file(&PathBuf::from("a.php"));
    let b_diags = project.check_file(&PathBuf::from("b.php"));
    assert!(
        !a_diags.iter().any(|d| d.identifier == "function.duplicate"),
        "got: {a_diags:?}"
    );
    assert!(
        !b_diags.iter().any(|d| d.identifier == "function.duplicate"),
        "got: {b_diags:?}"
    );
}

#[test]
fn an_update_file_edit_introduces_then_removes_a_duplicate() {
    let mut project = Project::new();
    project
        .load_file(PathBuf::from("a.php"), "<?php function foo() {}")
        .unwrap();
    project.finish_loading();

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
