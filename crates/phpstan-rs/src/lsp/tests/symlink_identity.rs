//! A document opened through a link is the file the link names. The server
//! keys the file by its resolved path, so its diagnostics are published on
//! the URI the editor opened, and navigation from it resolves in the
//! target's own files.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::json;
use std::fs;

#[cfg(unix)]
#[test]
fn a_document_opened_through_a_link_publishes_on_its_uri_and_resolves_in_the_target() {
    let dir = TempDir::new("link_document");
    let base = fs::canonicalize(&dir.0).unwrap();
    dir.write(
        "real/a.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    std::os::unix::fs::symlink("real", dir.0.join("link")).unwrap();
    let src = "<?php function useGreeter(Greeter $g) { return $g->greet(); }";
    dir.write("real/b.php", src);
    let link_uri = path_to_uri(&dir.0.join("link/b.php")).to_string();

    let open = notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": link_uri, "languageId": "php", "version": 1, "text": src}}),
    );
    let pos = lsp_pos(src, "Greeter $g");
    let messages = run(&dir, &[open, definition_request(2, &link_uri, pos)]);

    last_publish_diagnostics_for(&messages, &link_uri);
    let result = response_result(&messages, 2);
    assert_eq!(
        result["uri"],
        path_to_uri(&base.join("real/a.php")).to_string()
    );
}
