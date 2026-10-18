//! LSP wire protocol handling and session lifecycle: message loop
//! request/response/notification routing, URI conversions, and
//! resilience to malformed input or per-file failures.

use super::super::*;
use super::helpers::run;
use super::helpers::*;
use serde_json::{Value, json};
use std::io::Cursor;

#[test]
fn diagnostic_severity_reflects_diagnostic_level() {
    let dir = TempDir::new("severity_by_level");
    // level 0 (class.notFound) + level 6 (missingType.parameter, $x untyped)
    let level0_src =
        "<?php function f($x) { if ($x instanceof NotAClass) { return true; } return false; }";
    // level 1 (constructor.unusedParameter, $unused never read in the ctor body)
    let level1_src =
        "<?php class Greeter { public function __construct(int $unused) { echo \"hi\"; } }";
    // level 5 (argument.type, string literal passed where int is declared)
    let level5_src = "<?php function needsInt(int $n): void {} needsInt(\"nope\");";
    let a = dir.write("a.php", level0_src);
    let b = dir.write("b.php", level1_src);
    let c = dir.write("c.php", level5_src);
    let (a_uri, b_uri, c_uri) = (
        path_to_uri(&a).to_string(),
        path_to_uri(&b).to_string(),
        path_to_uri(&c).to_string(),
    );

    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": a_uri, "languageId": "php", "version": 1, "text": level0_src}}),
            ),
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": b_uri, "languageId": "php", "version": 1, "text": level1_src}}),
            ),
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": c_uri, "languageId": "php", "version": 1, "text": level5_src}}),
            ),
        ],
    );

    let diags_for = |uri: &str| -> Vec<Value> {
        messages
            .iter()
            .filter(|m| {
                m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri
            })
            .flat_map(|m| m["params"]["diagnostics"].as_array().unwrap().clone())
            .collect()
    };

    let a_diags = diags_for(&a_uri);
    let class_not_found = a_diags
        .iter()
        .find(|d| d["code"] == "class.notFound")
        .unwrap_or_else(|| panic!("expected class.notFound in {a_diags:?}"));
    assert_eq!(class_not_found["severity"], 1, "level 0 must map to ERROR");

    let missing_type = a_diags
        .iter()
        .find(|d| d["code"] == "missingType.parameter")
        .unwrap_or_else(|| panic!("expected missingType.parameter in {a_diags:?}"));
    assert_eq!(missing_type["severity"], 4, "level 6 must map to HINT");

    let b_diags = diags_for(&b_uri);
    let unused_param = b_diags
        .iter()
        .find(|d| d["code"] == "constructor.unusedParameter")
        .unwrap_or_else(|| panic!("expected constructor.unusedParameter in {b_diags:?}"));
    assert_eq!(unused_param["severity"], 2, "level 1 must map to WARNING");

    let c_diags = diags_for(&c_uri);
    let arg_type = c_diags
        .iter()
        .find(|d| d["code"] == "argument.type")
        .unwrap_or_else(|| panic!("expected argument.type in {c_diags:?}"));
    assert_eq!(
        arg_type["severity"], 2,
        "level 5 must map to WARNING (upper boundary)"
    );
}

#[test]
fn initialize_loads_the_workspace_and_returns_server_capabilities() {
    let dir = TempDir::new("init");
    dir.write("a.php", "<?php function helperA(): int { return 1; }");

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(
        responses.len(),
        1,
        "expected exactly one response: {responses:?}"
    );
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(
        responses[0]["result"]["capabilities"]["textDocumentSync"],
        json!({"openClose": true, "change": 1, "save": {"includeText": false}})
    );
}

#[test]
fn initialize_tolerates_unrecognized_initialization_options() {
    // Forward-compatibility guard for the VS Code extension's settings:
    // configPath/enable are sent via initializationOptions, but config
    // resolution actually runs from the workspace root's own
    // phpstan-rs.toml (`resolve_workspace` in lsp.rs), not from these -
    // InitializeParams.initialization_options is already Option<Value>
    // in lsp-types and is simply dropped, so this must keep working
    // rather than start erroring if a future field ever gets read here.
    let dir = TempDir::new("init_options");
    dir.write("a.php", "<?php function helperA(): int { return 1; }");

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({
                "rootUri": dir.uri(),
                "capabilities": {},
                "initializationOptions": {"configPath": "/does/not/exist.toml", "enable": true},
            }),
        ),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx);
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(
        responses.len(),
        1,
        "expected exactly one response: {responses:?}"
    );
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(
        responses[0]["result"]["capabilities"]["textDocumentSync"],
        json!({"openClose": true, "change": 1, "save": {"includeText": false}})
    );
}

fn initialize_with_capabilities(capabilities: Value) -> Vec<Value> {
    let dir = TempDir::new("init_position_encoding");
    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": capabilities}),
        ),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );
    decode_all(&writer.lock().unwrap())
}

