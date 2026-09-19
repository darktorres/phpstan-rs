use super::super::protocol::{ReadOutcome, read_message};
use super::super::*;
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

pub(super) fn encode(msgs: &[Value]) -> Vec<u8> {
    let mut buf = Vec::new();
    for m in msgs {
        let message: Message = serde_json::from_value(m.clone()).unwrap();
        write_message(&mut buf, &message).unwrap();
    }
    buf
}

pub(super) fn decode_all(bytes: &[u8]) -> Vec<Value> {
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

pub(super) fn request(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub(super) fn response(id: i64, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// The 0-indexed LSP `{line, character}` of `needle`'s first occurrence
/// in `src` - lets a hover/definition test point at "the `bar` in
/// `$x->bar()`" by the substring itself rather than a hand-counted
/// line/column, which silently drifts the moment the fixture source
/// changes. UTF-16-vs-UTF-8 column counting isn't a concern here - every
/// fixture below is plain ASCII.
pub(super) fn lsp_pos(src: &str, needle: &str) -> Value {
    let byte_offset = src.find(needle).unwrap_or_else(|| {
        panic!("{needle:?} not found in fixture source");
    });
    let (line, char_offset) = {
        let mut line = 0u32;
        let mut last_newline = 0usize;
        for (i, b) in src.as_bytes()[..byte_offset].iter().enumerate() {
            if *b == b'\n' {
                line += 1;
                last_newline = i + 1;
            }
        }
        (line, src[last_newline..byte_offset].chars().count() as u32)
    };
    json!({"line": line, "character": char_offset})
}

pub(super) fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// Loads `files` (each `(name, contents)`) into a fresh workspace via
/// `initialize`'s own `rootUri` scan, then sends `extra` requests/
/// notifications, then `exit`. Returns every response/notification
/// `run_loop` wrote.
pub(super) fn run(dir: &TempDir, extra: &[Value]) -> Vec<Value> {
    let mut input = vec![request(
        1,
        "initialize",
        json!({"rootUri": dir.uri(), "capabilities": {}}),
    )];
    input.extend_from_slice(extra);
    input.push(notification("exit", json!({})));
    let bytes = encode(&input);
    // `unbounded`, not production's `bounded(64)`: nothing drains
    // concurrently while `pump_messages` runs synchronously below, so a
    // bounded channel would deadlock inside `sender.send()` on a fixture
    // queuing more messages than its capacity, before `run_loop` ever
    // gets to run at all.
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut reader = Cursor::new(bytes);
    pump_messages(&mut reader, &tx);
    // Required: `Receiver::recv_timeout` only ever reports `Disconnected`
    // once every `Sender` clone is dropped - keeping `tx` alive here
    // would make `run_loop` spin on `RecvTimeoutError::Timeout` forever
    // for any fixture that omits a trailing `exit` notification.
    drop(tx);
    let mut writer = Vec::new();
    run_loop(&rx, &mut writer);
    decode_all(&writer)
}

/// Builds a `Message` directly from a JSON `Value`, for a test that
/// sends messages straight onto a channel rather than through
/// `encode`/`pump_messages`'s byte-framing round trip.
pub(super) fn to_message(v: Value) -> Message {
    serde_json::from_value(v).unwrap()
}

pub(super) fn hover_request(id: i64, uri: &str, pos: Value) -> Value {
    request(
        id,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": pos}),
    )
}

pub(super) fn definition_request(id: i64, uri: &str, pos: Value) -> Value {
    request(
        id,
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": pos}),
    )
}

pub(super) fn response_result(messages: &[Value], id: i64) -> Value {
    messages
        .iter()
        .find(|m| m["id"] == id)
        .unwrap_or_else(|| panic!("no response with id {id} in {messages:?}"))["result"]
        .clone()
}

/// A fresh, unique scratch directory per test - real files on disk,
/// since `load_workspace` walks the real filesystem via
/// `collect_php_files`/`std::fs::read`, not something worth faking.
pub(super) struct TempDir(pub(super) PathBuf);
impl TempDir {
    pub(super) fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "phpstan_rs_lsp_test_{label}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    pub(super) fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
    pub(super) fn uri(&self) -> String {
        path_to_uri(&self.0).to_string()
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A `BufRead` that always fails with a non-`InvalidData` I/O error -
/// drives `ReadOutcome::Disconnected` specifically, which a `Cursor`
/// alone can never produce (only `Eof`, or `MalformedBody`/`InvalidData`
/// via malformed bytes).
pub(super) struct FailingReader;
impl std::io::Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("simulated transport failure"))
    }
}
impl std::io::BufRead for FailingReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        Err(std::io::Error::other("simulated transport failure"))
    }
    fn consume(&mut self, _amt: usize) {}
}
