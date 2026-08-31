//! A minimal Language Server Protocol server, run via `phpstan-rs --lsp`.
//! Communicates over stdio using LSP's standard JSON-RPC-over-`Content-
//! Length`-framed-messages transport. Hand-rolls the actual protocol
//! logic (message loop, method dispatch) directly against `serde_json`
//! rather than pulling in an LSP framework crate (which would also mean
//! an async runtime) - this project's first and only dependency is
//! `serde`/`serde_json` itself, used only for getting JSON parsing and
//! string escaping right on arbitrary real file content.
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

use serde_json::{Value, json};

use crate::checker::Diagnostic;
use crate::project::Project;

/// Reads one framed LSP message (`Content-Length: N\r\n\r\n<N bytes of
/// JSON>`) from `reader`. Returns `None` at a clean EOF (the client closed
/// the pipe without sending `exit`, which real clients occasionally do).
/// The outcome of trying to read one framed LSP message. `Eof` (a clean
/// transport disconnection) and `Disconnected` (a real I/O failure
/// reading the header/body - the stream's position afterward is
/// unknown) both mean "stop the loop". `MalformedBody` is the one
/// genuinely recoverable case: the header and exactly `Content-Length`
/// bytes were both read successfully - the reader is correctly
/// positioned at the start of the *next* message - they just didn't
/// parse as JSON, so only this one message needs to be skipped, not the
/// whole connection torn down.
enum ReadOutcome {
    Message(Value),
    Eof,
    Disconnected(io::Error),
    MalformedBody(io::Error),
}

fn read_message<R: BufRead>(reader: &mut R) -> ReadOutcome {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = match reader.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => return ReadOutcome::Disconnected(e),
        };
        if n == 0 {
            return ReadOutcome::Eof;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break; // blank line ends the header block
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = value.trim().parse().ok();
        }
        // Any other header (e.g. Content-Type) is accepted and ignored.
    }
    let Some(len) = content_length else {
        // No known length to skip past to resynchronize on the next
        // message - not safely recoverable, unlike a malformed body of a
        // known length below.
        return ReadOutcome::Disconnected(io::Error::new(
            io::ErrorKind::InvalidData,
            "message had no Content-Length header",
        ));
    };
    let mut buf = vec![0u8; len];
    if let Err(e) = reader.read_exact(&mut buf) {
        return ReadOutcome::Disconnected(e);
    }
    match serde_json::from_slice(&buf) {
        Ok(value) => ReadOutcome::Message(value),
        Err(e) => ReadOutcome::MalformedBody(io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

/// Writes one framed LSP message to `writer` (stdout in practice) and
/// flushes it - LSP has no message boundary other than `Content-Length`,
/// so a buffered-but-unflushed write would leave the client waiting.
fn write_message<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://")?;
    Some(PathBuf::from(percent_decode(path)))
}

fn path_to_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// Decodes the small set of percent-escapes a `file://` URI from a real
/// editor realistically contains (spaces and a few punctuation
/// characters) - not a general-purpose URI decoder, just enough for real
/// filesystem paths.
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

/// One diagnostic's LSP JSON shape. `location` (a symbolic label like
/// `"ClassName::methodName"`, not a position) is folded into the message
/// text instead of dropped - LSP's own diagnostic shape has no field for
/// it. `Span`'s 1-indexed line/col become LSP's 0-indexed line/character.
fn diagnostic_to_json(d: &Diagnostic) -> Value {
    json!({
        "range": {
            "start": {
                "line": d.span.start.line.saturating_sub(1),
                "character": d.span.start.col.saturating_sub(1),
            },
            "end": {
                "line": d.span.end.line.saturating_sub(1),
                "character": d.span.end.col.saturating_sub(1),
            },
        },
        "severity": 1, // Error
        "source": "phpstan-rs",
        "message": format!("{}: {}", d.location, d.message),
    })
}

fn publish_diagnostics_notification(uri: &str, diagnostics: &[Diagnostic]) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diagnostics.iter().map(diagnostic_to_json).collect::<Vec<_>>(),
        },
    })
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

struct Server {
    project: Project,
    // Every URI the client currently has open or has otherwise seen, so
    // `initialize`'s workspace scan and `didOpen`/`didChange` share one
    // path->diagnostics story instead of the workspace scan being
    // invisible to later incremental updates.
    open_uris: HashMap<PathBuf, String>,
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

