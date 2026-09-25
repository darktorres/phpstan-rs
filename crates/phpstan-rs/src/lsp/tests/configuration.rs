//! `workspace/didChangeConfiguration` hot-reload: the pushed-settings
//! path, the `workspace/configuration` pull path for a client that
//! advertises `workspace.configuration` support, the `enable`
//! squiggles-off/on toggle, and the no-op fast path a real client's
//! constant unrelated-settings-save traffic depends on.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};

const DEPRECATED_FIXTURE: &str =
    "<?php\n/** @deprecated use newFunc() instead */\nfunction oldFunc(): void {}\noldFunc();\n";

fn did_open(uri: &str, text: &str) -> Value {
    notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": text}}),
    )
}

fn publishes_for<'a>(messages: &'a [Value], uri: &str) -> Vec<&'a Value> {
    messages
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .collect()
}

#[test]
fn a_settings_save_unrelated_to_phpstan_rs_is_a_no_op() {
    let dir = TempDir::new("dcc_no_op");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(DEPRECATED_FIXTURE, "oldFunc()");

    let messages = run(
        &dir,
        &[
            did_open(&uri, DEPRECATED_FIXTURE),
            did_change_configuration(json!({})),
            hover_request(3, &uri, pos),
        ],
    );

    assert_eq!(
        publishes_for(&messages, &uri).len(),
        1,
        "a genuine no-op must not trigger a republish, got: {messages:?}"
    );
    let hover = response_result(&messages, 3);
    assert!(
        hover["contents"].is_string(),
        "the session must keep answering normally after a no-op reload, got: {hover:?}"
    );
}

