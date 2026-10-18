//! A document opened through a link is the file the link names. The server
//! keys the file by its resolved path, so its diagnostics are published on
//! the URI the editor opened, and navigation from it resolves in the
//! target's own files. A file the editor has not opened is named under its
//! folder's root as the editor named that root.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::json;

#[cfg(unix)]
#[test]
fn a_document_opened_through_a_link_publishes_on_its_uri_and_resolves_in_the_target() {
    let dir = TempDir::new("link_document");
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
        path_to_uri(&dir.0.join("real/a.php")).to_string()
    );
}

#[cfg(unix)]
#[test]
fn a_workspace_opened_through_a_link_is_answered_in_the_links_spelling() {
    let dir = TempDir::new("link_workspace");
    dir.write(
        "real/defs.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    dir.write(
        "real/unopened.php",
        "<?php function f() { echo $undefinedInLinked; }",
    );
    let src = "<?php function useGreeter(Greeter $g) { return $g->greet(); }";
    dir.write("real/use.php", src);
    std::os::unix::fs::symlink("real", dir.0.join("link")).unwrap();
    let link = dir.0.join("link");
    let link_uri = path_to_uri(&link).to_string();
    let use_uri = path_to_uri(&link.join("use.php")).to_string();

    let open = notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": use_uri, "languageId": "php", "version": 1, "text": src}}),
    );
    let pos = lsp_pos(src, "Greeter $g");
    let messages = run_rooted(
        &link_uri,
        json!({}),
        &[open, definition_request(2, &use_uri, pos)],
    );

    let unopened = last_publish_diagnostics_for(
        &messages,
        &path_to_uri(&link.join("unopened.php")).to_string(),
    );
    assert!(
        unopened["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "variable.undefined"),
        "got: {unopened:?}"
    );
    let published: Vec<&str> = messages
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .filter_map(|m| m["params"]["uri"].as_str())
        .collect();
    assert!(
        published
            .iter()
            .all(|uri| uri.starts_with(&format!("{link_uri}/"))),
        "every file is published under the link: {published:?}"
    );
    let result = response_result(&messages, 2);
    assert_eq!(
        result["uri"],
        path_to_uri(&link.join("defs.php")).to_string()
    );
}
