//! `checker::whole_program::dead_enum_cases` (PHASE_BACKLOG Phase 92),
//! wired through `Project::check_whole_program` - opt-in, off by
//! default (see `registry::Registry::check_dead_enum_cases`'s own doc
//! comment). Driven through `check_whole_program` directly (not
//! `check_all`/`check_file`, which never call it) - this is the same
//! entry point `check-project`'s own primary loop calls once, after its
//! own per-file pass.

use super::super::*;

fn project_with_check_enabled() -> Project {
    let mut project = Project::new();
    project.set_check_dead_enum_cases(true);
    project
}

#[test]
fn off_by_default() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php enum Suit { case Hearts; case Spades; }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_never_referenced_case_is_flagged_when_enabled() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit { case Hearts; case Spades; }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).expect("expected findings for a.php");
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "enumCase.unused" && d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "enumCase.unused" && d.message.contains("Suit::Spades")),
        "got: {diags:?}"
    );
}

#[test]
fn a_case_referenced_by_bare_name_is_not_flagged_even_from_a_different_file() {
    let mut project = project_with_check_enabled();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php enum Suit { case Hearts; case Spades; }",
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("b.php"),
            "<?php function f() { return Suit::Hearts; }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results
        .get(&PathBuf::from("a.php"))
        .cloned()
        .unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("Suit::Spades")),
        "expected Spades to still be flagged, got: {diags:?}"
    );
}

#[test]
fn a_case_used_as_a_switch_arm_value_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit { case Hearts; case Spades; } \
             function f($x) { switch ($x) { case Suit::Hearts: break; } }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
}

#[test]
fn a_case_used_as_a_match_arm_value_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit { case Hearts; case Spades; } \
             function f($x) { return match ($x) { Suit::Hearts => 1, default => 0 }; }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
}

#[test]
fn a_case_produced_by_a_literal_from_call_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit: string { case Hearts = 'H'; case Spades = 'S'; } \
             function f() { return Suit::from('H'); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("Suit::Spades")),
        "expected Spades to still be flagged, got: {diags:?}"
    );
}

#[test]
fn a_case_produced_by_a_literal_try_from_call_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit: string { case Hearts = 'H'; case Spades = 'S'; } \
             function f() { return Suit::tryFrom('H'); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
}

#[test]
fn a_property_default_value_reference_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit { case Hearts; case Spades; } \
             class Card { public Suit $suit = Suit::Hearts; }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
}

#[test]
fn a_cases_call_suppresses_the_whole_enum() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit { case Hearts; case Spades; } \
             function f() { return Suit::cases(); }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn a_dynamic_member_access_suppresses_the_whole_enum() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit { case Hearts; case Spades; } \
             function f($name) { return Suit::{$name}; }",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(diags.is_empty(), "got: {diags:?}");
}

#[test]
fn an_enum_case_own_ignore_comment_suppresses_it() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_file(
            path.clone(),
            "<?php enum Suit {\n    case Hearts; // @phpstan-ignore enumCase.unused\n    case Spades;\n}\n",
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags.iter().any(|d| d.message.contains("Suit::Hearts")),
        "got: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("Suit::Spades")),
        "expected Spades to still be flagged, got: {diags:?}"
    );
}
