//! A minimal Language Server Protocol server, run via `phpstan-rs --lsp`.
//! Communicates over stdio using LSP's standard JSON-RPC-over-`Content-
//! Length`-framed-messages transport. Hand-rolls the actual protocol
//! logic (message loop, method dispatch) directly against `serde_json`
//! rather than pulling in an LSP framework crate (which would also mean
//! an async runtime) - this project's first and only dependency is
//! `serde`/`serde_json` itself, used only for getting JSON parsing and
//! string escaping right on arbitrary real file content.
//!
//! Known limitation, worth being upfront about: `checker::Diagnostic`
//! doesn't carry a real source position (line/column) today - the lexer
//! discards position info after tokenizing, and no span threads through
//! the parser/AST/checker. Every diagnostic reported here is anchored at
//! line 1, column 1 of its file, with the location label folded into the
//! message text, rather than pointing at the actual offending code. This
//! still delivers real, correctly-invalidated diagnostics as you type
//! (that's the point of Phase 7's incremental/transitive work paying off
//! here), just not yet pinpoint-accurate within a file. Real position
//! tracking is a natural, separate follow-up.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::checker::Diagnostic;
use crate::project::Project;

/// Reads one framed LSP message (`Content-Length: N\r\n\r\n<N bytes of
/// JSON>`) from `reader`. Returns `None` at a clean EOF (the client closed
/// the pipe without sending `exit`, which real clients occasionally do).
fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
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
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message had no Content-Length header",
        ));
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let value =
        serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(value))
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

/// One diagnostic's LSP JSON shape. Every diagnostic is anchored at the
/// very start of the file (see the module doc comment) - `location` (a
/// symbolic label like `"ClassName::methodName"`, not a position) is
/// folded into the message text instead of dropped.
fn diagnostic_to_json(d: &Diagnostic) -> Value {
    json!({
        "range": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 0, "character": 1},
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
        crate::collect_php_files(root, &mut files);
        for path in files {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let source = String::from_utf8_lossy(&bytes).into_owned();
            self.project.load_file(path, &source);
        }
        self.project.finish_loading();
    }

    fn handle_open_or_change(&mut self, uri: &str, text: &str) -> Vec<Value> {
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        self.open_uris.insert(path.clone(), uri.to_string());
        let results = self.project.update_file(&path, text);
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

    let mut server = Server::new();

    loop {
        let message = match read_message(&mut reader) {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) => {
                eprintln!("phpstan-rs lsp: failed to read message: {e}");
                break;
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
                    let _ = write_message(&mut writer, &response(id, result));
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
                        let _ = write_message(&mut writer, &notification);
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
                        let _ = write_message(&mut writer, &notification);
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
                    let _ = write_message(&mut writer, &response(id, Value::Null));
                }
            }
            Some("exit") => break,
            Some(other) => {
                if let Some(id) = id {
                    let _ = write_message(
                        &mut writer,
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
