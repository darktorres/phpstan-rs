//! `textDocument/inlayHint`: parameter-name hints for call/constructor/
//! method arguments (including `call_user_func`/`call_user_func_array`),
//! inferred local-variable type hints, and the version-keyed cache that
//! avoids paying a full `check_program` pass on every unchanged-document
//! request - driven through the real wire protocol, matching
//! signature_help.rs's own test style.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

fn did_open(uri: &str, text: &str) -> Value {
    notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": text}}),
    )
}

fn did_change(uri: &str, version: i64, text: &str) -> Value {
    notification(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}),
    )
}

/// `Null` (no hints at all - `Server::inlay_hints`'s own empty-response
/// convention, matching hover/definition/documentHighlight) becomes an
/// empty `Vec` here, so a test asserting "no hint of kind X" doesn't need
/// to special-case the all-skipped case separately from a real array
/// that merely contains no matching hint.
fn hints(result: &Value) -> Vec<&Value> {
    match result {
        Value::Null => Vec::new(),
        Value::Array(a) => a.iter().collect(),
        other => panic!("expected an inlayHint array or null, got: {other:?}"),
    }
}

fn label(h: &Value) -> &str {
    h["label"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a string label, got: {h:?}"))
}

fn range(start: Value, end: Value) -> Value {
    json!({"start": start, "end": end})
}

#[test]
fn initialize_advertises_inlay_hint_support() {
    let dir = TempDir::new("ih_caps");
    let messages = run(&dir, &[]);
    assert_eq!(
        messages[0]["result"]["capabilities"]["inlayHintProvider"],
        json!(true)
    );
}

#[test]
fn inlay_hint_shows_parameter_names_before_call_arguments() {
    let dir = TempDir::new("ih_params");
    let src = "<?php\nfunction greet(string $name, int $times) {}\ngreet('hi', 3);\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    let name_hint = hs
        .iter()
        .find(|h| h["position"] == lsp_pos(src, "'hi'"))
        .unwrap_or_else(|| panic!("no hint at 'hi', got: {hs:?}"));
    assert_eq!(label(name_hint), "name:");
    assert_eq!(name_hint["kind"], json!(2));
    let times_hint = hs
        .iter()
        .find(|h| h["position"] == lsp_pos(src, "3);"))
        .unwrap_or_else(|| panic!("no hint at 3, got: {hs:?}"));
    assert_eq!(label(times_hint), "times:");
}

#[test]
fn inlay_hint_skips_named_and_spread_arguments() {
    let dir = TempDir::new("ih_named_spread");
    let src = "<?php\nfunction greet(string $name, ...$rest) {}\ngreet(name: 'hi', ...['x']);\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    assert!(
        hs.iter().all(|h| h["kind"] != json!(2)),
        "expected no parameter-name hints for a named or spread argument, got: {hs:?}"
    );
}

#[test]
fn inlay_hint_uses_the_declared_parameter_name_for_variadic_overflow() {
    let dir = TempDir::new("ih_variadic");
    let src = "<?php\nfunction f(string $first, int ...$rest) {}\nf('a', 1, 2, 3);\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    for needle in ["1, ", "2, ", "3);"] {
        let h = hs
            .iter()
            .find(|h| h["position"] == lsp_pos(src, needle))
            .unwrap_or_else(|| panic!("no hint at {needle:?}, got: {hs:?}"));
        assert_eq!(label(h), "rest:");
    }
}

#[test]
fn inlay_hint_shows_inferred_type_after_an_unannotated_assignment() {
    let dir = TempDir::new("ih_local_type");
    let src = "<?php\nfunction f(int $n) {\n    $x = $n;\n    return $x;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    let h = hs
        .iter()
        .find(|h| h["kind"] == json!(1))
        .unwrap_or_else(|| panic!("expected a TYPE hint, got: {hs:?}"));
    assert_eq!(label(h), ": int");
    assert_eq!(h["position"], lsp_pos(src, " = $n;"));
}

#[test]
fn inlay_hint_suppresses_type_hint_when_an_inline_var_annotation_already_covers_it() {
    let dir = TempDir::new("ih_inline_var_suppress");
    let src =
        "<?php\nclass Bar {}\nfunction f() {\n    /** @var Bar $x */\n    $x = new Bar();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    assert!(
        hs.iter().all(|h| h["kind"] != json!(1)),
        "expected the @var Bar $x annotation to suppress the type hint, got: {hs:?}"
    );
}

