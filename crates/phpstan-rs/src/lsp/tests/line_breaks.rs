//! A document whose lines end in `\r\n`, as a checkout on Windows writes
//! them, or in `\r` alone is the document that ends them in `\n`: its
//! diagnostics sit at the same ranges, and a position on a later line names
//! the same symbol.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

const SOURCE: &str = concat!(
    "<?php\n",
    "class Greeter {\n",
    "    public function greet(): string { return 'hi'; }\n",
    "}\n",
    "function useGreeter(Greeter $g) {\n",
    "    echo $undefinedVariable;\n",
    "    return $g->greet();\n",
    "}\n",
);

/// What a client gets for `SOURCE` written with `line_break` and opened
/// as a document: the diagnostics published for it and the answers to a
/// definition request on `Greeter` in the signature, a hover on `greet`
/// and a hover at the very end of the signature's line.
fn answers(label: &str, line_break: &str) -> (Value, Value, Value, Value) {
    let dir = TempDir::new(label);
    let text = SOURCE.replace('\n', line_break);
    let path = dir.write("use.php", &text);
    let uri = path_to_uri(&path).to_string();
    // The positions are those of the document with line feeds, which the
    // other spellings must share.
    let signature = lsp_pos(SOURCE, "Greeter $g");
    let call = lsp_pos(SOURCE, "greet();");
    let line_end = json!({
        "line": signature["line"],
        "character": "function useGreeter(Greeter $g) {".chars().count(),
    });
    let open = notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": text}}),
    );
    let messages = run(
        &dir,
        &[
            open,
            definition_request(2, &uri, signature),
            hover_request(3, &uri, call),
            hover_request(4, &uri, line_end),
        ],
    );
    let diagnostics = last_publish_diagnostics_for(&messages, &uri)["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| json!({"range": d["range"], "code": d["code"], "message": d["message"]}))
        .collect::<Vec<_>>();
    let definition = response_result(&messages, 2).clone();
    // The definition's URI is the document's, which is not the same in
    // each directory.
    let definition = json!({"range": definition["range"], "same_file": definition["uri"] == uri});
    (
        json!(diagnostics),
        definition,
        response_result(&messages, 3).clone(),
        response_result(&messages, 4).clone(),
    )
}

#[test]
fn a_document_is_answered_at_the_same_positions_whatever_its_line_breaks() {
    let lf = answers("breaks_lf", "\n");
    assert!(
        lf.0.as_array()
            .unwrap()
            .iter()
            .any(|d| d["range"]["start"]["line"] == 5),
        "the undefined variable is reported on its line: {lf:?}"
    );
    assert_eq!(lf.1["same_file"], true, "{lf:?}");
    assert_eq!(lf.1["range"]["start"]["line"], 1, "{lf:?}");
    assert!(
        lf.2.is_object(),
        "the hover on greet() names the method: {lf:?}"
    );
    assert_eq!(lf.3, Value::Null, "{lf:?}");
    for (label, line_break) in [("breaks_crlf", "\r\n"), ("breaks_cr", "\r")] {
        assert_eq!(answers(label, line_break), lf, "{line_break:?}");
    }
}
