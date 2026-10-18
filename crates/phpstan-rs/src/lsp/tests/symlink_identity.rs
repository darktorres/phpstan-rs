//! A document opened through a link is the file the link names. The server
//! keys the file by its resolved path, so its diagnostics are published on
//! the URI the editor opened, and navigation from it resolves in the
//! target's own files. A file the editor has not opened is named under its
//! folder's root as the editor named that root. A file's diagnostics are
//! cleared on the URI they were sent on, and move with the file when the
//! editor comes to know it by another.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use crate::test_links::link_dir;
use serde_json::json;

#[test]
fn a_document_opened_through_a_link_publishes_on_its_uri_and_resolves_in_the_target() {
    let dir = TempDir::new("link_document");
    dir.write(
        "real/a.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    link_dir("real", dir.0.join("link"));
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
    link_dir("real", dir.0.join("link"));
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

#[test]
fn a_workspace_opened_through_a_link_and_removed_is_cleared_on_the_links_uris() {
    let dir = TempDir::new("link_workspace_removed");
    dir.write(
        "real/unopened.php",
        "<?php function f() { echo $undefinedInLinked; }",
    );
    link_dir("real", dir.0.join("link"));
    let link = dir.0.join("link");
    let link_uri = path_to_uri(&link).to_string();
    let unopened_uri = path_to_uri(&link.join("unopened.php")).to_string();

    let removed = json!({"uri": link_uri, "name": "link"});
    let messages = run_rooted(
        &link_uri,
        json!({}),
        &[did_change_workspace_folders(vec![], vec![removed])],
    );

    let published: Vec<(&str, &Vec<serde_json::Value>)> = messages
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .map(|m| {
            (
                m["params"]["uri"].as_str().unwrap(),
                m["params"]["diagnostics"].as_array().unwrap(),
            )
        })
        .collect();
    assert!(
        matches!(
            published.as_slice(),
            [(first, sent), (last, cleared)]
                if *first == unopened_uri
                    && *last == unopened_uri
                    && sent.iter().any(|d| d["code"] == "variable.undefined")
                    && cleared.is_empty()
        ),
        "the file is published and then cleared on the link's URI alone, got: {published:?}"
    );
}

#[test]
fn a_file_opened_by_another_spelling_has_its_diagnostics_moved_to_that_uri() {
    let dir = TempDir::new("link_workspace_moved");
    let src = "<?php function f() { echo $undefinedInLinked; }";
    dir.write("real/a.php", src);
    link_dir("real", dir.0.join("link"));
    let link = dir.0.join("link");
    let link_uri = path_to_uri(&link).to_string();
    let linked_uri = path_to_uri(&link.join("a.php")).to_string();
    let real_uri = path_to_uri(&dir.0.join("real/a.php")).to_string();

    let open = notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": real_uri, "languageId": "php", "version": 1, "text": src}}),
    );
    let messages = run_rooted(&link_uri, json!({}), &[open]);

    assert_eq!(
        last_publish_diagnostics_for(&messages, &linked_uri)["params"]["diagnostics"],
        json!([]),
        "the URI the diagnostics were first sent on is cleared, got: {messages:?}"
    );
    let moved = last_publish_diagnostics_for(&messages, &real_uri);
    assert!(
        moved["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "variable.undefined"),
        "the diagnostics move to the URI the file was opened by, got: {moved:?}"
    );
}
