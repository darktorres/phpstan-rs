//! Phase 44 (partial): `@phpstan-ignore-line`/`@phpstan-ignore-next-line`
//! inline suppression - end-to-end through `Project::load_file`/
//! `check_file`, on top of `ignore_comments::ignored_lines`'s own
//! already-tested pure text-scanning logic.

use std::path::PathBuf;

use super::super::*;

#[test]
fn a_trailing_ignore_line_comment_suppresses_that_line_s_diagnostics() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    totallyUndefinedFunction(); // @phpstan-ignore-line\n}\n",
        )
        .unwrap();
    project.finish_loading();
    let diagnostics = project.check_file(&PathBuf::from("f.php"));
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_standalone_ignore_next_line_comment_suppresses_the_following_line() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    // @phpstan-ignore-next-line\n    totallyUndefinedFunction();\n}\n",
        )
        .unwrap();
    project.finish_loading();
    let diagnostics = project.check_file(&PathBuf::from("f.php"));
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_call_with_no_ignore_comment_is_still_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    totallyUndefinedFunction();\n}\n",
        )
        .unwrap();
    project.finish_loading();
    let diagnostics = project.check_file(&PathBuf::from("f.php"));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "expected an undefined-function diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn an_ignore_line_comment_only_suppresses_its_own_line_not_others() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    totallyUndefinedFunction(); // @phpstan-ignore-line\n    anotherUndefinedFunction();\n}\n",
        )
        .unwrap();
    project.finish_loading();
    let diagnostics = project.check_file(&PathBuf::from("f.php"));
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "unexpected diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("anotherUndefinedFunction")),
        "expected the unrelated line's own diagnostic to survive, got: {diagnostics:?}"
    );
}

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
