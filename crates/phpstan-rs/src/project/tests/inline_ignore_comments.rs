//! Inline `@phpstan-ignore` comments across an incremental `update_file`
//! edit. Suppression itself is covered by the fixtures under
//! `tests/fixtures/project/inline_ignore_comments`.

use crate::project::session::load_test_file;
use std::path::{Path, PathBuf};

use super::super::*;

/// `path`'s diagnostics as the project reports them: its inline ignore
/// comments applied.
fn reported(project: &Project, path: &Path) -> Vec<Diagnostic> {
    let mut results = FxHashMap::from_iter([(path.to_path_buf(), project.check_file(path))]);
    project.apply_inline_ignores(&mut results, false);
    results.remove(path).unwrap_or_default()
}

#[test]
fn update_file_recomputes_ignore_lines_too() {
    // The incremental-edit path (`update_file`) must recompute
    // `ignore_lines` from the *new* source, not keep stale ones from the
    // file's previous version - the same "must not go stale" concern
    // every other per-file cached fact in `FileState` already has to
    // handle on an edit.
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("f.php"),
        "<?php\nfunction f() {\n    totallyUndefinedFunction();\n}\n",
        true,
    );
    project.finish_loading();
    let before = reported(&project, Path::new("f.php"));
    assert!(
        before
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "expected an undefined-function diagnostic before the edit, got: {before:?}"
    );

    project
        .update_file(
            &PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    totallyUndefinedFunction(); // @phpstan-ignore-line\n}\n",
        )
        .unwrap();
    let after = reported(&project, Path::new("f.php"));
    assert!(
        !after
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "unexpected diagnostics after adding the ignore comment: {after:?}"
    );
}

/// Two ignore comments, the second malformed (`@phpstan-ignore ,`).
const WITH_IGNORES: &str =
    "<?php\n// @phpstan-ignore-next-line\nfunction f() {}\n// @phpstan-ignore ,\n";

/// Whether `path`'s checked diagnostics report a malformed ignore comment.
fn reports_ignore_parse_error(project: &Project, path: &Path) -> bool {
    project
        .check_file(path)
        .iter()
        .any(|d| d.message.contains("Parse error in @phpstan-ignore"))
}

#[test]
fn a_context_file_keeps_no_ignore_comments() {
    // Only an analysed file's ignore comments are read, and a context file
    // read from the cache has none, so a parsed one keeps none either.
    let mut project = Project::new();
    let path = PathBuf::from("vendor/lib.php");
    load_test_file(&mut project, path.clone(), WITH_IGNORES, false);
    project.finish_loading();
    let ignores = &project.files.get(&path).unwrap().ignores;
    assert!(ignores.lines.is_empty());
    assert!(ignores.identifiers.is_empty());
    assert!(ignores.parse_errors.is_empty());
    assert!(!reports_ignore_parse_error(&project, &path));
}

#[test]
fn an_analysed_file_keeps_its_ignore_comments() {
    let mut project = Project::new();
    let path = PathBuf::from("src/lib.php");
    load_test_file(&mut project, path.clone(), WITH_IGNORES, true);
    project.finish_loading();
    assert!(
        !project
            .files
            .get(&path)
            .unwrap()
            .ignores
            .parse_errors
            .is_empty()
    );
    assert!(reports_ignore_parse_error(&project, &path));
}

#[test]
fn an_edit_keeps_a_context_file_a_context_file() {
    // A file the project has seen keeps its role, so an edit that names it
    // analysed leaves its ignore comments unread.
    let mut project = Project::new();
    let path = PathBuf::from("vendor/lib.php");
    load_test_file(&mut project, path.clone(), "<?php\n", false);
    project.finish_loading();
    project
        .update_file_as(&path, WITH_IGNORES, true, false, || false)
        .unwrap();
    let file = project.files.get(&path).unwrap();
    assert!(!file.analyzed);
    assert!(file.ignores.parse_errors.is_empty());
    assert!(!reports_ignore_parse_error(&project, &path));
}
