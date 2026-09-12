//! Shared harness logic for every `phpstan-rs` fuzz target - factored out
//! so `lex_and_parse` and `structured_php` (or any future target) don't
//! each re-implement the same panic-handling/stack-size precautions.

use std::path::Path;

use phpstan_rs::project;

// ROADMAP.md phase 19 ("Fuzz testing"): the lexer/parser pipeline must
// never crash the process on adversarial input. `project::parse_file` is
// reused as-is (the same call every real entry point - `check.rs`,
// `check_project.rs`, `demo.rs` - already makes) rather than re-composing
// `Lexer`+`Parser` separately here.
//
// Two known, *deliberate* panics exist below `parse_file` and must not be
// misreported as crashes, or a fuzz target would trivially and
// permanently latch onto the first one:
//
//   1. `Parser::enter_expr`'s `MAX_EXPR_DEPTH` (1000) guard - panics on
//      purpose on pathologically deep nesting, documented at its call
//      site as "a normal, catchable error."
//   2. A real stack overflow *below* that guard - `check.rs`'s own doc
//      comment records that ~200-300 levels of nesting already overflows
//      a default thread stack in a debug build, well short of 1000, and
//      that even 8MiB wasn't enough; it uses a 64MiB worker stack so the
//      guard can trip safely before the real stack limit does. The
//      libFuzzer driver runs a target on an ordinary-stack thread, so the
//      same precaution is needed here.
//
// Any *other* panic is a real bug and must still be reported as a crash.
// Rather than relying on however libfuzzer-sys's own panic hook happens to
// be wired (uncertain, and irrelevant on a *spawned* worker thread - a
// resumed unwind there wouldn't reach libFuzzer's own crash-detection path
// at all), an unrecognized panic calls `std::process::abort()` directly:
// unconditional, immediate process termination, which libFuzzer always
// treats as a crash regardless of what installed the panic hook.
const KNOWN_DEPTH_GUARD_MESSAGE: &str = "expression nested too deeply";

const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Runs `project::parse_file` on `source` under the guards described
/// above. Call this from every fuzz target's own `fuzz_target!` closure
/// rather than calling `project::parse_file` directly.
pub fn guarded_parse(source: String) {
    let _ = std::thread::Builder::new()
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || run_one(&source))
        .expect("failed to spawn fuzz worker thread")
        .join();
}

fn run_one(source: &str) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let result = std::panic::catch_unwind(|| {
        let _ = project::parse_file(Path::new("fuzz.php"), source);
    });

    std::panic::set_hook(default_hook);

    if let Err(payload) = result {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("");
        if !message.contains(KNOWN_DEPTH_GUARD_MESSAGE) {
            eprintln!("unexpected panic in parse_file: {message}");
            std::process::abort();
        }
    }
}
