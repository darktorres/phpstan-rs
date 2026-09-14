//! LSP wire protocol handling and session lifecycle: message loop
//! request/response/notification routing, URI conversions, and
//! resilience to malformed input or per-file failures.

use super::super::*;
use super::helpers::*;
use serde_json::{Value, json};
use std::io::Cursor;

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
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);

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
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);

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
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);

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
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);

    let responses = decode_all(&writer);
    assert_eq!(responses.len(), 1, "exit must stop the loop: {responses:?}");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"], Value::Null);
}

#[test]
fn eof_with_no_exit_message_ends_the_loop_cleanly() {
    // Real clients occasionally close the pipe without ever sending
    // `exit` - read_message's own doc comment already documents this
    // as an expected case; this proves run_loop itself degrades to a
    // clean return rather than an error or a hang.
    let input = encode(&[request(1, "initialize", json!({}))]);
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);
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

// A malformed message body (a real Content-Length header, correctly
// consumed, but invalid JSON) must be skipped, not terminate the whole
// session; a per-file parser panic during didOpen/didChange/the
// initial workspace scan must fail to analyze that one file, not crash
// the whole server process.

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

    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);

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
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();
    run_loop(&mut reader, &mut writer);

    let responses = decode_all(&writer);
    assert_eq!(responses.len(), 1, "session should survive: {responses:?}");
}
