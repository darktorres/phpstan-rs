//! `phpstan-rs --version`/`-V` (VSCode plan item 4.2's own load-bearing
//! prerequisite: CI's per-target smoke check and the VSCode extension's
//! downloaded-binary version match both depend on this actually working).
//! A real integration test (this file, under `tests/`, matching `cli.rs`'s
//! own convention) - `env!("CARGO_BIN_EXE_phpstan-rs")` is only defined
//! for a genuine Cargo integration-test target, not for `#[cfg(test)]`
//! unit tests compiled into the `phpstan-rs` binary itself (confirmed:
//! attempting this from `src/main/tests.rs`'s own `mod` list fails to
//! compile with "environment variable `CARGO_BIN_EXE_phpstan-rs` not
//! defined at compile time").

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_phpstan-rs"))
}

fn expected() -> String {
    format!("phpstan-rs {}\n", env!("CARGO_PKG_VERSION"))
}

#[test]
fn dash_dash_version_prints_the_exact_expected_shape_and_exits_zero() {
    let output = bin().arg("--version").output().unwrap();
    assert!(output.status.success(), "got: {output:?}");
    assert!(output.stderr.is_empty(), "got: {output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout, expected(), "got: {stdout:?}");
}

#[test]
fn dash_v_behaves_identically_to_dash_dash_version() {
    let output = bin().arg("-V").output().unwrap();
    assert!(output.status.success(), "got: {output:?}");
    assert!(output.stderr.is_empty(), "got: {output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout, expected(), "got: {stdout:?}");
}
