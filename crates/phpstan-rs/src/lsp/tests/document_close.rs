//! `textDocument/didClose`: a closed document goes back to its file on disk,
//! and an edit the debounce worker takes after its document has closed is
//! not applied.

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

fn did_close(uri: &str) -> Value {
    notification(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    )
}

/// Calls a function nothing defines, which is reported at every level.
const BROKEN_SRC: &str = "<?php\nundefined_function_for_close();\n";
const CLEAN_SRC: &str = "<?php\necho 1;\n";

#[test]
fn closing_a_document_checks_the_file_as_it_is_on_disk() {
    let dir = TempDir::new("close_restores_disk");
    let path = dir.write("a.php", CLEAN_SRC);
    let uri = path_to_uri(&path).to_string();

    let messages = run(&dir, &[did_open(&uri, BROKEN_SRC), did_close(&uri)]);

    let publishes = publishes_for(&messages, &uri);
    let last = publishes
        .last()
        .unwrap_or_else(|| panic!("no publishDiagnostics for {uri} in {messages:?}"));
    assert!(
        diagnostics_of(last).is_empty(),
        "the file on disk is clean, but the closed buffer's diagnostics were kept: {last:?}"
    );
}

#[test]
fn an_edit_taken_after_its_document_closed_is_not_applied() {
    let dir = TempDir::new("close_aborts_edit");
    let path = dir.write("a.php", CLEAN_SRC);
    let uri = path_to_uri(&path);
    let (mut server, _) = loaded_server(&dir);

    let messages = server.handle_debounced_change(&path, &uri, BROKEN_SRC, 2, &mut || true);

    assert!(messages.is_empty(), "got: {messages:?}");
    assert!(
        server.open_uris.is_empty(),
        "the aborted edit reopened the document: {:?}",
        server.open_uris.keys().collect::<Vec<_>>()
    );
}
