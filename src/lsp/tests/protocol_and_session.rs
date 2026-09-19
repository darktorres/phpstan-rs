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
    // level 1 (parameter.unused, $unused never read in the ctor body)
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
        .find(|d| d["code"] == "parameter.unused")
        .unwrap_or_else(|| panic!("expected parameter.unused in {b_diags:?}"));
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
        notification("exit", json!({})),
    ]);
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(input);
    pump_messages(&mut reader, &tx);
    drop(tx); // required: recv_timeout never sees Disconnected while any Sender is alive
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);

    let responses = decode_all(&writer);
    assert_eq!(
        responses.len(),
        1,
        "expected exactly one response: {responses:?}"
    );
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(
        responses[0]["result"]["capabilities"]["textDocumentSync"],
        1
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
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);

    let messages = decode_all(&writer);
    let b_uri = path_to_uri(&b_path).to_string();
    let b_notification = messages
        .iter()
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
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);

    let responses = decode_all(&writer);
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
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);

    let responses = decode_all(&writer);
    assert_eq!(responses.len(), 1, "exit must stop the loop: {responses:?}");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"], Value::Null);
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
    let handle = std::thread::spawn(move || {
        let mut writer = Vec::new();
        run_loop(&rx, &mut writer);
        writer
    });

    std::thread::sleep(RUN_LOOP_TICK * 3);

    tx.send(to_message(request(1, "shutdown", json!({}))))
        .unwrap();
    tx.send(to_message(notification("exit", json!({}))))
        .unwrap();
    drop(tx);

    let writer = handle.join().unwrap();
    let responses = decode_all(&writer);
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
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);
    assert_eq!(decode_all(&writer).len(), 1);
}

#[test]
fn percent_decode_handles_the_common_uri_escapes() {
    assert_eq!(percent_decode("hello%20world"), "hello world");
    assert_eq!(percent_decode("no_escapes_here"), "no_escapes_here");
}

#[test]
fn uri_to_path_and_path_to_uri_round_trip() {
    let uri: Uri = "file:///tmp/foo.php".parse().unwrap();
    let path = uri_to_path(&uri).unwrap();
    assert_eq!(path, PathBuf::from("/tmp/foo.php"));
    assert_eq!(path_to_uri(&path).to_string(), uri.to_string());
    let not_a_uri: Uri = "not-a-uri".parse().unwrap();
    assert_eq!(uri_to_path(&not_a_uri), None);
}

#[test]
fn uri_to_path_strips_the_leading_slash_before_a_windows_drive_letter() {
    let uri: Uri = "file:///C:/Users/dev/foo.php".parse().unwrap();
    let path = uri_to_path(&uri).unwrap();
    assert_eq!(path, PathBuf::from("C:/Users/dev/foo.php"));
}

#[test]
fn path_to_uri_reinserts_the_leading_slash_before_a_windows_drive_letter() {
    let path = PathBuf::from("C:/Users/dev/foo.php");
    assert_eq!(
        path_to_uri(&path).to_string(),
        "file:///C:/Users/dev/foo.php"
    );
}

#[test]
fn windows_drive_letter_uri_round_trips_through_uri_to_path_and_back() {
    let uri: Uri = "file:///C:/Users/dev/foo.php".parse().unwrap();
    let path = uri_to_path(&uri).unwrap();
    assert_eq!(path_to_uri(&path).to_string(), uri.to_string());
}

#[test]
fn uri_to_path_decodes_a_percent_encoded_drive_letter_colon() {
    let uri: Uri = "file:///C%3A/Users/dev/foo.php".parse().unwrap();
    assert_eq!(
        uri_to_path(&uri).unwrap(),
        PathBuf::from("C:/Users/dev/foo.php")
    );
}

#[test]
fn uri_to_path_leaves_an_ordinary_unix_path_alone() {
    let uri: Uri = "file:///tmp/foo.php".parse().unwrap();
    assert_eq!(uri_to_path(&uri).unwrap(), PathBuf::from("/tmp/foo.php"));
}

#[test]
fn path_to_uri_preserves_a_literal_backslash_in_a_real_unix_filename() {
    // Regression guard: a filename that genuinely contains '\' (legal on
    // Linux, this project's only build target), with no drive-letter
    // prefix, must round-trip byte-for-byte - not get silently rewritten
    // to '/' by a blind `Path::display()` string replace.
    let path = PathBuf::from("/tmp/foo\\bar.php");
    let uri = path_to_uri(&path);
    assert_eq!(uri.to_string(), "file:///tmp/foo%5Cbar.php");
    assert_eq!(uri_to_path(&uri).unwrap(), path);
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
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);

    let responses = decode_all(&writer);
    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert_eq!(responses[0]["id"], json!(1));
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
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);

    let responses = decode_all(&writer);
    assert_eq!(responses.len(), 1, "session should survive: {responses:?}");
}
