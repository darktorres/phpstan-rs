//! `textDocument/completion`, driven through the real wire protocol:
//! capability advertisement, bare class-name completion with/without an
//! auto-import edit, member completion via `->`/`::` (including the
//! cheap literal-class-name `::` path), and the two scenarios this
//! item's own design was corrected against - a mid-edit statement that
//! must be truncated (not doubled up) and an enclosing function found
//! by name despite lines shifting above it since the last good parse.
//! `completion::tests` covers the pure token-scan/import-decision logic
//! directly, with no `Project`/wire protocol involved.

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

/// Opens `valid_src` (which must parse cleanly) then edits it to
/// `edited_src` (which very often does NOT parse - that's the whole
/// point: completion is most commonly triggered right after typing
/// `->`/`::`/a fresh identifier, none of which are complete PHP on
/// their own) - `Project::ast`'s last-good AST then still reflects
/// `valid_src`, exactly the real editing sequence this item's whole
/// probe mechanism is built around.
fn run_edit(
    dir: &TempDir,
    uri: &str,
    valid_src: &str,
    edited_src: &str,
    extra: &[Value],
) -> Vec<Value> {
    let mut messages = vec![did_open(uri, valid_src), did_change(uri, 2, edited_src)];
    messages.extend_from_slice(extra);
    run(dir, &messages)
}

/// Same as `run_edit`, but for a test whose own probe specifically needs
/// `Workspace::latest_text` to already reflect `edited_src` - `didChange` is
/// debounced (VSCode plan item 2.2), so `run_edit`'s own single-batch
/// shape no longer guarantees that by the time `extra`'s own request
/// runs. See `helpers::run_across_debounce`'s own doc comment.
fn run_edit_debounced(
    dir: &TempDir,
    uri: &str,
    valid_src: &str,
    edited_src: &str,
    extra: &[Value],
) -> Vec<Value> {
    run_across_debounce(
        dir,
        &[did_open(uri, valid_src), did_change(uri, 2, edited_src)],
        extra,
    )
}

fn labels(result: &Value) -> Vec<&str> {
    result
        .as_array()
        .unwrap_or_else(|| panic!("expected a completion array, got: {result:?}"))
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect()
}

