//! A minimal Language Server Protocol server, run via `phpstan-rs --lsp`.
//! Communicates over stdio using LSP's standard JSON-RPC-over-`Content-
//! Length`-framed-messages transport, via the `lsp-server`/`lsp-types`
//! crates (protocol framing/message shapes and typed request/notification
//! params, respectively) rather than hand-rolling either against raw
//! `serde_json::Value` - this project already depends on `serde`/
//! `serde_json` for equally "boring, solved" infrastructure elsewhere
//! (composer.json parsing, this project's own config file), so hand-
//! rolling the LSP/JSON-RPC layer specifically stopped being the more
//! consistent choice once those were already dependencies.
//!
//! `checker::Diagnostic` carries a real `Span` (Phase 11), converted here
//! to LSP's 0-indexed line/character convention. The `location` symbolic
//! label (e.g. `"ClassName::methodName"`) is still folded into the
//! message text rather than dropped - LSP has no field for it. Known
//! remaining imprecision: LSP wants UTF-16 code-unit columns, and this
//! project's `Span` counts UTF-8 characters - an acceptable approximation
//! for now (both agree for any line without astral-plane characters, the
//! overwhelming majority of real PHP source), not a correctness bug worth
//! blocking on.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_server::{ErrorCode, Message, Notification as LspNotification, Request, Response};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializeResult, Position as LspPosition,
    PublishDiagnosticsParams, Range as LspRange, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};

use crate::checker::Diagnostic;
use crate::project::Project;

/// The outcome of trying to read one framed LSP message. `Eof` (a clean
/// transport disconnection) and `Disconnected` (a real I/O failure
/// reading the header/body - the stream's position afterward is
/// unknown) both mean "stop the loop". `MalformedBody` is the one
/// genuinely recoverable case: `lsp_server::Message::read` reads exactly
/// `Content-Length` bytes before ever attempting to parse them as JSON
/// (confirmed against its own source), so a body that fails to parse
/// still leaves the reader correctly positioned at the start of the
/// *next* message - only this one message needs to be skipped, not the
/// whole connection torn down. `lsp_server` reports both this and a
/// genuinely unrecoverable framing error (e.g. a missing Content-Length
/// header, where the next message boundary truly isn't known) as the
/// same `io::ErrorKind::InvalidData` - real PHP LSP clients essentially
/// never send the latter, so treating every `InvalidData` as
/// recoverable-and-skippable (rather than as `Disconnected`) matches
/// this crate's own real behavior without losing anything this project's
/// own tests actually exercise.
enum ReadOutcome {
    Message(Message),
    Eof,
    Disconnected(io::Error),
    MalformedBody(io::Error),
}

fn read_message<R: BufRead>(reader: &mut R) -> ReadOutcome {
    match Message::read(reader) {
        Ok(Some(msg)) => ReadOutcome::Message(msg),
        Ok(None) => ReadOutcome::Eof,
        Err(e) if e.kind() == io::ErrorKind::InvalidData => ReadOutcome::MalformedBody(e),
        Err(e) => ReadOutcome::Disconnected(e),
    }
}

fn write_message<W: Write>(writer: &mut W, message: &Message) -> io::Result<()> {
    message.write(writer)
}

/// `file://`-scheme only - this server has no use for any other URI
/// scheme a client might send. `Uri::path()` is still percent-encoded
/// (fluent_uri, `lsp_types::Uri`'s own backing type, never decodes it
/// implicitly); the small `percent_decode` helper below covers the
/// common escapes a real filesystem path needs.
fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    if uri.scheme()?.as_str() != "file" {
        return None;
    }
    Some(PathBuf::from(percent_decode(uri.path().as_str())))
}

fn path_to_uri(path: &Path) -> Uri {
    let encoded = percent_encode_path(&path.display().to_string());
    Uri::from_str(&format!("file://{encoded}"))
        .expect("a percent-encoded absolute filesystem path is always a valid file:// URI")
}