#[test]
fn inlay_hint_response_is_filtered_to_the_requested_range() {
    let dir = TempDir::new("ih_range_filter");
    let src = "<?php\nfunction f(int $a, int $b) {\n    $x = $a;\n    $y = $b;\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    // Covers only the first assignment's line.
    let narrow = range(
        json!({"line": 2, "character": 0}),
        json!({"line": 2, "character": 20}),
    );

    let messages = run(&dir, &[inlay_hint_request(2, &uri, narrow)]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    assert_eq!(
        hs.len(),
        1,
        "expected only the first assignment's hint, got: {hs:?}"
    );
    assert_eq!(hs[0]["position"], lsp_pos(src, " = $a;"));
}

#[test]
fn inlay_hint_skips_a_mixed_typed_assignment() {
    let dir = TempDir::new("ih_mixed_skip");
    let src = "<?php\nfunction f() {\n    $x = someUndefinedThing();\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    assert!(
        hints(&result).iter().all(|h| h["kind"] != json!(1)),
        "expected no TYPE hint for a Mixed-inferred assignment, got: {result:?}"
    );
}

#[test]
fn inlay_hint_covers_method_calls_and_new_expressions_too() {
    let dir = TempDir::new("ih_method_and_new");
    let src = "<?php\nclass Foo {\n    function __construct(int $arg) {}\n    function bar(int $arg): void {}\n}\nfunction f(Foo $obj) {\n    new Foo(1);\n    $obj->bar(2);\n}\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    let ctor_hint = hs
        .iter()
        .find(|h| h["position"] == lsp_pos(src, "1);"))
        .unwrap_or_else(|| panic!("no constructor-argument hint, got: {hs:?}"));
    assert_eq!(label(ctor_hint), "arg:");
    let method_hint = hs
        .iter()
        .find(|h| h["position"] == lsp_pos(src, "2);"))
        .unwrap_or_else(|| panic!("no method-call-argument hint, got: {hs:?}"));
    assert_eq!(label(method_hint), "arg:");
}

#[test]
fn inlay_hint_shows_parameter_names_for_call_user_func_and_call_user_func_array() {
    let dir = TempDir::new("ih_call_user_func");
    let src = "<?php\nfunction greet(string $name) {}\ncall_user_func('greet', 'hi');\ncall_user_func_array('greet', ['hi']);\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[inlay_hint_request(2, &uri, whole_file_range())]);
    let result = response_result(&messages, 2);
    let hs = hints(&result);
    let name_hints: Vec<&&Value> = hs.iter().filter(|h| label(h) == "name:").collect();
    assert_eq!(
        name_hints.len(),
        2,
        "expected both call_user_func and call_user_func_array's 'hi' argument hinted, got: {hs:?}"
    );
}

#[test]
fn inlay_hint_reflects_a_new_call_added_after_a_didchange_edit() {
    let dir = TempDir::new("ih_cache_invalidation");
    let initial_src = "<?php\nfunction greet(string $name) {}\ngreet('hi');\n";
    let edited_src = "<?php\nfunction greet(string $name) {}\ngreet('hi');\ngreet('bye');\n";
    let path = dir.write("a.php", initial_src);
    let uri = path_to_uri(&path).to_string();

    // `didChange` is debounced (VSCode plan item 2.2) - the second
    // request must run only after the worker's own timer has actually
    // applied the edit, not just after it's been queued, so this uses
    // `run_across_debounce` rather than a single synchronous batch.
    let messages = run_across_debounce(
        &dir,
        &[
            did_open(&uri, initial_src),
            inlay_hint_request(2, &uri, whole_file_range()),
            did_change(&uri, 2, edited_src),
        ],
        &[inlay_hint_request(3, &uri, whole_file_range())],
    );

    let first = hints(&response_result(&messages, 2)).len();
    let result_after_edit = response_result(&messages, 3);
    let second_hints = hints(&result_after_edit);
    assert_eq!(
        first, 1,
        "expected exactly one hint before the edit, got: {second_hints:?}"
    );
    assert!(
        second_hints
            .iter()
            .any(|h| h["position"] == lsp_pos(edited_src, "'bye'")),
        "expected the newly-added greet('bye') call to be hinted after the edit, got: {second_hints:?}"
    );
    assert_eq!(
        second_hints.len(),
        2,
        "expected the cache to reflect both calls after the edit, not a stale single-hint response, got: {second_hints:?}"
    );
}
