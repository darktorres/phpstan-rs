//! `checker::whole_program::circular_namespace_deps` (ROADMAP
//! Phase 86's B3), wired through `Project::check_whole_program` -
//! opt-in, off by default (see `registry::Registry::
//! check_circular_dependencies`'s own doc comment). Every fixture loads
//! via `load_analyzed_file`, not `load_file` - see `dead_enum_cases`'s
//! own module doc comment for why.

use super::super::*;

fn project_with_check_enabled() -> Project {
    let mut project = Project::new();
    project.set_check_circular_dependencies(true);
    project
}

#[test]
fn off_by_default() {
    let mut project = Project::new();
    project
        .load_analyzed_file(
            PathBuf::from("a.php"),
            "<?php namespace NsA; class A extends \\NsB\\B {}",
        )
        .unwrap();
    project
        .load_analyzed_file(
            PathBuf::from("b.php"),
            "<?php namespace NsB; class B extends \\NsA\\A {}",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_two_namespace_extends_cycle_is_flagged_when_enabled() {
    // Two DIFFERENT class pairs, each pair's own edge one-directional -
    // no single pair has a (necessarily-invalid-PHP) class-level cycle,
    // but NsA and NsB depend on each other in aggregate.
    let mut project = project_with_check_enabled();
    let path_a = PathBuf::from("a.php");
    let path_b = PathBuf::from("b.php");
    project
        .load_analyzed_file(
            path_a.clone(),
            "<?php namespace NsA; class A extends \\NsB\\B {}\nclass D {}",
        )
        .unwrap();
    project
        .load_analyzed_file(
            path_b.clone(),
            "<?php namespace NsB; class B {}\nclass C extends \\NsA\\D {}",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let all_diags: Vec<_> = results.values().flatten().collect();
    assert!(
        all_diags.iter().any(|d| {
            d.identifier == "namespace.circularDependency"
                && d.message.contains("NsA")
                && d.message.contains("NsB")
        }),
        "got: {results:?}"
    );
}

#[test]
fn a_one_directional_dependency_is_not_flagged() {
    let mut project = project_with_check_enabled();
    project
        .load_analyzed_file(
            PathBuf::from("a.php"),
            "<?php namespace NsA; class A extends \\NsB\\B {}",
        )
        .unwrap();
    project
        .load_analyzed_file(PathBuf::from("b.php"), "<?php namespace NsB; class B {}")
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_same_namespace_dependency_is_never_flagged() {
    let mut project = project_with_check_enabled();
    project
        .load_analyzed_file(
            PathBuf::from("a.php"),
            "<?php namespace NsA; class A {}\nclass B extends A {}",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_trait_use_can_participate_in_a_cycle() {
    let mut project = project_with_check_enabled();
    let path_a = PathBuf::from("a.php");
    let path_b = PathBuf::from("b.php");
    project
        .load_analyzed_file(
            path_a.clone(),
            "<?php namespace NsA; class A { use \\NsB\\TraitB; }",
        )
        .unwrap();
    project
        .load_analyzed_file(
            path_b.clone(),
            "<?php namespace NsB; trait TraitB {}\nclass C extends \\NsA\\A {}",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let all_diags: Vec<_> = results.values().flatten().collect();
    assert!(
        all_diags
            .iter()
            .any(|d| d.identifier == "namespace.circularDependency"),
        "got: {results:?}"
    );
}

#[test]
fn a_cycle_in_a_non_analyzed_file_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path_a = PathBuf::from("vendor/lib/a.php");
    let path_b = PathBuf::from("vendor/lib/b.php");
    project
        .load_file(
            path_a.clone(),
            "<?php namespace NsA; class A extends \\NsB\\B {}",
        )
        .unwrap();
    project
        .load_file(
            path_b.clone(),
            "<?php namespace NsB; class B extends \\NsA\\A {}",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(
        results.is_empty(),
        "a vendor-only file should never gain a new results entry, got: {results:?}"
    );
}
