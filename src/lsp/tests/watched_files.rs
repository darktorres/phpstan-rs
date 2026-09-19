//! `workspace/didChangeWatchedFiles`: dynamic-registration gating (LSP
//! has no static `ServerCapabilities` field for file watching), routing
//! Created/Changed/Deleted events through the same `update_file`/
//! `remove_file` machinery `didOpen`/`didChange`/workspace-folder removal
//! already use, intra-batch coalescing, and resilience to malformed or
//! unresolvable events.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::json;

fn watch_capabilities() -> serde_json::Value {
    json!({"workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}})
}

const GREETER_SRC: &str =
    "<?php\nclass Greeter { public function greet(): string { return 'hi'; } }\n";
const USE_GREETER_SRC: &str = "<?php\nfunction useGreeter(Greeter $g): void { $g->greet(); }\n";

fn mentions_greeter(diags: &[serde_json::Value]) -> bool {
    diags.iter().any(|d| d["code"] == "class.notFound")
}

#[test]
fn registers_watched_files_when_client_supports_dynamic_registration() {
    let dir = TempDir::new("watch_register");
    let messages = run_with_capabilities(&dir, watch_capabilities(), &[]);

    let register = messages
        .iter()
        .find(|m| m["method"] == "client/registerCapability")
        .unwrap_or_else(|| {
            panic!("expected a client/registerCapability request, got: {messages:?}")
        });
    let registrations = register["params"]["registrations"].as_array().unwrap();
    assert_eq!(registrations.len(), 1, "got: {registrations:?}");
    assert_eq!(
        registrations[0]["method"],
        "workspace/didChangeWatchedFiles"
    );
    let watchers = registrations[0]["registerOptions"]["watchers"]
        .as_array()
        .unwrap();
    assert_eq!(watchers.len(), 1, "got: {watchers:?}");
    assert_eq!(watchers[0]["globPattern"], "**/*.php");
}

#[test]
fn skips_registration_when_client_lacks_dynamic_registration_capability() {
    let dir = TempDir::new("watch_no_register");
    let messages = run(&dir, &[]);

    assert!(
        !messages
            .iter()
            .any(|m| m["method"] == "client/registerCapability"),
        "a client that never advertised the capability must not be sent a registration request, got: {messages:?}"
    );
}

#[test]
fn deleted_event_clears_diagnostics_and_rechecks_dependents() {
    let dir = TempDir::new("watch_delete");
    let greeter_path = dir.write("Greeter.php", GREETER_SRC);
    let greeter_uri = path_to_uri(&greeter_path).to_string();
    let use_path = dir.write("Use.php", USE_GREETER_SRC);
    let use_uri = path_to_uri(&use_path).to_string();

    let messages = run_with_capabilities(
        &dir,
        watch_capabilities(),
        &[did_change_watched_files(vec![file_event(&greeter_uri, 3)])],
    );

    assert!(
        messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == greeter_uri
                && m["params"]["diagnostics"] == json!([])),
        "expected an empty publish clearing Greeter.php's own stale diagnostics, got: {messages:?}"
    );
    let last_use = messages
        .iter()
        .rev()
        .find(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == use_uri)
        .unwrap_or_else(|| panic!("expected a publish for {use_uri}, got: {messages:?}"));
    assert!(
        mentions_greeter(last_use["params"]["diagnostics"].as_array().unwrap()),
        "expected Use.php to be rechecked with Greeter now undefined, got: {last_use:?}"
    );
}

#[test]
fn created_event_loads_a_never_scanned_file_and_resolves_pending_references() {
    let dir = TempDir::new("watch_create");
    let use_path = dir.write("Use.php", USE_GREETER_SRC);
    let use_uri = path_to_uri(&use_path).to_string();

    // Written out-of-band, after the initial workspace scan (which only
    // sees Use.php) has already run.
    let greeter_path = dir.0.join("Greeter.php");
    std::fs::write(&greeter_path, GREETER_SRC).unwrap();
    let greeter_uri = path_to_uri(&greeter_path).to_string();

    let messages = run_with_capabilities(
        &dir,
        watch_capabilities(),
        &[did_change_watched_files(vec![file_event(&greeter_uri, 1)])],
    );

    assert!(
        messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == greeter_uri),
        "expected a fresh publish for the newly created Greeter.php, got: {messages:?}"
    );
    let last_use = messages
        .iter()
        .rev()
        .find(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == use_uri)
        .unwrap_or_else(|| panic!("expected a publish for {use_uri}, got: {messages:?}"));
    assert!(
        !mentions_greeter(last_use["params"]["diagnostics"].as_array().unwrap()),
        "Use.php must now resolve the newly created Greeter, got: {last_use:?}"
    );
}

