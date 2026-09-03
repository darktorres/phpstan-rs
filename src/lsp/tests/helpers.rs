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

pub(super) fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
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
