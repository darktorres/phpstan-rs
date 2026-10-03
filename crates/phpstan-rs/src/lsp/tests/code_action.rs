//! `textDocument/codeAction`: converting a `DiagnosticFix` into a
//! `quickfix` `CodeAction`, the range-overlap check, and the LSP-side
//! wiring (capability advertisement, the `context.only` kind gate, and
//! the `last_diagnostics` cache `code_actions` reads instead of
//! re-running a whole-file checker pass per request).

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use diagnostics::DiagnosticBuilder;
use lsp_types::CodeActionOrCommand;
use serde_json::{Value, json};
use syntax::span::{LineIndex, Span};

fn dummy_span() -> Span {
    Span { start: 0, end: 9 }
}

fn index() -> LineIndex {
    LineIndex::new(b"<?php $x = 1;\n")
}

fn range(start: (u32, u32), end: (u32, u32)) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position {
            line: start.0,
            character: start.1,
        },
        end: lsp_types::Position {
            line: end.0,
            character: end.1,
        },
    }
}

// Tier 1: pure unit tests, no Project/Server - no production check
// populates `.fix` yet, so a synthetic diagnostic (mirroring
// checker/tests/diagnostic_identifiers.rs's own style) is the only way
// to exercise the conversion logic itself.

#[test]
fn diagnostic_with_fix_becomes_a_quickfix_code_action() {
    let uri = path_to_uri(Path::new("/tmp/a.php"));
    let diag = DiagnosticBuilder::new("msg".to_string(), dummy_span(), 5, "some.identifier")
        .fix(dummy_span(), "CorrectCasing", "Fix casing")
        .build(&index());

    let Some(CodeActionOrCommand::CodeAction(action)) =
        diagnostic_fix_to_code_action(&uri, &diag, None, Positions::utf16(&index()))
    else {
        panic!("expected a CodeAction for a diagnostic with a fix");
    };
    assert_eq!(action.title, "Fix casing");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));
    // `WorkspaceEdit.changes` is a `HashMap<Uri, Vec<TextEdit>>` - clippy
    // flags `Uri`'s internal parsing-cache cell as a "mutable key type",
    // but nothing here (or anywhere this server uses it) mutates that
    // cache in a way that could change a `Uri`'s `Eq`/`Hash`, so a plain
    // `into_iter().find()` sidesteps the lint without an `#[allow]`.
    let mut edits = action
        .edit
        .unwrap()
        .changes
        .unwrap()
        .into_iter()
        .find(|(u, _)| *u == uri)
        .map(|(_, edits)| edits)
        .unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits.remove(0).new_text, "CorrectCasing");
}

#[test]
fn diagnostic_without_fix_yields_no_code_action() {
    let uri = path_to_uri(Path::new("/tmp/a.php"));
    let diag = DiagnosticBuilder::new("msg".to_string(), dummy_span(), 5, "some.identifier")
        .build(&index());
    assert!(diagnostic_fix_to_code_action(&uri, &diag, None, Positions::utf16(&index())).is_none());
}

#[test]
fn ranges_overlap_matches_touching_and_contained_ranges_but_not_disjoint_ones() {
    assert!(!ranges_overlap(
        range((0, 0), (0, 1)),
        range((5, 0), (5, 1))
    ));
    assert!(ranges_overlap(
        range((0, 0), (0, 5)),
        range((0, 5), (0, 10))
    ));
    // A zero-width range (a bare cursor, no selection) inside a wider span.
    assert!(ranges_overlap(
        range((0, 2), (0, 2)),
        range((0, 0), (0, 10))
    ));
}

// Tier 2/3: full request/response integration via run_loop.

fn code_action_request(id: i64, uri: &str, r: Value, only: Option<Vec<&str>>) -> Value {
    let mut context = json!({"diagnostics": []});
    if let Some(only) = only {
        context["only"] = json!(only);
    }
    request(
        id,
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri}, "range": r, "context": context}),
    )
}