#[test]
fn changed_event_reroutes_through_update_file_like_didchange() {
    let dir = TempDir::new("watch_change");
    let greeter_path = dir.write("Greeter.php", "<?php\nclass Greeter {}\n");
    let greeter_uri = path_to_uri(&greeter_path).to_string();
    let use_path = dir.write("Use.php", USE_GREETER_SRC);
    let use_uri = path_to_uri(&use_path).to_string();

    // Simulates an edit landing after the workspace scan already read the
    // old (greet()-less) content but before the Changed event below is
    // processed - update_file must re-read the file's CURRENT on-disk
    // content, not whatever the scan cached.
    std::fs::write(&greeter_path, GREETER_SRC).unwrap();

    let messages = run_with_capabilities(
        &dir,
        watch_capabilities(),
        &[did_change_watched_files(vec![file_event(&greeter_uri, 2)])],
    );

    let last_use = messages
        .iter()
        .rev()
        .find(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == use_uri)
        .unwrap_or_else(|| panic!("expected a publish for {use_uri}, got: {messages:?}"));
    assert!(
        !mentions_greeter(last_use["params"]["diagnostics"].as_array().unwrap()),
        "the Changed event must recheck Use.php against Greeter's current (with-greet) shape, got: {last_use:?}"
    );
}

#[test]
fn same_batch_coalesces_to_the_last_event_per_path() {
    let dir = TempDir::new("watch_coalesce");
    let greeter_path = dir.write("Greeter.php", GREETER_SRC);
    let greeter_uri = path_to_uri(&greeter_path).to_string();
    std::fs::remove_file(&greeter_path).unwrap();

    let messages = run_with_capabilities(
        &dir,
        watch_capabilities(),
        &[did_change_watched_files(vec![
            file_event(&greeter_uri, 2), // Changed
            file_event(&greeter_uri, 3), // Deleted - the last event, must win
        ])],
    );

    assert!(
        messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == greeter_uri
                && m["params"]["diagnostics"] == json!([])),
        "expected the batch's last (Deleted) event to be the one applied, got: {messages:?}"
    );
}

#[test]
fn unresolvable_or_malformed_events_dont_panic_the_session() {
    let dir = TempDir::new("watch_malformed");
    let greeter_path = dir.write("Greeter.php", GREETER_SRC);
    let greeter_uri = path_to_uri(&greeter_path).to_string();

    let messages = run_with_capabilities(
        &dir,
        watch_capabilities(),
        &[
            did_change_watched_files(vec![
                file_event("untitled:Untitled-1", 2),
                file_event(&greeter_uri, 3),
            ]),
            notification(
                "workspace/didChangeWatchedFiles",
                json!({"not": "the right shape"}),
            ),
        ],
    );

    assert!(
        messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == greeter_uri
                && m["params"]["diagnostics"] == json!([])),
        "the valid Deleted event must still process despite an unresolvable sibling event \
         and a separately malformed notification, got: {messages:?}"
    );
}

#[test]
fn registercapability_error_response_is_logged_not_fatal() {
    let dir = TempDir::new("watch_register_error");
    let src = "<?php\nfunction f(): int { return 1; }\n";
    let path = dir.write("a.php", src);
    let uri = path_to_uri(&path).to_string();
    let pos = lsp_pos(src, "f(): int");

    // The client/registerCapability request client/registerCapability
    // fires from "initialized" is this session's first (and only)
    // outbound request, so its id is deterministically 0 (see
    // configuration.rs's did_change_configuration_pulls_... test for the
    // same reasoning about lsp_server::ReqQueue::Outgoing::next_id).
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": 0,
        "error": {"code": -32601, "message": "not supported"}
    });

    let messages = run_with_capabilities(
        &dir,
        watch_capabilities(),
        &[error_response, hover_request(3, &uri, pos)],
    );

    let hover = response_result(&messages, 3);
    assert!(
        hover["contents"].is_string(),
        "a rejected registration must not wedge the session or any other pending-request \
         handling, got: {hover:?}"
    );
}
