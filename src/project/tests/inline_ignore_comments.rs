//! Inline `@phpstan-ignore` comments across an incremental `update_file`
//! edit. Suppression itself is covered by the fixtures under
//! `tests/fixtures/project/inline_ignore_comments`.

use std::path::PathBuf;

use super::super::*;

#[test]
fn update_file_recomputes_ignore_lines_too() {
    // The incremental-edit path (`update_file`) must recompute
    // `ignore_lines` from the *new* source, not keep stale ones from the
    // file's previous version - the same "must not go stale" concern
    // every other per-file cached fact in `FileState` already has to
    // handle on an edit.
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    totallyUndefinedFunction();\n}\n",
        )
        .unwrap();
    project.finish_loading();
    let before = project.check_file(&PathBuf::from("f.php"));
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
    let after = project.check_file(&PathBuf::from("f.php"));
    assert!(
        !after
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "unexpected diagnostics after adding the ignore comment: {after:?}"
    );
}