    fn handle_open_or_change(&mut self, uri: &str, text: &str) -> Vec<Value> {
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        self.open_uris.insert(path.clone(), uri.to_string());
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
                publish_diagnostics_notification(&dep_uri, &diags)
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

        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned();

        match method {
            Some("initialize") => {
                if let Some(root) = message
                    .pointer("/params/rootUri")
                    .and_then(Value::as_str)
                    .and_then(uri_to_path)
                    .or_else(|| {
                        message
                            .pointer("/params/workspaceFolders/0/uri")
                            .and_then(Value::as_str)
                            .and_then(uri_to_path)
                    })
                {
                    server.load_workspace(&root);
                }
                if let Some(id) = id {
                    let result = json!({
                        "capabilities": {
                            // Full sync: didChange always carries the
                            // whole new document text, not incremental
                            // ranges - simplest correct choice, and
                            // update_file already takes a full source
                            // string anyway.
                            "textDocumentSync": 1,
                        },
                        "serverInfo": {"name": "phpstan-rs", "version": env!("CARGO_PKG_VERSION")},
                    });
                    let _ = write_message(writer, &response(id, result));
                }
            }
            Some("initialized") => {} // notification, nothing to do yet
            Some("textDocument/didOpen") => {
                let uri = message
                    .pointer("/params/textDocument/uri")
                    .and_then(Value::as_str);
                let text = message
                    .pointer("/params/textDocument/text")
                    .and_then(Value::as_str);
                if let (Some(uri), Some(text)) = (uri, text) {
                    for notification in server.handle_open_or_change(uri, text) {
                        let _ = write_message(writer, &notification);
                    }
                }
            }
            Some("textDocument/didChange") => {
                let uri = message
                    .pointer("/params/textDocument/uri")
                    .and_then(Value::as_str);
                // Full sync (see the `textDocumentSync: 1` capability
                // above) guarantees exactly one entry whose `text` is the
                // complete new document.
                let text = message
                    .pointer("/params/contentChanges/0/text")
                    .and_then(Value::as_str);
                if let (Some(uri), Some(text)) = (uri, text) {
                    for notification in server.handle_open_or_change(uri, text) {
                        let _ = write_message(writer, &notification);
                    }
                }
            }
            Some("textDocument/didClose") => {
                // Deliberately a no-op: the file stays in the project (its
                // registry entries and dependency edges are still real
                // and other open files may depend on them), it's just no
                // longer tracked as "currently open" for the purposes of
                // picking a URI to publish under later.
            }
            Some("shutdown") => {
                if let Some(id) = id {
                    let _ = write_message(writer, &response(id, Value::Null));
                }
            }
            Some("exit") => break,
            Some(other) => {
                if let Some(id) = id {
                    let _ = write_message(
                        writer,
                        &error_response(id, -32601, &format!("method not found: {other}")),
                    );
                }
                // An unrecognized *notification* (no id) is just ignored,
                // per the spec.
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // lsp.rs had 0% test coverage before this - every function here was
    // already generic over its reader/writer or otherwise pure, except
    // the actual message loop, which hardcoded real stdin/stdout inside
    // `run`. Splitting that into `run_loop` (generic, tested here) plus a
    // thin `run` wrapper needed no other change - the loop body already
    // only ever calls the generic `read_message`/`write_message`.

    fn encode(msgs: &[Value]) -> Vec<u8> {
        let mut buf = Vec::new();
        for m in msgs {
            write_message(&mut buf, m).unwrap();
        }
        buf
    }

    fn decode_all(bytes: &[u8]) -> Vec<Value> {
        let mut cursor = Cursor::new(bytes);
        let mut out = Vec::new();
        loop {
            match read_message(&mut cursor) {
                ReadOutcome::Message(v) => out.push(v),
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
            path_to_uri(&self.0)
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
            request(1, "initialize", json!({"rootUri": dir.uri()})),
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
            request(1, "initialize", json!({"rootUri": dir.uri()})),
            notification(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": a_uri, "text": "<?php class Greeter {}"}}),
            ),
            notification("exit", json!({})),
        ]);
        let mut reader = Cursor::new(input);
        let mut writer = Vec::new();
        run_loop(&mut reader, &mut writer);

        let messages = decode_all(&writer);
        let b_uri = path_to_uri(&b_path);
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
        let uri = "file:///tmp/foo.php";
        let path = uri_to_path(uri).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/foo.php"));
        assert_eq!(path_to_uri(&path), uri);
        assert_eq!(uri_to_path("not-a-uri"), None);
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
            request(1, "initialize", json!({"rootUri": dir.uri()})),
            notification(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": format!("file://{}/bad.php", dir.0.display()),
                        // Momentarily-invalid syntax, the normal state
                        // while a file is being typed - an unclosed
                        // parameter list trips this parser's own
                        // panic!-based error reporting, its only
                        // mechanism for malformed input.
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
