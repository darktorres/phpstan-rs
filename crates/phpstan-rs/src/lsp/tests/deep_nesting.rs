//! A deeply nested file typed into an open editor. The debounce worker
//! applies the edit on its own thread, and that thread keeps the default
//! stack, so a parse, check or drop that recurses on every level of the
//! file must not take the server down with it.

use super::super::*;
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

fn nested_arrays(depth: usize) -> String {
    format!("<?php\n$a = {}1{};\n", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn a_deeply_nested_edit_applied_by_the_debounce_worker_publishes_diagnostics() {
    let dir = TempDir::new("deep_nesting_edit");
    let initial = "<?php\n$a = 1;\n";
    let deep = nested_arrays(120);
    let path = dir.write("a.php", initial);
    let uri = path_to_uri(&path).to_string();

    let messages = run_across_debounce(
        &dir,
        &[did_open(&uri, initial), did_change(&uri, 2, &deep)],
        &[],
    );

    assert!(
        messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == json!(uri)),
        "the server answered the deep edit with no diagnostics for the file: {messages:?}"
    );
}
