//! The server's session record: a session of the language server, run in
//! this process through its own message loop, over a workspace, and what
//! it answers, as text two builds' records can be compared by.
//!
//! The record has, in order:
//! - each loaded file with its role, and the registry dump of what the
//!   session loaded;
//! - each file's published diagnostics, their ranges in UTF-16;
//! - over a sample of files drawn with a fixed seed (context files among
//!   them) and positions in each at an identifier or a variable: the
//!   definition, hover and references answers, and the code actions of
//!   each diagnostic published for the file.
//!
//! The session runs twice: the first to learn the files and diagnostics
//! the sample is drawn from, the second to ask about it.

use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex};

use lsp_server::{Message, Notification, Request, RequestId};
use serde_json::{Value, json};
use syntax::span::{LineIndex, Unit};
use syntax::token::Token;

use super::protocol::{ReadOutcome, path_to_uri, read_message};

/// The record of a session over the workspace at `root`, asking about
/// `files` files drawn with `seed`, at up to `positions` positions each.
pub fn record_session(root: &Path, seed: u64, files: usize, positions: usize) -> String {
    let root_uri = path_to_uri(root);
    let opening = || {
        vec![
            request(
                1,
                "initialize",
                json!({"rootUri": root_uri, "capabilities": {}}),
            ),
            notification("initialized", json!({})),
        ]
    };

    let mut first = opening();
    first.push(request(2, "phpstan-rs/sessionDump", Value::Null));
    let replies = converse(first);
    let dump = response_of(&replies, 2);
    let loaded: Vec<(String, String)> = dump["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| Some((file[0].as_str()?.into(), file[1].as_str()?.into())))
        .collect();
    let mut published: Vec<(String, Value)> = replies
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .map(|m| {
            (
                m["params"]["uri"].as_str().unwrap_or_default().to_string(),
                m["params"]["diagnostics"].clone(),
            )
        })
        .collect();
    published.sort_by(|a, b| a.0.cmp(&b.0));

    let mut rng = Rng(seed);
    let mut paths: Vec<&str> = loaded.iter().map(|(_, path)| path.as_str()).collect();
    paths.sort_unstable();
    let mut sample: Vec<&str> = Vec::new();
    while sample.len() < files.min(paths.len()) {
        let at = rng.below(paths.len());
        sample.push(paths.remove(at));
    }
    sample.sort_unstable();
    let mut second = opening();
    let mut asked: Vec<(i64, String)> = Vec::new();
    // After the opening's ids and the shutdown's.
    let mut next_id = 10;
    let mut ask = |second: &mut Vec<Message>, method: &str, params: Value, label: String| {
        second.push(request(next_id, method, params));
        asked.push((next_id, format!("{method} {label}")));
        next_id += 1;
    };
    for path in &sample {
        let uri = path_to_uri(Path::new(path));
        let uri_text = uri.as_str().to_string();
        for (line, character) in sample_positions(Path::new(path), &mut rng, positions) {
            let position = json!({"line": line, "character": character});
            let document = json!({"uri": uri, "position": position});
            let label = format!("{uri_text}:{line}:{character}");
            ask(
                &mut second,
                "textDocument/definition",
                document.clone(),
                label.clone(),
            );
            ask(
                &mut second,
                "textDocument/hover",
                document.clone(),
                label.clone(),
            );
            let mut references = document;
            references["context"] = json!({"includeDeclaration": true});
            ask(&mut second, "textDocument/references", references, label);
        }
        let diagnostics = published
            .iter()
            .find(|(published, _)| *published == uri_text)
            .and_then(|(_, diagnostics)| diagnostics.as_array().cloned())
            .unwrap_or_default();
        for diagnostic in diagnostics {
            let range = diagnostic["range"].clone();
            let label = format!("{uri_text} {range}");
            ask(
                &mut second,
                "textDocument/codeAction",
                json!({
                    "textDocument": {"uri": uri},
                    "range": range,
                    "context": {"diagnostics": [diagnostic]},
                }),
                label,
            );
        }
    }
    let answers = converse(second);

    let mut text = String::new();
    text.push_str("== files\n");
    for (role, path) in &loaded {
        text.push_str(&format!("{role}\t{path}\n"));
    }
    text.push_str("== registry\n");
    text.push_str(dump["registry"].as_str().unwrap_or_default());
    text.push_str("\n== diagnostics\n");
    for (uri, diagnostics) in &published {
        text.push_str(&format!("{uri}\t{diagnostics}\n"));
    }
    text.push_str("== answers\n");
    for (id, label) in &asked {
        text.push_str(&format!("{label}\t{}\n", response_of(&answers, *id)));
    }
    text
}

/// Positions at identifiers and variables of the file at `path`, up to
/// `count` of them drawn by `rng`, in order, as LSP lines and UTF-16
/// characters.
fn sample_positions(path: &Path, rng: &mut Rng, count: usize) -> Vec<(u32, u32)> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let mut starts: Vec<u32> = syntax::lexer::Lexer::from_bytes(&bytes)
        .tokenize_with_spans()
        .into_iter()
        .filter(|(token, _, _)| matches!(token, Token::Identifier(_) | Token::Variable(_)))
        .map(|(_, span, _)| span.start)
        .collect();
    let mut picked = Vec::new();
    while picked.len() < count && !starts.is_empty() {
        let at = rng.below(starts.len());
        picked.push(starts.remove(at));
    }
    picked.sort_unstable();
    let lines = LineIndex::new(&bytes);
    picked
        .into_iter()
        .map(|offset| {
            (
                lines.line_col(offset).line - 1,
                lines.column(offset, Unit::Utf16),
            )
        })
        .collect()
}