#[test]
fn initialize_advertises_completion_provider() {
    let dir = TempDir::new("completion_caps");
    let messages = run(&dir, &[]);
    let triggers: Vec<&str> =
        messages[0]["result"]["capabilities"]["completionProvider"]["triggerCharacters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
    assert!(triggers.contains(&">"), "got: {triggers:?}");
    assert!(triggers.contains(&":"), "got: {triggers:?}");
}

#[test]
fn bare_completion_offers_a_class_with_an_auto_import_edit() {
    let dir = TempDir::new("completion_bare_import");
    dir.write("a.php", "<?php\nnamespace App;\nclass Widget {}\n");
    let valid_src = "<?php\nnamespace Other;\n";
    let edited_src = "<?php\nnamespace Other;\nWid";
    let path = dir.write("b.php", valid_src);
    let uri = path_to_uri(&path).to_string();

    let messages = run_edit(
        &dir,
        &uri,
        valid_src,
        edited_src,
        &[completion_request(3, &uri, pos_after(edited_src, "Wid"))],
    );

    let result = response_result(&messages, 3);
    let items = result.as_array().unwrap();
    let widget = items
        .iter()
        .find(|i| i["label"] == "Widget")
        .unwrap_or_else(|| panic!("expected a Widget completion item, got: {items:?}"));
    let edits = widget["additionalTextEdits"]
        .as_array()
        .unwrap_or_else(|| panic!("expected an auto-import edit, got: {widget:?}"));
    assert!(
        edits[0]["newText"]
            .as_str()
            .unwrap()
            .contains("use App\\Widget;"),
        "got: {edits:?}"
    );
}

#[test]
fn bare_completion_needs_no_import_when_already_in_the_same_namespace() {
    let dir = TempDir::new("completion_bare_same_ns");
    let valid_src = "<?php\nnamespace App;\nclass Widget {}\n";
    let edited_src = "<?php\nnamespace App;\nclass Widget {}\nWid";
    let path = dir.write("a.php", valid_src);
    let uri = path_to_uri(&path).to_string();

    let messages = run_edit(
        &dir,
        &uri,
        valid_src,
        edited_src,
        &[completion_request(3, &uri, pos_after(edited_src, "Wid"))],
    );

    let result = response_result(&messages, 3);
    let items = result.as_array().unwrap();
    let widget = items
        .iter()
        .find(|i| i["label"] == "Widget")
        .unwrap_or_else(|| panic!("got: {items:?}"));
    assert_eq!(widget["additionalTextEdits"], Value::Null);
}

#[test]
fn member_completion_after_arrow_lists_methods_and_properties() {
    let dir = TempDir::new("completion_member_arrow");
    let valid_src = "<?php\nclass Greeter {\n    public $name;\n    function greet(): string { return 'hi'; }\n}\nfunction f(Greeter $g) {\n    $g->greet();\n}\n";
    let edited_src = "<?php\nclass Greeter {\n    public $name;\n    function greet(): string { return 'hi'; }\n}\nfunction f(Greeter $g) {\n    $g->\n}\n";
    let path = dir.write("a.php", valid_src);
    let uri = path_to_uri(&path).to_string();

    let messages = run_edit(
        &dir,
        &uri,
        valid_src,
        edited_src,
        &[completion_request(3, &uri, pos_after(edited_src, "$g->"))],
    );

    let result = response_result(&messages, 3);
    let got = labels(result);
    assert!(got.contains(&"greet"), "got: {got:?}");
    assert!(got.contains(&"name"), "got: {got:?}");
}

#[test]
fn member_completion_after_double_colon_on_a_literal_class_lists_enum_cases() {
    let dir = TempDir::new("completion_enum_colon");
    let valid_src = "<?php\nenum Suit: string {\n    case Hearts = 'H';\n    case Spades = 'S';\n}\nfunction f() {\n    Suit::Hearts;\n}\n";
    let edited_src = "<?php\nenum Suit: string {\n    case Hearts = 'H';\n    case Spades = 'S';\n}\nfunction f() {\n    Suit::\n}\n";
    let path = dir.write("a.php", valid_src);
    let uri = path_to_uri(&path).to_string();

    let messages = run_edit(
        &dir,
        &uri,
        valid_src,
        edited_src,
        &[completion_request(3, &uri, pos_after(edited_src, "Suit::"))],
    );

    let result = response_result(&messages, 3);
    let items = result.as_array().unwrap();
    let hearts = items
        .iter()
        .find(|i| i["label"] == "Hearts")
        .unwrap_or_else(|| panic!("got: {items:?}"));
    assert_eq!(
        hearts["kind"],
        serde_json::to_value(lsp_types::CompletionItemKind::ENUM_MEMBER).unwrap()
    );
}

#[test]
fn member_completion_truncates_a_stale_mid_edit_statement_instead_of_doubling_it() {
    // The design this item shipped against originally spliced the probe
    // by "inserting before the next statement", which left the STALE,
    // pre-edit version of the statement currently being typed still in
    // place, re-running its own side effects redundantly ahead of the
    // fresh probe. Truncating (dropping every statement reaching the
    // edit line, appending fresh) instead means there is nothing stale
    // left to double up - this just needs to still resolve correctly
    // via the parameter-seeded type, not the (now-dropped) body
    // statement.
    let dir = TempDir::new("completion_stale_statement");
    let valid_src = "<?php\nclass Counter {\n    function increment(): static { return $this; }\n    function value(): int { return 0; }\n}\nfunction f(Counter $c) {\n    $c = $c->increment();\n}\n";
    let edited_src = "<?php\nclass Counter {\n    function increment(): static { return $this; }\n    function value(): int { return 0; }\n}\nfunction f(Counter $c) {\n    $c = $c->increment()->\n}\n";
    let path = dir.write("a.php", valid_src);
    let uri = path_to_uri(&path).to_string();

    let messages = run_edit_debounced(
        &dir,
        &uri,
        valid_src,
        edited_src,
        &[completion_request(
            3,
            &uri,
            pos_after(edited_src, "$c = $c->increment()->"),
        )],
    );

    let result = response_result(&messages, 3);
    let got = labels(result);
    assert!(got.contains(&"increment"), "got: {got:?}");
    assert!(got.contains(&"value"), "got: {got:?}");
}

#[test]
fn member_completion_finds_the_enclosing_function_by_name_despite_lines_shifting_above_it() {
    // Lines inserted above the trigger (comments here) shift every real
    // line number after that point relative to the last-good AST's own
    // (now stale) spans. A line-based scope lookup could land inside a
    // numerically-coincidental but wrong enclosing function; this
    // project's own scope lookup is name-based instead, via a backward
    // scan over the LIVE buffer's own tokens, so it's unaffected.
    let dir = TempDir::new("completion_line_drift");
    let valid_src = "<?php\nclass Counter {\n    function value(): int { return 0; }\n}\nfunction helperOne() {}\nfunction helperTwo(Counter $c) {\n    $c->value();\n}\n";
    let edited_src = "<?php\n// one\n// two\n// three\nclass Counter {\n    function value(): int { return 0; }\n}\nfunction helperOne() {}\nfunction helperTwo(Counter $c) {\n    $c->\n}\n";
    let path = dir.write("a.php", valid_src);
    let uri = path_to_uri(&path).to_string();

    let messages = run_edit_debounced(
        &dir,
        &uri,
        valid_src,
        edited_src,
        &[completion_request(3, &uri, pos_after(edited_src, "$c->"))],
    );

    let result = response_result(&messages, 3);
    let got = labels(result);
    assert!(got.contains(&"value"), "got: {got:?}");
}
