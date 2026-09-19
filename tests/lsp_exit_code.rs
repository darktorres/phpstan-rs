//! Subprocess-level regression coverage for the LSP `exit` notification's
//! process-exit-code contract: 0 if a `shutdown` request preceded it, 1
//! otherwise. This is the one behavior no in-process `run_loop` unit
//! test can prove, since it depends on `main.rs`'s own `std::process::
//! exit` wiring actually reaching the real OS exit status - follows
//! `tests/cli.rs`'s own established pattern (spawn the real compiled
//! binary via `CARGO_BIN_EXE_`, assert on real process output) rather
//! than refactor working CLI-glue code just for in-process testability.

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_phpstan-rs"))
}

fn framed(json: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{json}", json.len()).into_bytes()
}

#[test]
fn exit_after_shutdown_exits_with_code_0() {
    let mut child = bin()
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(&framed(
            r#"{"jsonrpc":"2.0","id":1,"method":"shutdown","params":{}}"#,
        ))
        .unwrap();
    stdin
        .write_all(&framed(r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#))
        .unwrap();
    drop(stdin);

    let status = child.wait_with_output().unwrap().status;
    assert_eq!(status.code(), Some(0));
}

#[test]
fn bare_exit_with_no_prior_shutdown_exits_with_code_1() {
    let mut child = bin()
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(&framed(r#"{"jsonrpc":"2.0","method":"exit","params":{}}"#))
        .unwrap();
    drop(stdin);

    let status = child.wait_with_output().unwrap().status;
    assert_eq!(status.code(), Some(1));
}
