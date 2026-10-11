//! Startup regression coverage for `phpstan-rs lsp`: the server must answer
//! `initialize` well inside the budget the editor smoke test allows its
//! client (10 s), from an empty stub cache as well as a warm one. This was
//! ~40 s on Windows before the program ran on a large-stack thread, and it
//! failed only on Windows CI, so it runs as a real subprocess.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BUDGET: Duration = Duration::from_secs(10);

fn framed(json: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{json}", json.len()).into_bytes()
}

/// Spawns `phpstan-rs lsp` with `cache_dir` as its stub cache, sends
/// `initialize`, and returns how long the first reply took.
fn time_to_initialize_reply(cache_dir: &std::path::Path) -> Duration {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phpstan-rs"))
        .arg("lsp")
        .env("PHPSTAN_RS_CACHE_DIR", cache_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let started = Instant::now();
    stdin
        .write_all(&framed(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#,
        ))
        .unwrap();

    // The reply is read on its own thread so a hung server fails the test
    // at the budget instead of blocking it.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut header = String::new();
        let mut len = 0usize;
        loop {
            header.clear();
            if stdout.read_line(&mut header).unwrap_or(0) == 0 {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length: ") {
                len = value.trim().parse().unwrap_or(0);
            }
            if header == "\r\n" {
                break;
            }
        }
        let mut body = vec![0u8; len];
        let _ = stdout.read_exact(&mut body);
        let _ = tx.send(body);
    });

    let reply = rx.recv_timeout(BUDGET);
    let elapsed = started.elapsed();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    let body = reply.unwrap_or_else(|_| panic!("no initialize reply within {BUDGET:?}"));
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("\"id\":1"), "unexpected reply: {text}");
    elapsed
}

#[test]
fn initialize_is_answered_within_budget_from_an_empty_cache() {
    let dir = std::env::temp_dir().join(format!(
        "phpstan-rs-lsp-startup-cold-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let elapsed = time_to_initialize_reply(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(elapsed < BUDGET, "cold start took {elapsed:?}");
}

#[test]
fn initialize_is_answered_within_budget_from_a_warm_cache() {
    let dir = std::env::temp_dir().join(format!(
        "phpstan-rs-lsp-startup-warm-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    // The first start fills the cache; the second is the one timed.
    let _ = time_to_initialize_reply(&dir);
    let elapsed = time_to_initialize_reply(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(elapsed < BUDGET, "warm start took {elapsed:?}");
}
