//! The load of a workspace folder runs on the pool, beside the message
//! loop: what the client sends for a document under the folder meanwhile
//! waits, and is applied as the workspace is installed.

use super::super::*;
use super::helpers::*;
use serde_json::{Value, json};

fn server() -> Server {
    Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema())
}

fn to_values(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .collect()
}

fn codes_published_for(messages: &[Value], uri: &str) -> Vec<Vec<String>> {
    messages
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .map(|m| {
            m["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|d| d["code"].as_str().map(str::to_string))
                .collect()
        })
        .collect()
}

#[test]
fn starting_the_load_leaves_the_folder_loading_until_the_loop_installs_it() {
    let dir = TempDir::new("loading_pending");
    dir.write("A.php", "<?php class A {}");
    let mut server = server();

    server.start_loading(std::slice::from_ref(&dir.0));
    assert!(server.folders.is_empty());
    assert_eq!(server.loading.len(), 1);

    let messages = server.finish_loads();
    assert!(messages.is_empty(), "nothing to publish: {messages:?}");
    assert!(server.loading.is_empty());
    assert_eq!(server.folders.len(), 1);
    assert!(
        server.folders[0]
            .project
            .registry()
            .classes()
            .contains_key("A")
    );
}

#[test]
fn a_document_opened_and_edited_while_the_folder_loads_is_applied_as_last_sent() {
    let dir = TempDir::new("loading_documents");
    let path = dir.write("A.php", "<?php class OnDisk {}");
    let uri = path_to_uri(&path);
    let mut server = server();

    server.start_loading(std::slice::from_ref(&dir.0));
    assert!(
        server
            .handle_open_or_change(&uri, "<?php class First {}", Some(1))
            .is_empty()
    );
    assert!(
        server
            .handle_debounced_change(&path, &uri, "<?php function f(Missing $m) {}", 2, || false)
            .is_empty()
    );
    assert!(server.folders.is_empty());

    let messages = to_values(&server.finish_loads());

    let folder = &server.folders[0];
    assert!(folder.open_documents.contains(&path));
    assert!(!folder.project.registry().classes().contains_key("OnDisk"));
    assert!(!folder.project.registry().classes().contains_key("First"));
    let published = codes_published_for(&messages, &uri.to_string());
    assert!(
        published
            .last()
            .is_some_and(|codes| codes.iter().any(|c| c == "class.notFound")),
        "expected the last text sent to be what is published, got: {messages:?}"
    );
    assert_eq!(
        messages
            .iter()
            .find(|m| m["params"]["uri"] == uri.to_string().as_str())
            .map(|m| m["params"]["version"].clone()),
        Some(json!(2))
    );
}

#[test]
fn a_document_closed_while_the_folder_loads_is_not_applied() {
    let dir = TempDir::new("loading_closed");
    let path = dir.write("A.php", "<?php class OnDisk {}");
    let uri = path_to_uri(&path);
    let mut server = server();

    server.start_loading(std::slice::from_ref(&dir.0));
    server.handle_open_or_change(&uri, "<?php class InEditor {}", Some(1));
    server.close_document(&path);
    server.finish_loads();

    let folder = &server.folders[0];
    assert!(!folder.open_documents.contains(&path));
    assert!(folder.project.registry().classes().contains_key("OnDisk"));
}

#[test]
fn a_watched_file_event_while_the_folder_loads_is_replayed_on_install() {
    let dir = TempDir::new("loading_watched");
    dir.write("A.php", "<?php class A {}");
    let mut server = server();

    server.start_loading(std::slice::from_ref(&dir.0));
    let late = dir.write("Late.php", "<?php class Late {}");
    let replies = server.handle_watched_files_changed(vec![lsp_types::FileEvent::new(
        path_to_uri(&late),
        lsp_types::FileChangeType::CREATED,
    )]);
    assert!(replies.is_empty());

    server.finish_loads();

    let folder = &server.folders[0];
    assert!(folder.project.is_loaded(&late));
    assert!(folder.project.registry().classes().contains_key("Late"));
}

#[test]
fn a_request_is_answered_once_the_folders_it_needs_are_installed() {
    let dir = TempDir::new("loading_request");
    let src = "<?php class Foo {}\nfunction f(Foo $x) {}";
    let path = dir.write("A.php", src);
    let uri = path_to_uri(&path).to_string();
    let mut server = server();
    server.start_loading(std::slice::from_ref(&dir.0));

    let mut writer = Vec::new();
    handle_request(
        &mut server,
        &mut writer,
        lsp_server::Request::new(
            lsp_server::RequestId::from(7),
            "textDocument/definition".to_string(),
            json!({"textDocument": {"uri": uri}, "position": lsp_pos(src, "Foo $x")}),
        ),
    );

    let messages = decode_all(&writer);
    let answer = response_result(&messages, 7);
    assert!(
        answer["uri"].as_str().unwrap_or("").ends_with("A.php"),
        "got: {answer:?}"
    );
    assert!(server.loading.is_empty());
}

#[test]
fn the_scan_reports_progress_until_the_last_folder_is_installed() {
    let a = TempDir::new("loading_progress_a");
    a.write("A.php", "<?php class A {}");
    let b = TempDir::new("loading_progress_b");
    b.write("B.php", "<?php class B {}");
    let mut server = server();

    let begin = server.start_scan(PendingWorkspaceScan {
        roots: vec![a.0.clone(), b.0.clone()],
        progress_token: Some(lsp_types::ProgressToken::String("scan".to_string())),
    });
    let rest = server.finish_loads();

    let methods: Vec<String> = begin
        .iter()
        .chain(&rest)
        .map(|m| serde_json::to_value(m).unwrap())
        .map(|m| {
            m["params"]["value"]["kind"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(methods, ["begin", "end"], "got: {begin:?} then {rest:?}");
}
