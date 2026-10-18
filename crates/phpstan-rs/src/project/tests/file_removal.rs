//! `Project::remove_file`: registry unwind, dependent re-check, and the
//! documented (not fixed by this item) trait/mixin staleness limitation.

use super::super::*;
use crate::project::session::load_test_file;

#[test]
fn removing_a_file_drops_its_declarations_from_the_registry() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php function helperA(): int { return 1; } class Foo {}",
        false,
    );
    project.finish_loading();
    assert!(project.registry().functions.contains_key("helperA"));
    assert!(project.registry().classes.contains_key("Foo"));

    project.remove_file(&PathBuf::from("a.php"));

    assert!(!project.registry().functions.contains_key("helperA"));
    assert!(!project.registry().classes.contains_key("Foo"));
}

#[test]
fn removing_a_file_re_checks_its_former_dependents() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("thing.php"),
        "<?php function thing(): int { return 1; }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("caller.php"),
        "<?php function useThing() { return thing(); }",
        false,
    );
    project.finish_loading();

    let result = project.remove_file(&PathBuf::from("thing.php"));

    let caller_diagnostics = result
        .get(&project.number_of("caller.php"))
        .unwrap_or_else(|| panic!("expected caller.php in {result:?}"));
    assert!(
        caller_diagnostics
            .iter()
            .any(|d| d.message.contains("thing")),
        "expected a diagnostic mentioning the now-undefined `thing`, got: {caller_diagnostics:?}"
    );
}

#[test]
fn removing_a_file_returns_it_with_empty_diagnostics_so_the_editor_can_clear_it() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php function helperA(): int { return 1; }",
        false,
    );
    project.finish_loading();

    let result = project.remove_file(&PathBuf::from("a.php"));

    assert_eq!(
        result
            .keys()
            .map(|&id| project.path_of(id))
            .collect::<Vec<_>>(),
        vec![Path::new("a.php")]
    );
    assert!(result[&project.number_of("a.php")].is_empty());
}

#[test]
fn removing_a_never_loaded_file_is_a_harmless_no_op() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php function helperA(): int { return 1; }",
        false,
    );
    project.finish_loading();

    let result = project.remove_file(&PathBuf::from("never.php"));

    assert_eq!(
        result
            .keys()
            .map(|&id| project.path_of(id))
            .collect::<Vec<_>>(),
        vec![Path::new("never.php")]
    );
    assert!(result[&project.number_of("never.php")].is_empty());
    assert!(project.registry().functions.contains_key("helperA"));
}

#[test]
fn removing_a_files_direct_ancestor_does_not_unstick_the_subclass_relationship_either() {
    // EMPIRICALLY VERIFIED, correcting an earlier (wrong) draft of this
    // test that assumed the opposite: `class_extends_implements_closure`
    // reads `Child`'s own, unchanged `ClassReflection.extends: ["Base"]` field
    // and inserts "Base" into Child's ancestor set unconditionally - it
    // never checks whether "Base" itself is still a live entry in
    // `self.classes` before recording it as a direct ancestor. Since
    // `remove_file(base.php)` never reparses child.php, Child's own
    // `extends` field is untouched, so this is the same staleness class
    // as the trait/mixin case above, just reached through a different
    // registry field. See the chain test below for the one real case
    // the finishing-pass rerun does fix.
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("base.php"),
        "<?php class Base {}",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("child.php"),
        "<?php class Child extends Base {}",
        false,
    );
    project.finish_loading();
    assert!(project.registry().is_subclass_of("Child", "Base"));

    project.remove_file(&PathBuf::from("base.php"));

    assert!(project.registry().is_subclass_of("Child", "Base"));
}

#[test]
fn removing_a_files_class_does_stop_a_transitive_ancestor_relationship_through_it() {
    // The one real case the finishing-pass rerun does fix: `Grandchild`
    // extends `Child` extends `Base` (three separate files). Before
    // removal, `class_extends_implements_closure("Grandchild")` walks
    // Grandchild -> Child (found in `self.classes`, contributes its own
    // `extends: ["Base"]`) -> Base, so "Base" is in Grandchild's
    // ancestor set. After removing child.php, the closure still records
    // "Child" itself (Grandchild's own `extends` field is unchanged),
    // but can no longer look `self.classes.get("Child")` up to continue
    // past it to reach "Base" - so `is_subclass_of("Grandchild","Base")`
    // correctly flips to `false`, even though
    // `is_subclass_of("Grandchild","Child")` incorrectly stays `true`
    // (same direct-ancestor staleness the test above documents).
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("base.php"),
        "<?php class Base {}",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("child.php"),
        "<?php class Child extends Base {}",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("grandchild.php"),
        "<?php class Grandchild extends Child {}",
        false,
    );
    project.finish_loading();
    assert!(project.registry().is_subclass_of("Grandchild", "Base"));

    project.remove_file(&PathBuf::from("child.php"));

    assert!(!project.registry().is_subclass_of("Grandchild", "Base"));
    assert!(project.registry().is_subclass_of("Grandchild", "Child"));
}

#[test]
fn removing_a_file_updates_file_count() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php function a(): int { return 1; }",
        false,
    );
    load_test_file(
        &mut project,
        PathBuf::from("b.php"),
        "<?php function b(): int { return 2; }",
        false,
    );
    project.finish_loading();
    assert_eq!(project.file_count(), 2);

    project.remove_file(&PathBuf::from("a.php"));

    assert_eq!(project.file_count(), 1);
}
