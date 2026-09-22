//! `checker::whole_program::dead_code` (ROADMAP Phase 86's B2),
//! wired through `Project::check_whole_program` - opt-in, off by
//! default (see `registry::Registry::check_dead_code`'s own doc
//! comment). Every fixture loads via `load_analyzed_file`, not
//! `load_file` - see `dead_enum_cases`'s own module doc comment for why.

use super::super::*;

fn project_with_check_enabled() -> Project {
    let mut project = Project::new();
    project.set_check_dead_code(true);
    project
}

#[test]
fn off_by_default() {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("a.php"), "<?php function unused() {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_never_referenced_function_is_flagged_when_enabled() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php function unused() {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).expect("expected findings for a.php");
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "function.unused" && d.message.contains("unused")),
        "got: {diags:?}"
    );
}

#[test]
fn a_function_called_from_a_different_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php function used() {}")
        .unwrap();
    project
        .load_analyzed_file(PathBuf::from("b.php"), "<?php used();")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("used")),
        "got: {diags:?}"
    );
}

#[test]
fn autoload_is_never_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php function __autoload($class) {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn a_never_referenced_class_is_flagged_when_enabled() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Unused {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).expect("expected findings for a.php");
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "class.unused" && d.message.contains("Unused")),
        "got: {diags:?}"
    );
}

#[test]
fn a_class_instantiated_from_a_different_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Used {}")
        .unwrap();
    project
        .load_analyzed_file(PathBuf::from("b.php"), "<?php new Used();")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Used")),
        "got: {diags:?}"
    );
}

#[test]
fn a_class_only_referenced_via_a_type_hint_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Hinted {}")
        .unwrap();
    project
        .load_analyzed_file(PathBuf::from("b.php"), "<?php function f(Hinted $x) {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Hinted")),
        "got: {diags:?}"
    );
}

#[test]
fn a_class_only_extended_from_a_different_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Base {}")
        .unwrap();
    project
        .load_analyzed_file(PathBuf::from("b.php"), "<?php class Sub extends Base {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Base")),
        "got: {diags:?}"
    );
}

#[test]
fn a_never_referenced_class_in_a_non_analyzed_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("vendor/lib/a.php");
    project
        .load_file(path.clone(), "<?php class Unused {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(
        !results.contains_key(&path),
        "a vendor-only file should never gain a new results entry, got: {results:?}"
    );
}

#[test]
fn a_functions_own_ignore_comment_suppresses_it() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php\n// @phpstan-ignore function.unused\nfunction unused() {}\n",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("unused")),
        "got: {diags:?}"
    );
}