#[test]
fn initialize_echoes_utf8_position_encoding_when_the_client_offers_it() {
    let responses =
        initialize_with_capabilities(json!({"general": {"positionEncodings": ["utf-8"]}}));
    assert_eq!(
        responses[0]["result"]["capabilities"]["positionEncoding"],
        "utf-8"
    );
}

#[test]
fn initialize_omits_position_encoding_when_the_client_does_not_offer_utf8() {
    let no_general = initialize_with_capabilities(json!({}));
    assert!(
        no_general[0]["result"]["capabilities"]
            .get("positionEncoding")
            .is_none()
    );

    let utf16_only =
        initialize_with_capabilities(json!({"general": {"positionEncodings": ["utf-16"]}}));
    assert!(
        utf16_only[0]["result"]["capabilities"]
            .get("positionEncoding")
            .is_none()
    );
}

#[test]
fn initialize_offers_utf8_position_encoding_regardless_of_its_position_in_the_clients_list() {
    let responses = initialize_with_capabilities(
        json!({"general": {"positionEncodings": ["utf-16", "utf-8"]}}),
    );
    assert_eq!(
        responses[0]["result"]["capabilities"]["positionEncoding"],
        "utf-8"
    );
}

#[test]
fn initialize_response_does_not_depend_on_initialized_ever_arriving() {
    // Deliberately no "initialized" notification: proves the scan
    // genuinely moved out of "initialize" - this would fail against the
    // old code, where b.php would already be indexed by the time exit
    // is processed.
    let dir = TempDir::new("no_initialized");
    dir.write(
        "a.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    dir.write(
        "b.php",
        "<?php function useGreeter(Greeter $g) { return $g->greet(); }",
    );

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": format!("{}/a.php", dir.uri()), "languageId": "php", "version": 1, "text": "<?php class Greeter {}"}}),
        ),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    assert!(
        messages
            .iter()
            .any(|m| m["id"] == json!(1) && m["result"]["capabilities"].is_object()),
        "the initialize response must not be blocked on initialized ever arriving: {messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"].as_str().unwrap_or("").ends_with("b.php")),
        "b.php was never scanned (no initialized notification was sent), so it must not appear: {messages:?}"
    );
}

#[test]
fn initialized_reports_progress_when_the_client_supplies_a_token() {
    let dir = TempDir::new("progress_with_token");
    dir.write("a.php", "<?php function helperA(): int { return 1; }");

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}, "workDoneToken": "scan-1"}),
        ),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    let progress: Vec<&Value> = messages
        .iter()
        .filter(|m| m["method"] == "$/progress")
        .collect();
    assert_eq!(progress.len(), 2, "got: {messages:?}");
    assert_eq!(progress[0]["params"]["token"], "scan-1");
    assert_eq!(progress[0]["params"]["value"]["kind"], "begin");
    assert_eq!(progress[1]["params"]["token"], "scan-1");
    assert_eq!(progress[1]["params"]["value"]["kind"], "end");
}

#[test]
fn no_progress_token_means_no_progress_notifications() {
    let dir = TempDir::new("progress_without_token");
    dir.write("a.php", "<?php function helperA(): int { return 1; }");

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    assert!(!messages.iter().any(|m| m["method"] == "$/progress"));
}