fn point_range(pos: Value) -> Value {
    json!({"start": pos, "end": pos})
}

#[test]
fn initialize_advertises_code_action_provider() {
    let dir = TempDir::new("ca_caps");
    let messages = run(&dir, &[]);
    let caps = &messages[0]["result"]["capabilities"];
    assert_eq!(
        caps["codeActionProvider"]["codeActionKinds"],
        json!(["quickfix"])
    );
}

#[test]
fn code_action_on_a_real_unfixable_diagnostic_returns_empty_array_not_null() {
    let dir = TempDir::new("ca_no_fix");
    let src = "<?php function f() { echo $undefinedVariable; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let r = point_range(lsp_pos(src, "$undefinedVariable"));

    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": src}}),
            ),
            code_action_request(2, &uri, r, None),
        ],
    );

    let result = response_result(&messages, 2);
    assert_eq!(*result, json!([]), "got: {result:?}");
}

#[test]
fn code_action_for_a_non_file_uri_returns_null() {
    let dir = TempDir::new("ca_non_file_uri");
    let r = point_range(json!({"line": 0, "character": 0}));
    let messages = run(
        &dir,
        &[code_action_request(2, "untitled:Untitled-1", r, None)],
    );
    assert_eq!(*response_result(&messages, 2), Value::Null);
}

#[test]
fn code_action_with_only_excluding_quickfix_returns_empty_without_needing_a_valid_diagnostic() {
    let dir = TempDir::new("ca_only_excludes");
    let r = point_range(json!({"line": 0, "character": 0}));
    // A URI that was never even loaded - if the `only` short-circuit
    // didn't fire before code_actions ran, this would return null, not
    // an empty array.
    let messages = run(
        &dir,
        &[code_action_request(
            2,
            "untitled:Untitled-1",
            r,
            Some(vec!["refactor"]),
        )],
    );
    assert_eq!(*response_result(&messages, 2), json!([]));
}

#[test]
fn only_with_a_narrower_quickfix_subkind_still_short_circuits() {
    // This server only ever emits the bare "quickfix" kind - a request
    // for the narrower "quickfix.remove" must still short-circuit to
    // empty, not proceed (the old, backwards `starts_with("quickfix.")`
    // check would have gotten this one wrong).
    let dir = TempDir::new("ca_only_narrower");
    let r = point_range(json!({"line": 0, "character": 0}));
    let messages = run(
        &dir,
        &[code_action_request(
            2,
            "untitled:Untitled-1",
            r,
            Some(vec!["quickfix.remove"]),
        )],
    );
    assert_eq!(*response_result(&messages, 2), json!([]));
}

#[test]
fn code_action_reuses_the_diagnostics_computed_at_didopen_without_a_fresh_recheck() {
    let dir = TempDir::new("ca_cache_stable");
    let src = "<?php function f() { echo $undefinedVariable; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let r = point_range(lsp_pos(src, "$undefinedVariable"));

    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": src}}),
            ),
            code_action_request(2, &uri, r.clone(), None),
            code_action_request(3, &uri, r, None),
        ],
    );

    assert_eq!(response_result(&messages, 2), response_result(&messages, 3));
}

#[test]
fn code_action_for_a_workspace_file_never_opened_still_returns_diagnostics() {
    let dir = TempDir::new("ca_cache_miss_fallback");
    let src = "<?php function f() { echo $undefinedVariable; }";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let r = point_range(lsp_pos(src, "$undefinedVariable"));

    // No didOpen - a.php is only ever seen via the workspace scan, so
    // last_diagnostics has no entry for it yet when this request arrives.
    let messages = run(&dir, &[code_action_request(2, &uri, r, None)]);
    let result = response_result(&messages, 2);
    assert_eq!(
        *result,
        json!([]),
        "the cache-miss fallback must still compute a real (if empty-of-fixes) answer, got: {result:?}"
    );
}
