//! `@phpstan-ignore-line`/`@phpstan-ignore-next-line` inline suppression,
//! and the fuller bare `@phpstan-ignore <identifier list>` form -
//! end-to-end through `Project::load_file`/`check_file`, on top of
//! `ignore_comments`'s own already-tested pure text-scanning logic.

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

#[test]
fn an_identifier_list_suppresses_only_the_matching_identifier() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    totallyUndefinedFunction(); // @phpstan-ignore function.notFound\n}\n",
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
fn an_identifier_list_with_the_wrong_identifier_does_not_suppress() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    totallyUndefinedFunction(); // @phpstan-ignore argument.type\n}\n",
        )
        .unwrap();
    project.finish_loading();
    let diagnostics = project.check_file(&PathBuf::from("f.php"));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("totallyUndefinedFunction")),
        "expected the mismatched-identifier ignore to leave the real diagnostic in place, got: {diagnostics:?}"
    );
}

#[test]
fn a_standalone_identifier_list_suppresses_the_following_line() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f() {\n    // @phpstan-ignore function.notFound\n    totallyUndefinedFunction();\n}\n",
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
fn a_malformed_identifier_list_is_reported_as_a_parse_error() {
    let mut project = Project::new();
    project
        .load_analyzed_file(
            PathBuf::from("f.php"),
            "<?php\nfunction f(): void {\n    totallyUndefinedFunction(); // @phpstan-ignore function.notFound,\n}\n",
        )
        .unwrap();
    project.finish_loading();
    let diagnostics = project.check_file(&PathBuf::from("f.php"));
    let parse_error = diagnostics
        .iter()
        .find(|d| d.identifier == "ignore.parseError")
        .expect("parse error reported");
    assert_eq!(parse_error.span.start.line, 3);
    assert_eq!(
        parse_error.message,
        "Parse error in @phpstan-ignore: Unexpected end after comma (,), expected identifier"
    );
    assert!(parse_error.non_ignorable);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.identifier == "function.notFound"),
        "a malformed list suppresses nothing: {diagnostics:?}"
    );
}

fn deferred_results(source: &str, report_unmatched: bool) -> Vec<(u32, String, String)> {
    let mut project = Project::new();
    project.set_defer_inline_ignores(true);
    let path = PathBuf::from("f.php");
    project.load_analyzed_file(path.clone(), source).unwrap();
    project.finish_loading();
    let mut results = FxHashMap::default();
    results.insert(path.clone(), project.check_file(&path));
    project.apply_inline_ignores(&mut results, report_unmatched);
    results
        .remove(&path)
        .unwrap_or_default()
        .into_iter()
        .map(|d| (d.span.start.line, d.identifier, d.message))
        .collect()
}

#[test]
fn deferred_ignores_still_suppress() {
    let found = deferred_results(
        "<?php\nfunction f(): void {\n    a(); // @phpstan-ignore function.notFound\n    // @phpstan-ignore-next-line\n    b();\n}\n",
        false,
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn unmatched_ignores_are_reported_when_asked() {
    let source = "<?php\nfunction f(): void {\n    a(); // @phpstan-ignore function.notFound, argument.type\n    $x = 1; // @phpstan-ignore-line\n    // @phpstan-ignore-next-line\n    b();\n}\n";
    assert_eq!(
        deferred_results(source, true),
        vec![
            (
                3,
                "ignore.unmatchedIdentifier".to_string(),
                "No error with identifier argument.type is reported on line 3.".to_string()
            ),
            (
                4,
                "ignore.unmatchedLine".to_string(),
                "No error to ignore is reported on line 4.".to_string()
            ),
        ]
    );
    assert!(deferred_results(source, false).is_empty());
}

#[test]
fn ignores_inside_a_trait_are_never_reported_unmatched() {
    let source = "<?php\ntrait T {\n    public function m(): void {\n        $x = 1; // @phpstan-ignore-line\n    }\n}\n";
    assert!(deferred_results(source, true).is_empty());
}