#[test]
fn a_pushed_config_path_hot_applies_the_new_config_and_republishes() {
    let dir = TempDir::new("dcc_push_config_path");
    dir.write("other.toml", "check_deprecated_usage = true\n");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[
            did_open(&uri, DEPRECATED_FIXTURE),
            did_change_configuration(
                json!({"phpstan-rs": {"enable": true, "configPath": "other.toml"}}),
            ),
        ],
    );

    let publishes = publishes_for(&messages, &uri);
    assert!(
        publishes.len() >= 2,
        "expected didOpen's own publish plus a republish after the config-path switch, got: {messages:?}"
    );
    assert!(
        !publishes[0]["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "function.deprecated"),
        "no phpstan-rs.toml is configured at the root yet, got: {publishes:?}"
    );
    assert!(
        publishes.last().unwrap()["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "function.deprecated"),
        "other.toml's check_deprecated_usage=true must be hot-applied, got: {publishes:?}"
    );
}

#[test]
fn disabling_then_re_enabling_clears_then_restores_diagnostics_without_a_new_did_open() {
    let dir = TempDir::new("dcc_disable_reenable");
    dir.write("phpstan-rs.toml", "check_deprecated_usage = true\n");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(DEPRECATED_FIXTURE, "oldFunc()");

    let messages = run(
        &dir,
        &[
            did_open(&uri, DEPRECATED_FIXTURE),
            did_change_configuration(json!({"phpstan-rs": {"enable": false}})),
            hover_request(3, &uri, pos),
            did_change_configuration(json!({"phpstan-rs": {"enable": true}})),
        ],
    );

    let publishes = publishes_for(&messages, &uri);
    // The workspace scan (item 1.3/2.5) and didOpen's own recheck both
    // publish a.php's real diagnostics before any didChangeConfiguration
    // is even sent - matching item 1.3's own documented two-publish
    // behavior for a file that's both scanned and opened - so this
    // asserts by content/order rather than a fixed index.
    let disable_at = publishes
        .iter()
        .position(|m| m["params"]["diagnostics"] == json!([]))
        .unwrap_or_else(|| panic!("expected an empty (disable) publish, got: {publishes:?}"));
    assert!(
        publishes[..disable_at]
            .iter()
            .all(|m| m["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "function.deprecated")),
        "expected only real-diagnostic publishes before the disable, got: {publishes:?}"
    );
    assert!(
        publishes.last().unwrap()["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "function.deprecated"),
        "enable:true must restore the real diagnostic without a fresh didOpen, got: {publishes:?}"
    );

    let hover = response_result(&messages, 3);
    assert!(
        hover["contents"].is_string(),
        "hover/definition must keep working while disabled - enable:false means \
         'stop showing squiggles', not 'stop the language server', got: {hover:?}"
    );
}

#[test]
fn did_change_configuration_pulls_live_settings_when_the_client_advertises_support() {
    let dir = TempDir::new("dcc_pull");
    dir.write("other.toml", "check_deprecated_usage = true\n");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run_with_capabilities(
        &dir,
        json!({"workspace": {"configuration": true}}),
        &[
            did_open(&uri, DEPRECATED_FIXTURE),
            did_change_configuration(json!({})),
            // The first outbound request in a fresh session always gets
            // id 0 (lsp_server::ReqQueue::Outgoing::next_id starts at 0),
            // and this pull is the only outbound request this test's
            // session ever sends - so this response is guaranteed to
            // match the request the didChangeConfiguration notification
            // above triggers, without needing to inspect it first.
            response(0, json!([{"enable": true, "configPath": "other.toml"}])),
        ],
    );

    let pull_request = messages
        .iter()
        .find(|m| m["method"] == "workspace/configuration")
        .unwrap_or_else(|| panic!("expected a workspace/configuration request, got: {messages:?}"));
    assert_eq!(
        pull_request["params"]["items"],
        json!([{"section": "phpstan-rs"}])
    );

    let publishes = publishes_for(&messages, &uri);
    assert!(
        publishes.last().unwrap()["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "function.deprecated"),
        "the pulled configPath must be applied once the response arrives, got: {publishes:?}"
    );
}

#[test]
fn did_change_configuration_with_no_pull_support_falls_back_to_a_fresh_disk_resolve() {
    // No client capability and no pushed value: reload_configuration must
    // still re-run against the currently-configured root/config_path
    // rather than silently doing nothing - exercised here as a no-op
    // (nothing on disk actually changed) since this in-process test
    // harness feeds every input message up front and so can't model a
    // real mid-session file edit; `reload_configuration_rereads_an_
    // on_disk_config_change` below covers the "something really did
    // change" half directly.
    let dir = TempDir::new("dcc_fallback_no_op");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[
            did_open(&uri, DEPRECATED_FIXTURE),
            did_change_configuration(json!({})),
        ],
    );

    assert!(
        !messages
            .iter()
            .any(|m| m["method"] == "workspace/configuration"),
        "a client that never advertised pull support must not be sent a pull request, got: {messages:?}"
    );
    assert_eq!(
        publishes_for(&messages, &uri).len(),
        1,
        "nothing changed on disk, so the fallback resolve must still no-op, got: {messages:?}"
    );
}

/// `reload_configuration`'s own resolve+republish, called directly on a
/// `Server` rather than through the JSON-RPC wire: the wire-level `run`
/// harness feeds every input message up front, so it can't model a real
/// file edit landing *between* two points within one session the way this
/// test needs to (see also `did_change_configuration_with_no_pull_
/// support_falls_back_to_a_fresh_disk_resolve`, which covers the same
/// fallback path's no-op half through the real notification).
#[test]
fn reload_configuration_rereads_an_on_disk_config_change() {
    let dir = TempDir::new("dcc_on_disk_change");
    dir.write("phpstan-rs.toml", "check_deprecated_usage = false\n");
    let path = dir.write("a.php", DEPRECATED_FIXTURE);
    let uri = path_to_uri(&path).to_string();

    let mut server = Server::new();
    server.add_workspace_folders(std::slice::from_ref(&dir.0));
    let initial: Vec<Value> = server
        .publish_workspace_diagnostics()
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .collect();
    assert!(
        !initial.iter().any(|m| m["params"]["uri"] == uri
            && m["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "function.deprecated")),
        "check_deprecated_usage starts off, got: {initial:?}"
    );

    dir.write("phpstan-rs.toml", "check_deprecated_usage = true\n");
    let reload: Vec<Value> = server
        .reload_configuration(settings::PhpstanRsSettings {
            enable: true,
            config_path: None,
        })
        .iter()
        .map(|m| serde_json::to_value(m).unwrap())
        .collect();

    let diags = reload
        .iter()
        .find(|m| m["params"]["uri"] == uri)
        .unwrap_or_else(|| panic!("expected a publish for {uri}, got: {reload:?}"));
    assert!(
        diags["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "function.deprecated"),
        "the on-disk phpstan-rs.toml edit must be picked up, got: {diags:?}"
    );
}

#[test]
fn resolved_config_equality_is_not_fooled_by_sql_reflection_password_redaction() {
    use sql::config::{SqlDriver, SqlReflectionConfig};

    let base = SqlReflectionConfig {
        driver: SqlDriver::Pgsql,
        host: None,
        port: None,
        user: None,
        password: None,
        password_env: None,
        database: None,
        path: None,
        schema_file: None,
        session_init_sql: None,
    };
    let with_password_a = SqlReflectionConfig {
        password: Some("a".to_string()),
        ..base.clone()
    };
    let with_password_b = SqlReflectionConfig {
        password: Some("b".to_string()),
        ..base
    };

    assert_ne!(
        with_password_a, with_password_b,
        "two configs differing only in a redacted-in-Debug field must still compare unequal"
    );
    assert_eq!(
        format!("{with_password_a:?}"),
        format!("{with_password_b:?}"),
        "this pins down *why* the comparison above must use PartialEq, not a Debug-formatted \
         fingerprint: both configs' Debug output is identical"
    );
}