#[test]
fn initialized_is_a_noop_with_no_workspace_root() {
    let input = encode(&[
        request(1, "initialize", json!({"capabilities": {}})),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert_eq!(responses[0]["id"], 1);
}

#[test]
fn a_second_initialized_notification_is_a_noop() {
    let dir = TempDir::new("double_initialized");
    dir.write("a.php", "<?php function helperA(): int { return 1; }");

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert!(responses[0]["result"]["capabilities"].is_object());
}

#[test]
fn initialize_logs_a_workspace_file_that_fails_to_parse() {
    let dir = TempDir::new("init_bad_file");
    dir.write("ok.php", "<?php function helperOk(): int { return 1; }");
    // Momentarily-invalid syntax, the normal state while a file is being
    // edited elsewhere in the workspace - written to disk so the
    // workspace scan itself picks it up, not sent via didOpen.
    dir.write("bad.php", "<?php function foo(int $x");

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    let response = messages
        .iter()
        .find(|m| m["id"] == json!(1))
        .unwrap_or_else(|| panic!("expected the initialize response, got: {messages:?}"));
    assert!(
        response["result"]["capabilities"].is_object(),
        "the response itself must not be displaced or dropped: {response:?}"
    );
    let log_notification = messages
        .iter()
        .find(|m| {
            m["method"] == "window/logMessage"
                && m["params"]["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("bad.php")
        })
        .unwrap_or_else(|| {
            panic!("expected a window/logMessage notification for bad.php, got: {messages:?}")
        });
    assert_eq!(
        log_notification["params"]["type"], 2,
        "expected WARNING severity"
    );
}

#[test]
fn did_open_with_a_workspace_loaded_publishes_diagnostics_for_dependents_too() {
    // A real cross-file scenario: a.php declares Greeter, b.php calls
    // a method on it. Opening a.php with a version that drops that
    // method must publish a fresh diagnostic for b.php, even though
    // b.php itself was never opened - only ever seen via the initial
    // workspace scan.
    let dir = TempDir::new("cross_file");
    dir.write(
        "a.php",
        "<?php class Greeter { public function greet(): string { return 'hi'; } }",
    );
    let b_path = dir.write(
        "b.php",
        "<?php function useGreeter(Greeter $g) { return $g->greet(); }",
    );
    let a_uri = format!("{}/a.php", dir.uri());

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": a_uri, "languageId": "php", "version": 1, "text": "<?php class Greeter {}"}}),
        ),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    let b_uri = path_to_uri(&b_path).to_string();
    // The LAST one, not the first: "initialized" now also publishes
    // b.php's own pre-existing diagnostics (missingType.return) via the
    // whole-workspace push, before the didOpen-triggered recheck below
    // adds the now-missing-greet() one - a real client always takes the
    // latest notification per URI, same as this test should.
    let b_notification = messages
        .iter()
        .rev()
        .find(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == b_uri);
    let Some(b_notification) = b_notification else {
        panic!("expected a publishDiagnostics notification for b.php, got: {messages:?}");
    };
    // Found by content, not indexed as `diagnostics[0]` -
    // `missingType.return` (`useGreeter` itself has no `: T` return
    // type) is a second, real diagnostic on this exact fixture, sorted
    // ahead of the undefined-method one below since it anchors to the
    // whole function's own span, earlier in the file. What this test
    // actually cares about - that dropping `Greeter::greet()` publishes
    // a fresh, correctly-ranged diagnostic for the dependent file - is
    // unaffected by that second diagnostic existing alongside it.
    let greet_diagnostic = b_notification["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["message"].as_str().unwrap().contains("greet"));
    let Some(greet_diagnostic) = greet_diagnostic else {
        panic!(
            "expected a diagnostic mentioning the now-missing greet() method, got: {b_notification:?}"
        );
    };
    // A real range pointing at `$g->greet()` itself (0-indexed LSP
    // columns for a 1-indexed byte range starting at real column 48).
    assert_eq!(
        greet_diagnostic["range"],
        json!({"start": {"line": 0, "character": 47}, "end": {"line": 0, "character": 58}})
    );
    // b.php was never opened, only ever seen via the workspace scan and
    // as a re-checked dependent - neither carries a document version, so
    // the wire field is omitted entirely (`skip_serializing_if`), which
    // `serde_json::Value` surfaces as `Value::Null` for the missing key.
    assert_eq!(b_notification["params"]["version"], Value::Null);
}

#[test]
fn did_change_publishes_diagnostics_stamped_with_the_new_document_version() {
    let dir = TempDir::new("version_stamp_didchange");
    let path = dir.write(
        "a.php",
        "<?php class Foo { public function bar(): int { return 1; } }",
    );
    let uri = path_to_uri(&path).to_string();

    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": "<?php class Foo { public function bar(): int { return 1; } }"}}),
            ),
            notification(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": 7},
                    "contentChanges": [{"text": "<?php class Foo { public function bar(): int { return $this->missing(); } }"}],
                }),
            ),
        ],
    );

    // NOT .find(): a.php's own didOpen already produced an earlier,
    // version-1 publish for this same uri before the didChange below.
    let last = last_publish_diagnostics_for(&messages, &uri);
    assert_eq!(last["params"]["version"], json!(7), "got: {last:?}");
}

#[test]
fn a_dependent_files_diagnostics_keep_its_own_last_known_version_not_the_editors() {
    let dir = TempDir::new("version_stamp_dependent");
    let a_src = "<?php class Greeter { public function greet(): string { return 'hi'; } }";
    let a_path = dir.write("a.php", a_src);
    let a_uri = path_to_uri(&a_path).to_string();
    let b_src = "<?php function useGreeter(Greeter $g): void { $g->greet(); }";
    let b_path = dir.write("b.php", b_src);
    let b_uri = path_to_uri(&b_path).to_string();

    let messages = run(
        &dir,
        &[
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": a_uri, "languageId": "php", "version": 5, "text": a_src}}),
            ),
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": b_uri, "languageId": "php", "version": 3, "text": b_src}}),
            ),
            notification(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": a_uri, "version": 9},
                    "contentChanges": [{"text": "<?php class Greeter {}"}],
                }),
            ),
        ],
    );

    let last_a = last_publish_diagnostics_for(&messages, &a_uri);
    assert_eq!(last_a["params"]["version"], json!(9), "got: {last_a:?}");
    // b.php is a dependent, not the file being edited - its own last
    // known (didOpen) version, not a.php's version 9, must be stamped on
    // the diagnostics this didChange fans out to it.
    let last_b = last_publish_diagnostics_for(&messages, &b_uri);
    assert_eq!(last_b["params"]["version"], json!(3), "got: {last_b:?}");
}

