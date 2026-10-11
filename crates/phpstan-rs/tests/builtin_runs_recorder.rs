//! The builtin-runs recorder's own tests (`tools/builtin-runs/tests/run.php`):
//! the table and its merge, the cache key, the selection, the sandbox's
//! command and a sandboxed worker, the rows pinned from the committed table,
//! and fixtures of user classes run under the recorder's rules.
//!
//! The script needs `pcntl`, and its fixtures and limits need FFI, so it runs
//! with `ffi.enable=1`. Without `php` or `pcntl` the test prints why and
//! passes. A sandboxed worker that bwrap cannot start is printed as skipped.
//!
//! `cargo test -p phpstan-rs --test builtin_runs_recorder`.

use std::path::Path;
use std::process::Command;

#[test]
fn builtin_runs_recorder() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/builtin-runs/tests/run.php");
    match Command::new("php")
        .args(["-r", "exit(function_exists('pcntl_fork') ? 0 : 3);"])
        .status()
    {
        Err(_) => {
            println!("builtin-runs recorder: no `php` on PATH, nothing run");
            return;
        }
        Ok(status) if !status.success() => {
            println!("builtin-runs recorder: this PHP has no pcntl, nothing run");
            return;
        }
        Ok(_) => {}
    }
    let output = Command::new("php")
        .args(["-d", "ffi.enable=1"])
        .arg(&script)
        .output()
        .expect("php runs the recorder's tests");
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "the recorder's tests failed");
}
