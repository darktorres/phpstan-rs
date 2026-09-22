//! `checker::whole_program::dead_abstract_classes` (ROADMAP Phase
//! 93), wired through `Project::check_whole_program` - opt-in, off by
//! default (see `registry::Registry::check_dead_abstract_classes`'s own
//! doc comment). Every fixture loads via `load_analyzed_file`, not
//! `load_file` - see `dead_enum_cases`'s own module doc comment for why.

use super::super::*;

fn project_with_check_enabled() -> Project {
    let mut project = Project::new();
    project.set_check_dead_abstract_classes(true);
    project
}

#[test]
fn off_by_default() {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("a.php"), "<?php abstract class Base {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn an_abstract_class_with_no_subclass_is_flagged_when_enabled() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php abstract class Base {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).expect("expected findings for a.php");
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "class.deadAbstract" && d.message.contains("Base")),
        "got: {diags:?}"
    );
}

#[test]
fn an_abstract_class_with_a_concrete_subclass_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php abstract class Base {}\nclass Concrete extends Base {}",
        )
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
fn an_abstract_class_with_only_an_abstract_subclass_is_still_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            "<?php abstract class Base {}\nabstract class MoreAbstract extends Base {}",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "class.deadAbstract" && d.message.contains("Base")),
        "got: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "class.deadAbstract" && d.message.contains("MoreAbstract")),
        "expected MoreAbstract to also be flagged, got: {diags:?}"
    );
}

#[test]
fn a_concrete_subclass_from_a_different_file_still_counts() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php abstract class Base {}")
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php class Concrete extends Base {}",
        )
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
fn an_interface_is_never_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php interface Foo {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_concrete_class_is_never_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(path.clone(), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_dead_abstract_class_in_a_non_analyzed_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("vendor/lib/a.php");
    project
        .load_file(path.clone(), "<?php abstract class Base {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(
        !results.contains_key(&path),
        "a vendor-only file should never gain a new results entry, got: {results:?}"
    );
}