/// Decodes the small set of percent-escapes a real filesystem path
/// realistically contains (spaces and a few punctuation characters) -
/// not a general-purpose URI decoder, just enough for real filesystem
/// paths.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The inverse of `percent_decode` - escapes every byte outside a plain
/// URI path's safe set (keeping `/` itself literal), so a real filesystem
/// path containing a space or other reserved character still produces a
/// syntactically valid `file://` URI.
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// One diagnostic's LSP shape. `location` (a symbolic label like
/// `"ClassName::methodName"`, not a position) is folded into the message
/// text instead of dropped - `lsp_types::Diagnostic` has no field for
/// it. `Span`'s 1-indexed line/col become LSP's 0-indexed line/character.
fn to_lsp_diagnostic(d: &Diagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: LspRange {
            start: LspPosition {
                line: d.span.start.line.saturating_sub(1),
                character: d.span.start.col.saturating_sub(1),
            },
            end: LspPosition {
                line: d.span.end.line.saturating_sub(1),
                character: d.span.end.col.saturating_sub(1),
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("phpstan-rs".to_string()),
        message: format!("{}: {}", d.location, d.message),
        ..Default::default()
    }
}

fn publish_diagnostics_notification(uri: Uri, diagnostics: &[Diagnostic]) -> Message {
    Message::Notification(LspNotification::new(
        "textDocument/publishDiagnostics".to_string(),
        PublishDiagnosticsParams {
            uri,
            diagnostics: diagnostics.iter().map(to_lsp_diagnostic).collect(),
            version: None,
        },
    ))
}

struct Server {
    project: Project,
    // Every URI the client currently has open or has otherwise seen, so
    // `initialize`'s workspace scan and `didOpen`/`didChange` share one
    // path->diagnostics story instead of the workspace scan being
    // invisible to later incremental updates.
    open_uris: HashMap<PathBuf, Uri>,
}

impl Server {
    fn new() -> Self {
        Server {
            project: Project::new(),
            open_uris: HashMap::new(),
        }
    }

    /// Loads every `.php` file under `root` into the project up front, so
    /// the cross-file registry and dependency graph this whole phase is
    /// about are populated from the very first diagnostic, not built up
    /// one `didOpen` at a time.
    fn load_workspace(&mut self, root: &Path) {
        let mut files = Vec::new();
        crate::project::collect_php_files(root, &mut files);
        // See main.rs's run_check_project for why this matters: a real
        // workspace can genuinely declare the same fully-qualified class
        // twice (duplicate vendored copies of a library), and
        // unsorted readdir order would make which one wins the
        // registry slot filesystem-dependent.
        files.sort();
        for path in files {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let source = String::from_utf8_lossy(&bytes).into_owned();
            // Momentarily-invalid syntax is the normal state while a
            // file is being edited elsewhere in the workspace, not an
            // edge case - a single malformed file here must not prevent
            // the rest of the workspace from loading at all, matching
            // main.rs's own try_load_file for the equivalent check-
            // project path.
            let path_for_log = path.clone();
            if let Err(e) = self.project.load_file(path, &source) {
                eprintln!(
                    "phpstan-rs lsp: skipping {} - failed to parse: {e}",
                    path_for_log.display()
                );
            }
        }
        self.project.finish_loading();
    }

    fn handle_open_or_change(&mut self, uri: &Uri, text: &str) -> Vec<Message> {
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        self.open_uris.insert(path.clone(), uri.clone());
        // Same graceful-degradation as load_workspace, and more exposed
        // in practice here: momentarily-invalid syntax is the normal
        // state while a file is being typed, not an edge case. A parse
        // failure here used to unwind and take down the whole LSP server
        // process for the rest of the editing session, not just fail to
        // analyze this one file - update_file's own Result now makes
        // that a plain, recoverable Err instead.
        let results = match self.project.update_file(&path, text) {
            Ok(results) => results,
            Err(e) => {
                eprintln!(
                    "phpstan-rs lsp: skipping {} - failed to parse: {e}",
                    path.display()
                );
                HashMap::new()
            }
        };
        results
            .into_iter()
            .map(|(p, diags)| {
                // A dependent file re-checked here might not itself be
                // open in the editor - it still needs a fresh
                // publishDiagnostics, using whatever URI it was last seen
                // under (the workspace scan, if nothing else).
                let dep_uri = self
                    .open_uris
                    .get(&p)
                    .cloned()
                    .unwrap_or_else(|| path_to_uri(&p));
                publish_diagnostics_notification(dep_uri, &diags)
            })
            .collect()
    }
}

pub fn run() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    run_loop(&mut reader, &mut writer);
}

