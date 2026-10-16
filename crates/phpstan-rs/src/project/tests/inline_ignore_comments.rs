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