#[test]
fn an_unrecognized_method_gets_a_method_not_found_error_response() {
    let input = encode(&[
        request(1, "totally/madeUp", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32601);
}

#[test]
fn shutdown_returns_a_null_result_and_exit_ends_the_loop() {
    let input = encode(&[
        request(1, "shutdown", json!({})),
        notification("exit", json!({})),
        // Anything after `exit` must never be processed - the loop
        // has to break immediately on it, not keep reading.
        request(2, "totally/madeUp", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    let exit_code = run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );
    assert_eq!(exit_code, 0, "shutdown before exit must exit cleanly");

    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(responses.len(), 1, "exit must stop the loop: {responses:?}");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"], Value::Null);
}

#[test]
fn exit_without_a_prior_shutdown_exits_with_code_1() {
    let input = encode(&[notification("exit", json!({}))]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    let exit_code = run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );
    assert_eq!(
        exit_code, 1,
        "exit with no prior shutdown is a protocol violation"
    );
}

#[test]
fn eof_after_shutdown_without_exit_still_exits_cleanly() {
    // Locks in that a clean transport ending's exit code is NOT derived
    // from `shutdown_received` - only the `exit` notification's own arm
    // is judged by that contract.
    let input = encode(&[request(1, "shutdown", json!({}))]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    let exit_code = run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );
    assert_eq!(exit_code, 0);
}

#[test]
fn a_transport_read_failure_ends_the_loop_cleanly_too() {
    // Exercises `RecvTimeoutError::Disconnected` via a genuine I/O
    // failure (not a clean EOF) - mechanically distinct from the EOF
    // case above, and previously untested by either.
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = FailingReader;
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    let exit_code = run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );
    assert_eq!(
        exit_code, 0,
        "a genuine transport failure is not judged by the shutdown contract either"
    );
}

#[test]
fn run_loop_wakes_on_an_idle_timeout_instead_of_blocking_forever() {
    // Real-thread/real-time, unlike this file's other tests: proves
    // `run_loop` is actually driven by `recv_timeout` (not a plain
    // blocking `recv`) by letting several idle ticks elapse with zero
    // messages sent before sending anything at all - a plain blocking
    // `recv` would still pass this test by coincidence (it'd just sit
    // there until the first send), but a loop that busy-looped or
    // panicked on a bare timeout would fail it.
    let (tx, rx) = crossbeam_channel::unbounded();
    let writer = Arc::new(Mutex::new(Vec::new()));
    let worker_writer = Arc::clone(&writer);
    let handle = std::thread::spawn(move || {
        run_loop(
            &rx,
            worker_writer,
            env!("CARGO_PKG_VERSION"),
            crate::app::config_schema(),
        );
    });

    std::thread::sleep(RUN_LOOP_TICK * 3);

    tx.send(to_message(request(1, "shutdown", json!({}))))
        .unwrap();
    tx.send(to_message(notification("exit", json!({}))))
        .unwrap();
    drop(tx);

    handle.join().unwrap();
    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(
        responses.len(),
        1,
        "expected exactly one response: {responses:?}"
    );
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"], Value::Null);
}

#[test]
fn eof_with_no_exit_message_ends_the_loop_cleanly() {
    // Real clients occasionally close the pipe without ever sending
    // `exit` - read_message's own doc comment already documents this
    // as an expected case; this proves run_loop itself degrades to a
    // clean return rather than an error or a hang. This is also the one
    // test in this file that actually exercises the `drop(tx)` step
    // below: since no `exit` ever arrives, `pump_messages` returns via
    // `ReadOutcome::Eof`, not a forwarded `exit` - without dropping
    // `tx`, `recv_timeout` would never see `Disconnected` and this test
    // would hang instead of completing.
    let input = encode(&[request(1, "initialize", json!({}))]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    let exit_code = run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );
    assert_eq!(
        exit_code, 0,
        "EOF without exit is not judged by the shutdown contract"
    );
    assert_eq!(decode_all(&writer.lock().unwrap()).len(), 1);
}

#[test]
fn log_message_notification_has_the_expected_wire_shape() {
    let message = log_message_notification(MessageType::ERROR, "oh no".to_string());
    let value = serde_json::to_value(&message).unwrap();
    assert_eq!(value["method"], "window/logMessage");
    assert_eq!(value["params"]["type"], 1);
    assert_eq!(value["params"]["message"], "oh no");
}

/// The path the URI `text` names, `None` for one that names none.
fn uri_path(text: &str) -> Option<PathBuf> {
    uri_to_path(&text.parse::<Uri>().unwrap())
}

#[test]
fn a_uri_of_another_scheme_or_with_a_user_a_port_or_a_nul_byte_names_no_path() {
    for text in [
        "not-a-uri",
        "http://example.com/a.php",
        "untitled:Untitled-1",
        "file://user@localhost/a.php",
        "file://localhost:80/a.php",
        "file:///a%00b.php",
    ] {
        assert_eq!(uri_path(text), None, "{text}");
    }
}

#[cfg(unix)]
#[test]
fn on_unix_a_path_and_its_uri_round_trip_by_their_bytes() {
    use std::os::unix::ffi::OsStrExt;
    for (path, text) in [
        (Path::new("/tmp/foo.php"), "file:///tmp/foo.php"),
        (
            Path::new("/tmp/hello world.php"),
            "file:///tmp/hello%20world.php",
        ),
        // `\` is a name's character here, not a separator.
        (Path::new("/tmp/foo\\bar.php"), "file:///tmp/foo%5Cbar.php"),
        (
            Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9.php")),
            "file:///tmp/caf%E9.php",
        ),
        (Path::new("/"), "file:///"),
    ] {
        let uri = path_to_uri(path);
        assert_eq!(uri.to_string(), text);
        assert_eq!(uri_to_path(&uri).as_deref(), Some(path), "{text}");
    }
}

#[cfg(unix)]
#[test]
fn on_unix_a_uri_names_a_path_of_this_machine_and_a_drive_letter_is_a_name() {
    for (text, expected) in [
        ("file://localhost/tmp/foo.php", Some("/tmp/foo.php")),
        ("file://LocalHost/tmp/foo.php", Some("/tmp/foo.php")),
        ("FILE:///tmp/foo.php", Some("/tmp/foo.php")),
        ("file:/tmp/foo.php", Some("/tmp/foo.php")),
        ("file:///c:/x/foo.php", Some("/c:/x/foo.php")),
        ("file:///C%3A/x", Some("/C:/x")),
        ("file://server/share/foo.php", None),
        ("file:foo.php", None),
    ] {
        assert_eq!(uri_path(text), expected.map(PathBuf::from), "{text}");
    }
}

#[cfg(windows)]
#[test]
fn on_windows_a_drives_path_and_its_uri_round_trip() {
    for (path, text) in [
        (r"C:\Users\dev\foo.php", "file:///C:/Users/dev/foo.php"),
        (
            r"C:\Users\dev\hello world.php",
            "file:///C:/Users/dev/hello%20world.php",
        ),
        (r"C:\", "file:///C:/"),
    ] {
        let uri = path_to_uri(Path::new(path));
        assert_eq!(uri.to_string(), text);
        assert_eq!(
            uri_to_path(&uri).as_deref(),
            Some(Path::new(path)),
            "{text}"
        );
    }
    // The letter is the drive's, in either case; a verbatim drive is the drive.
    assert_eq!(path_to_uri(Path::new(r"c:\a")).to_string(), "file:///C:/a");
    assert_eq!(
        path_to_uri(Path::new(r"\\?\C:\a\nul.php")).to_string(),
        "file:///C:/a/nul.php"
    );
}

#[cfg(windows)]
#[test]
fn on_windows_a_uri_names_a_drive_in_either_encoding_or_a_share_by_its_host() {
    for (text, expected) in [
        (
            "file:///c%3A/Users/dev/foo.php",
            Some(r"c:\Users\dev\foo.php"),
        ),
        ("file://localhost/C:/a", Some(r"C:\a")),
        ("file:///C:", Some(r"C:\")),
        ("file://server/share/a.php", Some(r"\\server\share\a.php")),
        ("file://wsl%24/Ubuntu/home", Some(r"\\wsl$\Ubuntu\home")),
        (
            "file://%3F/Volume%7Ba1%7D/a.php",
            Some(r"\\?\Volume{a1}\a.php"),
        ),
        ("file://./pipe/x", Some(r"\\.\pipe\x")),
        // No drive and no share, a drive's relative path, a name that is
        // not UTF-8, a host without a share.
        ("file:///foo.php", None),
        ("file:///C:foo.php", None),
        ("file:///C:/caf%E9.php", None),
        ("file://server/", None),
    ] {
        assert_eq!(uri_path(text), expected.map(PathBuf::from), "{text}");
    }
}

#[cfg(windows)]
#[test]
fn on_windows_a_shares_path_and_its_uri_round_trip_as_do_the_namespaces() {
    for (path, text) in [
        (r"\\server\share\a.php", "file://server/share/a.php"),
        (r"\\wsl$\Ubuntu\home", "file://wsl%24/Ubuntu/home"),
        (r"\\?\Volume{a1}\a.php", "file://%3F/Volume%7Ba1%7D/a.php"),
        (r"\\.\pipe\x", "file://./pipe/x"),
    ] {
        let uri = path_to_uri(Path::new(path));
        assert_eq!(uri.to_string(), text);
        assert_eq!(
            uri_to_path(&uri).as_deref(),
            Some(Path::new(path)),
            "{text}"
        );
    }
    assert_eq!(
        path_to_uri(Path::new(r"\\?\UNC\server\share\a.php")).to_string(),
        "file://server/share/a.php"
    );
}

// A malformed message body (a real Content-Length header, correctly
// consumed, but invalid JSON) must be skipped, not terminate the whole
// session; a per-file parser panic during didOpen/didChange/the
// initial workspace scan must fail to analyze that one file, not crash
// the whole server process. The skip itself now happens inside
// `pump_messages` (on the reader thread, in real usage) rather than
// inline in `run_loop` - this test's own assertion is unchanged and is
// the regression check for that move.

#[test]
fn a_malformed_message_body_is_skipped_without_terminating_the_session() {
    let malformed_body = b"{not valid json";
    let mut input = Vec::new();
    write!(input, "Content-Length: {}\r\n\r\n", malformed_body.len()).unwrap();
    input.extend_from_slice(malformed_body);
    input.extend_from_slice(&encode(&[
        request(1, "initialize", json!({})),
        notification("exit", json!({})),
    ]));

    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    // The malformed body is the very first thing in the stream, skipped
    // by `pump_messages` before it ever reads the `initialize` request
    // that follows - the resulting `window/logMessage` notification is
    // therefore written BEFORE the initialize response, not after, so
    // this looks messages up by content, never by position.
    let messages = decode_all(&writer.lock().unwrap());
    assert_eq!(messages.len(), 2, "got: {messages:?}");
    let response = messages
        .iter()
        .find(|m| m["id"] == json!(1))
        .unwrap_or_else(|| panic!("expected the initialize response, got: {messages:?}"));
    assert_eq!(
        response["result"]["capabilities"]["textDocumentSync"],
        json!({"openClose": true, "change": 1, "save": {"includeText": false}})
    );
    let log_notification = messages
        .iter()
        .find(|m| m["method"] == "window/logMessage")
        .unwrap_or_else(|| panic!("expected a window/logMessage notification, got: {messages:?}"));
    assert_eq!(
        log_notification["params"]["type"], 2,
        "expected WARNING severity"
    );
}

#[test]
fn a_file_that_crashes_the_parser_does_not_take_down_the_lsp_session() {
    let dir = TempDir::new("lsp-crash");
    dir.write("ok.php", "<?php function helperOk(): int { return 1; }");

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": format!("file://{}/bad.php", dir.0.display()),
                    "languageId": "php",
                    "version": 1,
                    // Momentarily-invalid syntax, the normal state
                    // while a file is being typed - an unclosed
                    // parameter list, a real ParseError this
                    // parser's Result-based error reporting now
                    // handles gracefully instead of panicking.
                    "text": "<?php function foo(int $x",
                }
            }),
        ),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    assert_eq!(
        messages.len(),
        2,
        "expected the initialize response plus one logMessage notification for bad.php, session should survive: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m["id"] == json!(1)),
        "session should survive and still answer initialize: {messages:?}"
    );
    let log_notification = messages
        .iter()
        .find(|m| {
            m["method"] == "window/logMessage"
                && m["params"]["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("bad.php")
        })
        .unwrap_or_else(|| {
            panic!("expected a window/logMessage notification for bad.php, got: {messages:?}")
        });
    assert_eq!(
        log_notification["params"]["type"], 2,
        "expected WARNING severity"
    );
}

#[test]
fn send_request_allocates_sequential_ids_and_the_right_method() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let Message::Request(req1) = server.send_request::<lsp_types::request::WorkDoneProgressCreate>(
        lsp_types::WorkDoneProgressCreateParams {
            token: lsp_types::NumberOrString::Number(1),
        },
    ) else {
        panic!("send_request must build a Message::Request");
    };
    let Message::Request(req2) = server.send_request::<lsp_types::request::RegisterCapability>(
        lsp_types::RegistrationParams {
            registrations: vec![],
        },
    ) else {
        panic!("send_request must build a Message::Request");
    };
    assert_eq!(req1.method, "window/workDoneProgress/create");
    assert_eq!(req2.method, "client/registerCapability");
    assert_ne!(req1.id, req2.id);
}

#[test]
fn handle_request_registers_and_clears_its_own_pending_entry_on_a_normal_response() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let mut writer = Vec::new();
    handle_request(
        &mut server,
        &mut writer,
        Request::new(RequestId::from(1), "shutdown".to_string(), json!({})),
    );

    assert!(!server.pending_incoming_requests.has_pending());
    let messages = decode_all(&writer);
    assert_eq!(messages[0]["result"], Value::Null);
}

#[test]
fn a_cancel_notification_sets_the_flag_and_answers_with_request_canceled() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let mut writer = Vec::new();
    let id = RequestId::from(1);
    let flag = Arc::new(AtomicBool::new(false));
    server
        .pending_incoming_requests
        .register(id.clone(), Arc::clone(&flag));

    handle_notification(
        &mut server,
        &mut writer,
        LspNotification::new("$/cancelRequest".to_string(), json!({"id": 1})),
    );

    assert!(flag.load(Ordering::Relaxed));
    assert!(!server.pending_incoming_requests.has_pending());
    let messages = decode_all(&writer);
    assert_eq!(
        messages[0]["error"]["code"],
        ErrorCode::RequestCanceled as i32
    );
}