/// The actual message loop, generic over its I/O so tests can drive it
/// with an in-memory `Cursor`/`Vec<u8>` instead of a real process's
/// stdin/stdout - `run` is just this plus the real handles.
fn run_loop<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) {
    let mut server = Server::new();

    loop {
        let message = match read_message(reader) {
            ReadOutcome::Message(m) => m,
            ReadOutcome::Eof => break,
            ReadOutcome::Disconnected(e) => {
                eprintln!("phpstan-rs lsp: failed to read message: {e}");
                break;
            }
            // A malformed message body (invalid JSON, but a known
            // Content-Length correctly consumed) doesn't tear down the
            // rest of the session - a single non-conformant message from
            // a client (an editor-extension bug, a dropped byte over a
            // flaky transport, ...) shouldn't take the whole language
            // server down for the rest of the editing session.
            ReadOutcome::MalformedBody(e) => {
                eprintln!("phpstan-rs lsp: skipping malformed message: {e}");
                continue;
            }
        };

        match message {
            Message::Request(req) => handle_request(&mut server, writer, req),
            Message::Notification(not) => {
                if not.method == "exit" {
                    break;
                }
                handle_notification(&mut server, writer, not);
            }
            // This server never sends a request of its own, so it never
            // expects a response back from the client either.
            Message::Response(_) => {}
        }
    }
}

fn handle_request<W: Write>(server: &mut Server, writer: &mut W, req: Request) {
    match req.method.as_str() {
        "initialize" => {
            #[allow(deprecated)] // root_uri: no other way to reach it pre-workspace_folders
            let root = serde_json::from_value::<InitializeParams>(req.params)
                .ok()
                .and_then(|params| {
                    params
                        .root_uri
                        .or_else(|| params.workspace_folders?.into_iter().next().map(|f| f.uri))
                });
            if let Some(root) = root.as_ref().and_then(uri_to_path) {
                server.load_workspace(&root);
            }
            let result = InitializeResult {
                capabilities: ServerCapabilities {
                    // Full sync: didChange always carries the whole new
                    // document text, not incremental ranges - simplest
                    // correct choice, and update_file already takes a
                    // full source string anyway.
                    text_document_sync: Some(TextDocumentSyncCapability::Kind(
                        TextDocumentSyncKind::FULL,
                    )),
                    ..Default::default()
                },
                server_info: Some(ServerInfo {
                    name: "phpstan-rs".to_string(),
                    version: Some(env!("CARGO_PKG_VERSION").to_string()),
                }),
            };
            let _ = write_message(writer, &Message::Response(Response::new_ok(req.id, result)));
        }
        "shutdown" => {
            let _ = write_message(
                writer,
                &Message::Response(Response::new_ok(req.id, serde_json::Value::Null)),
            );
        }
        other => {
            let _ = write_message(
                writer,
                &Message::Response(Response::new_err(
                    req.id,
                    ErrorCode::MethodNotFound as i32,
                    format!("method not found: {other}"),
                )),
            );
        }
    }
}