/// Runs a session of `messages`, then `shutdown` and `exit`, through the
/// server's message loop in this process, and gives every message it
/// wrote.
fn converse(mut messages: Vec<Message>) -> Vec<Value> {
    messages.push(request(9, "shutdown", Value::Null));
    messages.push(notification("exit", Value::Null));
    let (sender, receiver) = crossbeam_channel::unbounded();
    for message in messages {
        // The receiver lives below, so a send cannot fail.
        let _ = sender.send(message);
    }
    drop(sender);
    let writer = Arc::new(Mutex::new(Vec::new()));
    super::run_loop(
        &receiver,
        Arc::clone(&writer),
        env!("CARGO_PKG_VERSION"),
        crate::app::config_schema(),
    );
    let bytes = writer.lock().map(|bytes| bytes.clone()).unwrap_or_default();
    let mut cursor = Cursor::new(bytes);
    let mut out = Vec::new();
    while let ReadOutcome::Message(message) = read_message(&mut cursor) {
        if let Ok(value) = serde_json::to_value(&message) {
            out.push(value);
        }
    }
    out
}

/// The result of the response to request `id`, or its error.
fn response_of(replies: &[Value], id: i64) -> Value {
    replies
        .iter()
        .find(|m| m["id"] == json!(id) && m.get("method").is_none())
        .map(|m| match m.get("error") {
            Some(error) => json!({"error": error}),
            None => m["result"].clone(),
        })
        .unwrap_or(Value::Null)
}

fn request(id: i64, method: &str, params: Value) -> Message {
    Message::Request(Request::new(
        RequestId::from(id as i32),
        method.to_string(),
        params,
    ))
}

fn notification(method: &str, params: Value) -> Message {
    Message::Notification(Notification::new(method.to_string(), params))
}

/// A fixed sequence of numbers from a seed (xorshift64*), so that the
/// sample is the same in every run.
struct Rng(u64);

impl Rng {
    /// A number below `bound`, which is not zero.
    fn below(&mut self, bound: usize) -> usize {
        let mut x = self.0.max(1);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) % bound as u64) as usize
    }
}
