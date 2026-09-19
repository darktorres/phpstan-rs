//! `workspace/symbol`: capability advertisement and end-to-end
//! request/response shape, driven through the real wire protocol.
//! `workspace_symbol::tests` covers the ranking/matching logic itself
//! directly against a `Project`.

use super::helpers::run;
use super::helpers::*;
use serde_json::json;

#[test]
fn initialize_advertises_workspace_symbol_support() {
    let dir = TempDir::new("ws_caps");
    let messages = run(&dir, &[]);
    assert_eq!(
        messages[0]["result"]["capabilities"]["workspaceSymbolProvider"],
        json!(true)
    );
}

#[test]
fn workspace_symbol_finds_a_class_declared_in_a_different_file() {
    let dir = TempDir::new("ws_cross_file");
    let a_path = dir.write("a.php", "<?php class NeedleClass {}");
    dir.write("b.php", "<?php class Unrelated {}");

    let messages = run(&dir, &[workspace_symbol_request(2, "Needle")]);

    let result = response_result(&messages, 2);
    let symbols = result.as_array().unwrap();
    assert_eq!(symbols.len(), 1, "got: {symbols:?}");
    assert_eq!(symbols[0]["name"], "NeedleClass");
    assert_eq!(
        symbols[0]["kind"],
        serde_json::to_value(lsp_types::SymbolKind::CLASS).unwrap()
    );
    assert!(
        symbols[0]["location"]["uri"]
            .as_str()
            .unwrap()
            .ends_with("a.php"),
        "got: {symbols:?}, expected a.php: {a_path:?}"
    );
}