#[test]
fn a_cancel_notification_matches_a_string_request_id_too() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let mut writer = Vec::new();
    let id = RequestId::from("abc".to_string());
    let flag = Arc::new(AtomicBool::new(false));
    server
        .pending_incoming_requests
        .register(id.clone(), Arc::clone(&flag));

    handle_notification(
        &mut server,
        &mut writer,
        LspNotification::new("$/cancelRequest".to_string(), json!({"id": "abc"})),
    );

    assert!(flag.load(Ordering::Relaxed));
    let messages = decode_all(&writer);
    assert_eq!(
        messages[0]["error"]["code"],
        ErrorCode::RequestCanceled as i32
    );
}

#[test]
fn a_cancel_notification_for_an_already_answered_id_writes_nothing() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let mut writer = Vec::new();
    let id = RequestId::from(1);
    server
        .pending_incoming_requests
        .register(id.clone(), Arc::new(AtomicBool::new(false)));
    // Simulates the id having already been answered normally, before
    // the cancel below arrives.
    server.pending_incoming_requests.complete(&id);

    handle_notification(
        &mut server,
        &mut writer,
        LspNotification::new("$/cancelRequest".to_string(), json!({"id": 1})),
    );

    assert!(decode_all(&writer).is_empty());
}

#[test]
fn respond_unless_canceled_drops_a_late_response_for_an_already_canceled_id() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let mut writer = Vec::new();
    let id = RequestId::from(1);
    server
        .pending_incoming_requests
        .register(id.clone(), Arc::new(AtomicBool::new(false)));
    // Simulates a "$/cancelRequest" having already completed this id
    // (and written its own RequestCanceled response) before the real
    // handler's late answer tries to write here.
    server.pending_incoming_requests.complete(&id);

    respond_unless_canceled(
        &mut server,
        &mut writer,
        &id,
        Message::Response(Response::new_ok(id.clone(), json!("late"))),
    );

    assert!(
        decode_all(&writer).is_empty(),
        "a response for an already-completed id must never be written"
    );
}

