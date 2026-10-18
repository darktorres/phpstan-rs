//! What a disabled server does: nothing the editor can see. It publishes
//! only the clear of what the editor holds, checks nothing on an edit, a
//! save or a watched file, answers every feature request with `null`, and
//! catches up when it is enabled again.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

const DEPRECATION_RULE: &str = "[ext.deprecation-rules]\nenabled = true\n";

const CLEAN: &str = "<?php\nfunction oldFunc(): void {}\n";

const DEPRECATED_CALL: &str =
    "<?php\n/** @deprecated use newFunc() instead */\nfunction oldFunc(): void {}\noldFunc();\n";

const OTHER_DEPRECATED_CALL: &str =
    "<?php\n/** @deprecated use newFunc() instead */\nfunction oldOther(): void {}\noldOther();\n";

fn settings(enable: bool) -> settings::PhpstanRsSettings {
    settings::PhpstanRsSettings {
        enable,
        config_path: None,
    }
}

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

fn reports_deprecation(messages: &[Value], uri: &str) -> bool {
    publishes_for(messages, uri).iter().any(|publish| {
        diagnostics_of(publish)
            .iter()
            .any(|d| d["code"] == "function.deprecated")
    })
}

#[test]
fn disabling_clears_a_file_the_editor_never_opened() {
    let dir = TempDir::new("disabled_clears_unopened");
    dir.write("phpstan-rs.toml", DEPRECATION_RULE);
    let path = dir.write("a.php", DEPRECATED_CALL);
    let uri = path_to_uri(&path).to_string();
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        reports_deprecation(&initial, &uri),
        "the workspace scan publishes a.php, got: {initial:?}"
    );

    let disabled = as_values(server.reload_configuration(settings(false)));
    let published = publishes_for(&disabled, &uri);
    assert_eq!(published.len(), 1, "got: {disabled:?}");
    assert!(
        diagnostics_of(published[0]).is_empty(),
        "the clear of a file nothing opened, got: {disabled:?}"
    );
}

#[test]
fn a_disabled_server_checks_and_publishes_nothing_until_it_is_enabled() {
    let dir = TempDir::new("disabled_catches_up");
    dir.write("phpstan-rs.toml", DEPRECATION_RULE);
    let a = dir.write("a.php", CLEAN);
    let a_uri = path_to_uri(&a);
    let (mut server, initial) = loaded_server(&dir);
    assert!(
        publishes_for(&initial, a_uri.as_str()).is_empty(),
        "a.php starts clean, got: {initial:?}"
    );
    assert!(server.reload_configuration(settings(false)).is_empty());

    let opened = server.handle_open_or_change(&a_uri, CLEAN, Some(1));
    let edited = server.handle_debounced_change(&a, &a_uri, DEPRECATED_CALL, 2, &mut || false);
    let saved = server.analyse_on_save();
    let b = dir.write("b.php", OTHER_DEPRECATED_CALL);
    let b_uri = path_to_uri(&b);
    let watched = server.handle_watched_files_changed(vec![lsp_types::FileEvent::new(
        b_uri.clone(),
        lsp_types::FileChangeType::CREATED,
    )]);
    assert!(
        opened.is_empty() && edited.is_empty() && saved.is_empty() && watched.is_empty(),
        "a disabled server sends nothing: {opened:?} {edited:?} {saved:?} {watched:?}"
    );
    let key = server.identify(&a);
    let holds = |server: &Server, text: &str| {
        server.folders[0].slot.project.content_hash(&key)
            == Some(source::Digested::new(text.as_bytes()).digest())
    };
    assert!(
        holds(&server, CLEAN),
        "a disabled server leaves the project as it was: it checks nothing"
    );

    let enabled = as_values(server.reload_configuration(settings(true)));
    assert!(
        holds(&server, DEPRECATED_CALL),
        "enabling brings the project to the text the editor holds"
    );
    assert!(
        reports_deprecation(&enabled, a_uri.as_str()),
        "the edit made while disabled is checked once the server is enabled, got: {enabled:?}"
    );
    assert!(
        reports_deprecation(&enabled, b_uri.as_str()),
        "the file created while disabled is checked once the server is enabled, got: {enabled:?}"
    );
}

#[test]
fn a_folder_added_while_disabled_is_analysed_when_the_server_is_enabled() {
    let dir = TempDir::new("disabled_folder_added");
    dir.write("phpstan-rs.toml", DEPRECATION_RULE);
    let path = dir.write("a.php", DEPRECATED_CALL);
    let uri = path_to_uri(&path).to_string();
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    server.reload_configuration(settings(false));

    let loaded = server.add_workspace_folders(std::slice::from_ref(&dir.0));
    let analysed = server.publish_workspace_diagnostics();
    assert!(
        publishes_for(&as_values(loaded), &uri).is_empty() && analysed.is_empty(),
        "a disabled server publishes nothing for a folder it loads"
    );

    let enabled = as_values(server.reload_configuration(settings(true)));
    assert!(
        reports_deprecation(&enabled, &uri),
        "the folder is analysed once the server is enabled, got: {enabled:?}"
    );
}

#[test]
fn an_edit_sent_to_a_disabled_server_publishes_nothing() {
    let dir = TempDir::new("disabled_edit_over_the_wire");
    dir.write("phpstan-rs.toml", DEPRECATION_RULE);
    let path = dir.write("a.php", CLEAN);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[
            did_open(&uri, CLEAN),
            did_change_configuration(json!({"phpstan-rs": {"enable": false}})),
            did_open(&uri, DEPRECATED_CALL),
            did_change(&uri, 3, DEPRECATED_CALL),
        ],
    );

    assert!(
        !reports_deprecation(&messages, &uri),
        "a disabled server publishes no diagnostic for an edit, got: {messages:?}"
    );
}

#[test]
fn a_disabled_server_answers_every_feature_request_with_null() {
    let dir = TempDir::new("disabled_requests");
    dir.write("phpstan-rs.toml", DEPRECATION_RULE);
    let path = dir.write("a.php", DEPRECATED_CALL);
    let uri = path_to_uri(&path).to_string();
    let position = lsp_pos(DEPRECATED_CALL, "oldFunc()");
    let range = json!({"start": position, "end": position});
    let mut input = vec![
        did_open(&uri, DEPRECATED_CALL),
        did_change_configuration(json!({"phpstan-rs": {"enable": false}})),
    ];
    for (offset, method) in FEATURE_REQUESTS.iter().enumerate() {
        input.push(request(
            10 + offset as i64,
            method,
            json!({
                "textDocument": {"uri": uri},
                "position": position,
                "range": range,
                "context": {"diagnostics": [], "includeDeclaration": true},
                "query": "old",
                "newName": "renamed",
            }),
        ));
    }

    let messages = run(&dir, &input);

    for (offset, method) in FEATURE_REQUESTS.iter().enumerate() {
        let answer = response_result(&messages, 10 + offset as i64);
        assert!(
            answer.is_null(),
            "{method} answers a disabled server's empty result, got: {answer:?}"
        );
    }
}