fn handle_notification<W: Write>(server: &mut Server, writer: &mut W, not: LspNotification) {
    match not.method.as_str() {
        "initialized" => {} // nothing to do yet
        "textDocument/didOpen" => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(not.params) {
                let uri = params.text_document.uri;
                for message in server.handle_open_or_change(&uri, &params.text_document.text) {
                    let _ = write_message(writer, &message);
                }
            }
        }
        "textDocument/didChange" => {
            if let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(not.params) {
                let uri = params.text_document.uri;
                // Full sync (see the `textDocumentSync: FULL` capability
                // above) guarantees exactly one entry whose `text` is the
                // complete new document.
                if let Some(change) = params.content_changes.into_iter().next() {
                    for message in server.handle_open_or_change(&uri, &change.text) {
                        let _ = write_message(writer, &message);
                    }
                }
            }
        }
        "textDocument/didClose" => {
            // Deliberately a no-op: the file stays in the project (its
            // registry entries and dependency edges are still real and
            // other open files may depend on them), it's just no longer
            // tracked as "currently open" for the purposes of picking a
            // URI to publish under later.
        }
        _ => {
            // An unrecognized notification is just ignored, per the spec.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::Cursor;

    // lsp.rs had 0% test coverage before this - every function here was
    // already generic over its reader/writer or otherwise pure, except
    // the actual message loop, which hardcoded real stdin/stdout inside
    // `run`. Splitting that into `run_loop` (generic, tested here) plus a
    // thin `run` wrapper needed no other change - the loop body already
    // only ever calls the generic `read_message`/`write_message`.
    //
    // Tests still build/inspect messages as plain `serde_json::Value` -
    // `lsp_server::Message`'s own `#[serde(untagged)]` Request/Response/
    // Notification shape round-trips through `Value` exactly the same
    // way, so this keeps every existing assertion (`responses[0]["id"]`,
    // etc.) unchanged while the real transport underneath now genuinely
    // goes through `lsp_server::Message::read`/`write`.

    fn encode(msgs: &[Value]) -> Vec<u8> {
        let mut buf = Vec::new();
        for m in msgs {
            let message: Message = serde_json::from_value(m.clone()).unwrap();
            write_message(&mut buf, &message).unwrap();
        }
        buf
    }

    fn decode_all(bytes: &[u8]) -> Vec<Value> {
        let mut cursor = Cursor::new(bytes);
        let mut out = Vec::new();
        loop {
            match read_message(&mut cursor) {
                ReadOutcome::Message(m) => out.push(serde_json::to_value(&m).unwrap()),
                ReadOutcome::Eof => break,
                ReadOutcome::Disconnected(e) | ReadOutcome::MalformedBody(e) => {
                    panic!("unexpected read_message failure in test: {e}")
                }
            }
        }
        out
    }

    fn request(id: i64, method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    fn notification(method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "method": method, "params": params})
    }

    /// A fresh, unique scratch directory per test - real files on disk,
    /// since `load_workspace` walks the real filesystem via
    /// `collect_php_files`/`std::fs::read`, not something worth faking.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "phpstan_rs_lsp_test_{label}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, contents).unwrap();
            path
        }
        fn uri(&self) -> String {
            path_to_uri(&self.0).to_string()
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
        let b_notification = messages.iter().find(|m| {
            m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == b_uri
        });
        let Some(b_notification) = b_notification else {
            panic!("expected a publishDiagnostics notification for b.php, got: {messages:?}");
        };
        assert!(
            b_notification["params"]["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains("greet"),
            "expected b.php's diagnostic to mention the now-missing greet() method, got: {b_notification:?}"
        );
        // A real range, not the old hardcoded {0,0}-{0,1} placeholder -
        // pointing at `$g->greet()` itself (0-indexed LSP columns for a
        // 1-indexed byte range starting at real column 48).
        assert_eq!(
            b_notification["params"]["diagnostics"][0]["range"],
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

    // Finding 1 from the architecture audit - a malformed message body
    // (a real Content-Length header, correctly consumed, but invalid
    // JSON) used to terminate the whole session rather than just being
    // skipped; a per-file parser panic during didOpen/didChange/the
    // initial workspace scan used to crash the whole server process
    // rather than just failing to analyze that one file.

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
}
