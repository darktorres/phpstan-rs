//! A workspace load runs off the message loop. While one runs, the loop still
//! answers `shutdown`, removes a request that `$/cancelRequest` names, and
//! writes log lines through; every other message waits, in arrival order,
//! until the load installs. Each test holds a load at a gate the test opens,
//! and reads the output while the gate is still closed.

use super::super::*;
use super::helpers::*;
use serde_json::{Value, json};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A `run_loop` on its own thread, fed through `tx`, writing into a buffer the
/// test reads while the loop runs.
struct LiveServer {
    tx: crossbeam_channel::Sender<Message>,
    output: Arc<Mutex<Vec<u8>>>,
    handle: JoinHandle<i32>,
}

impl LiveServer {
    /// Starts a server whose next load runs under `hook`.
    fn start(hook: Option<LoadHook>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&output);
        let handle = std::thread::spawn(move || {
            LOAD_HOOK.with(|slot| *slot.borrow_mut() = hook);
            run_loop(
                &rx,
                writer,
                env!("CARGO_PKG_VERSION"),
                crate::app::config_schema(),
            )
        });
        LiveServer { tx, output, handle }
    }

    fn send(&self, message: Value) {
        self.tx.send(to_message(message)).unwrap();
    }

    /// Waits until the output satisfies `seen`, and returns the output then.
    fn wait_for(&self, what: &str, seen: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let messages = decode_all(&self.output.lock().unwrap());
            if seen(&messages) {
                return messages;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; the output so far: {messages:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn has_response(messages: &[Value], id: i64) -> bool {
    messages.iter().any(|m| m["id"] == json!(id))
}

fn is_progress(message: &Value, kind: &str) -> bool {
    message["method"] == "$/progress" && message["params"]["value"]["kind"] == kind
}

fn is_log_line(message: &Value, text: &str) -> bool {
    message["method"] == "window/logMessage" && message["params"]["message"] == text
}

fn position_of(messages: &[Value], predicate: impl Fn(&Value) -> bool) -> usize {
    messages.iter().position(predicate).unwrap()
}

#[test]
fn a_message_held_by_a_load_is_answered_after_the_load_installs() {
    let dir = TempDir::new("load_held_answer");
    let foo = dir.write("Foo.php", "<?php class Foo {}");
    let use_src = "<?php function useFoo(Foo $f) {}";
    let use_uri = path_to_uri(&dir.write("Use.php", use_src)).to_string();
    let pos = lsp_pos(use_src, "Foo $f");
    let (gate, gate_open) = crossbeam_channel::bounded::<()>(0);
    let server = LiveServer::start(Some(LoadHook::Gate(gate_open)));

    server.send(request(
        1,
        "initialize",
        json!({"rootUri": dir.uri(), "capabilities": {}, "workDoneToken": "scan"}),
    ));
    server.send(notification("initialized", json!({})));
    server.wait_for("the scan to announce itself", |m| {
        m.iter().any(|m| is_progress(m, "begin"))
    });

    server.send(definition_request(2, &use_uri, pos.clone()));
    server.send(notification("$/cancelRequest", json!({"id": 2})));
    server.send(request(3, "shutdown", json!({})));
    server.send(notification(
        "window/logMessage",
        json!({"type": 4, "message": "during"}),
    ));
    server.wait_for(
        "the cancel, the shutdown and the log line to be answered",
        |m| has_response(m, 2) && has_response(m, 3) && m.iter().any(|m| is_log_line(m, "during")),
    );

    server.send(definition_request(4, &use_uri, pos));
    server.send(notification(
        "window/logMessage",
        json!({"type": 4, "message": "after"}),
    ));
    let held = server.wait_for("the log line sent after the held request", |m| {
        m.iter().any(|m| is_log_line(m, "after"))
    });
    assert!(
        !has_response(&held, 4),
        "a held request was answered before its load installed: {held:?}"
    );
    assert!(
        !held.iter().any(|m| is_progress(m, "end")),
        "the load ended while its gate was still closed: {held:?}"
    );

    drop(gate);
    let installed = server.wait_for("the held request to be answered", |m| has_response(m, 4));
    assert!(
        position_of(&installed, |m| is_progress(m, "end"))
            < position_of(&installed, |m| m["id"] == json!(4)),
        "the held request was answered before the load's progress ended: {installed:?}"
    );
    assert_eq!(
        response_result(&installed, 4)["uri"],
        json!(path_to_uri(&foo).to_string())
    );
    let cancelled: Vec<&Value> = installed.iter().filter(|m| m["id"] == json!(2)).collect();
    assert_eq!(
        cancelled.len(),
        1,
        "a cancelled request is answered exactly once: {installed:?}"
    );
    assert_eq!(
        cancelled[0]["error"]["code"],
        json!(ErrorCode::RequestCanceled as i32)
    );
    assert_eq!(response_result(&installed, 3), &Value::Null);

    server.send(notification("exit", json!({})));
    assert_eq!(server.handle.join().unwrap(), 0);
}

#[test]
fn an_exit_sent_during_a_load_ends_the_session_only_after_the_load_installs() {
    let dir = TempDir::new("load_exit_held");
    dir.write("Foo.php", "<?php class Foo {}");
    let (gate, gate_open) = crossbeam_channel::bounded::<()>(0);
    let server = LiveServer::start(Some(LoadHook::Gate(gate_open)));

    server.send(request(
        1,
        "initialize",
        json!({"rootUri": dir.uri(), "capabilities": {}}),
    ));
    server.send(notification("initialized", json!({})));
    server.send(request(2, "shutdown", json!({})));
    server.send(notification("exit", json!({})));
    server.send(notification(
        "window/logMessage",
        json!({"type": 4, "message": "after exit"}),
    ));
    server.wait_for("the shutdown answer and the line after the exit", |m| {
        has_response(m, 2) && m.iter().any(|m| is_log_line(m, "after exit"))
    });
    // The loop has received the exit (it wrote the line after it), and it
    // cannot end while the load holds the gate.
    assert!(!server.handle.is_finished());

    drop(gate);
    assert_eq!(server.handle.join().unwrap(), 0);
}

#[test]
fn a_folder_added_during_a_load_is_loaded_once_the_load_installs() {
    let a = TempDir::new("load_add_first");
    a.write("A.php", "<?php class A {}");
    let b = TempDir::new("load_add_second");
    let bar = b.write("Bar.php", "<?php class Bar {}");
    let use_src = "<?php function useBar(Bar $b) {}";
    let use_uri = path_to_uri(&b.write("Use.php", use_src)).to_string();
    let pos = lsp_pos(use_src, "Bar $b");
    let (gate, gate_open) = crossbeam_channel::bounded::<()>(0);
    let server = LiveServer::start(Some(LoadHook::Gate(gate_open)));

    server.send(request(
        1,
        "initialize",
        json!({"rootUri": a.uri(), "capabilities": {}}),
    ));
    server.send(notification("initialized", json!({})));
    server.send(did_change_workspace_folders(
        vec![workspace_folder(&b)],
        vec![],
    ));
    server.send(definition_request(2, &use_uri, pos));
    server.send(notification(
        "window/logMessage",
        json!({"type": 4, "message": "queued"}),
    ));
    server.wait_for("the messages sent during the load", |m| {
        m.iter().any(|m| is_log_line(m, "queued"))
    });

    drop(gate);
    let answered = server.wait_for("the definition in the added folder", |m| has_response(m, 2));
    assert_eq!(
        response_result(&answered, 2)["uri"],
        json!(path_to_uri(&bar).to_string()),
        "the folder added during the load must be loaded by the time its request runs"
    );

    server.send(request(3, "shutdown", json!({})));
    server.send(notification("exit", json!({})));
    assert_eq!(server.handle.join().unwrap(), 0);
}

#[test]
fn an_edit_held_by_a_load_is_checked_once_the_load_installs() {
    let dir = TempDir::new("load_edit_held");
    dir.write("Foo.php", "<?php class Foo {}");
    let uri = path_to_uri(&dir.write("Use.php", "<?php function useFoo(): void {}")).to_string();
    let (gate, gate_open) = crossbeam_channel::bounded::<()>(0);
    let server = LiveServer::start(Some(LoadHook::Gate(gate_open)));

    server.send(request(
        1,
        "initialize",
        json!({"rootUri": dir.uri(), "capabilities": {}}),
    ));
    server.send(notification("initialized", json!({})));
    server.send(notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": "<?php function useFoo(): void {}"}}),
    ));
    server.send(notification(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"text": "<?php function useFoo(): void { missing_function(); }"}]
        }),
    ));
    server.send(notification(
        "window/logMessage",
        json!({"type": 4, "message": "queued"}),
    ));
    server.wait_for("the edit to be held", |m| {
        m.iter().any(|m| is_log_line(m, "queued"))
    });

    drop(gate);
    server.wait_for("the held edit's diagnostics", |m| {
        m.iter().any(|m| {
            m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == uri.as_str()
                && m["params"]["diagnostics"]
                    .to_string()
                    .contains("missing_function")
        })
    });

    server.send(request(2, "shutdown", json!({})));
    server.send(notification("exit", json!({})));
    assert_eq!(server.handle.join().unwrap(), 0);
}

#[test]
fn a_panic_in_a_load_ends_the_message_loop_as_a_panic_in_the_loop_would() {
    let dir = TempDir::new("load_panic");
    dir.write("Foo.php", "<?php class Foo {}");
    let server = LiveServer::start(Some(LoadHook::Panic));

    server.send(request(
        1,
        "initialize",
        json!({"rootUri": dir.uri(), "capabilities": {}}),
    ));
    server.send(notification("initialized", json!({})));
    assert!(server.handle.join().is_err());
}