#[test]
fn a_cancel_request_for_an_unknown_id_does_not_disrupt_the_session() {
    let input = encode(&[
        notification("$/cancelRequest", json!({"id": 999})),
        request(1, "shutdown", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx);
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    assert_eq!(messages.len(), 1, "got: {messages:?}");
    assert_eq!(messages[0]["id"], json!(1));
    assert_eq!(messages[0]["result"], Value::Null);
}

#[test]
fn a_matching_response_clears_the_pending_entry() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let Message::Request(req) = server.send_request::<lsp_types::request::WorkDoneProgressCreate>(
        lsp_types::WorkDoneProgressCreateParams {
            token: lsp_types::NumberOrString::Number(1),
        },
    ) else {
        panic!("send_request must build a Message::Request");
    };
    assert!(server.pending_requests.outgoing.has_pending());

    let mut writer = Vec::new();
    handle_response(
        &mut server,
        &mut writer,
        Response::new_ok(req.id.clone(), serde_json::Value::Null),
    );

    assert!(!server.pending_requests.outgoing.has_pending());
}

#[test]
fn a_second_response_for_an_already_completed_id_is_ignored_without_panicking() {
    let mut server = Server::new(env!("CARGO_PKG_VERSION"), crate::app::config_schema());
    let Message::Request(req) = server.send_request::<lsp_types::request::WorkDoneProgressCreate>(
        lsp_types::WorkDoneProgressCreateParams {
            token: lsp_types::NumberOrString::Number(1),
        },
    ) else {
        panic!("send_request must build a Message::Request");
    };
    let mut writer = Vec::new();
    handle_response(
        &mut server,
        &mut writer,
        Response::new_ok(req.id.clone(), serde_json::Value::Null),
    );

    handle_response(
        &mut server,
        &mut writer,
        Response::new_ok(req.id.clone(), serde_json::Value::Null),
    );

    assert!(!server.pending_requests.outgoing.has_pending());
}

#[test]
fn a_bare_response_message_from_the_client_does_not_crash_the_session() {
    let input = encode(&[
        response(7, json!(null)),
        request(1, "initialize", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let responses = decode_all(&writer.lock().unwrap());
    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert_eq!(responses[0]["id"], json!(1));
}

#[test]
fn initialize_publishes_diagnostics_for_unopened_workspace_files_with_findings() {
    let dir = TempDir::new("publish_on_initialized");
    let root_src = "<?php function f() { echo $undefinedRoot; }";
    let root_path = dir.write("root.php", root_src);
    let nested_src = "<?php function g() { echo $undefinedNested; }";
    let nested_path = dir.write("nested/dir/deep.php", nested_src);
    let root_uri = path_to_uri(&root_path).to_string();
    let nested_uri = path_to_uri(&nested_path).to_string();

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    for uri in [&root_uri, &nested_uri] {
        let notification = messages.iter().find(|m| {
            m["method"] == "textDocument/publishDiagnostics" && &m["params"]["uri"] == uri
        });
        let Some(notification) = notification else {
            panic!("expected a publishDiagnostics notification for {uri}, got: {messages:?}");
        };
        let diags = notification["params"]["diagnostics"].as_array().unwrap();
        assert!(
            diags.iter().any(|d| d["code"] == "variable.undefined"),
            "got: {diags:?}"
        );
    }
}

#[test]
fn initialize_does_not_publish_diagnostics_for_clean_workspace_files() {
    let dir = TempDir::new("no_publish_when_clean");
    dir.write("a.php", "<?php function helperA(): int { return 1; }");

    let messages = run(&dir, &[]);

    assert!(
        !messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"),
        "got: {messages:?}"
    );
}

#[test]
fn initialize_does_not_publish_diagnostics_for_files_under_vendor_directory() {
    let dir = TempDir::new("no_publish_for_vendor");
    dir.write("app.php", "<?php function helperA(): int { return 1; }");
    let vendor_path = dir.write(
        "vendor/lib/bad.php",
        "<?php function f() { echo $undefinedInVendor; }",
    );
    let vendor_uri = path_to_uri(&vendor_path).to_string();

    let messages = run(&dir, &[]);

    assert!(
        !messages
            .iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == vendor_uri),
        "vendor/ files must never be diagnosed even when loaded as context: got: {messages:?}"
    );
}

#[test]
fn initialize_diagnostics_push_survives_one_workspace_file_failing_to_parse() {
    let dir = TempDir::new("publish_survives_a_parse_failure");
    dir.write("bad.php", "<?php function foo(int $x");
    let good_src = "<?php function f() { echo $undefinedInGood; }";
    let good_path = dir.write("good.php", good_src);
    let good_uri = path_to_uri(&good_path).to_string();

    let input = encode(&[
        request(
            1,
            "initialize",
            json!({"rootUri": dir.uri(), "capabilities": {}}),
        ),
        notification("initialized", json!({})),
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let writer = Arc::new(Mutex::new(Vec::new()));
    run_loop(
        &rx,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );

    let messages = decode_all(&writer.lock().unwrap());
    assert!(
        messages.iter().any(|m| m["id"] == json!(1)),
        "the initialize response must still arrive: got: {messages:?}"
    );
    let good_notification = messages.iter().find(|m| {
        m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == good_uri
    });
    let Some(good_notification) = good_notification else {
        panic!(
            "expected diagnostics for good.php despite bad.php failing to parse, got: {messages:?}"
        );
    };
    let diags = good_notification["params"]["diagnostics"]
        .as_array()
        .unwrap();
    assert!(
        diags.iter().any(|d| d["code"] == "variable.undefined"),
        "got: {diags:?}"
    );
}

#[test]
fn a_position_counts_the_unit_the_session_agreed_on() {
    use super::super::protocol::{offset_from_lsp, range_to_lsp, span_to_lsp_range};
    use syntax::span::{LineCol, LineIndex, Range, Span, Unit};
    // `é` is two bytes and one UTF-16 unit, `😀` four bytes and two units.
    let source = "<?php $é = '😀'; $x;";
    let index = LineIndex::new(source.as_bytes());
    let x = source.find("$x").unwrap() as u32;
    let span = Span::new(x, x + 2);
    let chars_before = source[..x as usize].chars().count() as u32;

    let utf16 = span_to_lsp_range(&index, Unit::Utf16, span);
    assert_eq!(utf16.start.character, chars_before + 1);
    let utf8 = span_to_lsp_range(&index, Unit::Utf8, span);
    assert_eq!(utf8.start.character, x);

    // A diagnostic's range counts characters; the session's unit recounts it.
    let range = Range {
        start: LineCol {
            line: 1,
            col: chars_before + 1,
        },
        end: LineCol {
            line: 1,
            col: chars_before + 3,
        },
    };
    assert_eq!(range_to_lsp(Some(&index), Unit::Utf16, range), utf16);
    assert_eq!(range_to_lsp(Some(&index), Unit::Utf8, range), utf8);
    // With no file loaded the characters stand.
    assert_eq!(
        range_to_lsp(None, Unit::Utf8, range).start.character,
        chars_before
    );

    // And a cursor position goes back to the same offset.
    assert_eq!(offset_from_lsp(&index, Unit::Utf16, utf16.start), Some(x));
    assert_eq!(offset_from_lsp(&index, Unit::Utf8, utf8.start), Some(x));
}
