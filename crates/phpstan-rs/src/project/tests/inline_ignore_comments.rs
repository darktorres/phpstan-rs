//! Inline `@phpstan-ignore` comments across an incremental `update_file`
//! edit. Suppression itself is covered by the fixtures under
//! `tests/fixtures/project/inline_ignore_comments`.

use std::path::PathBuf;

use rustc_hash::FxHashMap;

use super::super::Project;
use super::super::test_support::Files;

#[test]
fn update_file_recomputes_ignore_lines_too() {
    // The incremental-edit path (`update_file`) must recompute
    // `ignore_lines` from the *new* source, not keep stale ones from the
    // file's previous version - the same "must not go stale" concern
    // every other per-file cached fact in `FileState` already has to
    // handle on an edit.
    let mut project = Files::new()
        .analysed(
            "f.php",
            "<?php\nfunction f() {\n    totallyUndefinedFunction();\n}\n",
        )
        .load();
    let finalized = |project: &Project| {
        let path = PathBuf::from("f.php");
        let mut results = FxHashMap::default();
        results.insert(path.clone(), project.check_file(&path));
        project.apply_inline_ignores(&mut results, false);
        results.remove(&path).unwrap()
    };
    let before = finalized(&project);
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
    let after = finalized(&project);
    assert!(
        !after
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "unexpected diagnostics after adding the ignore comment: {after:?}"
    );
}
