//! Shared harness logic for `structured_php` and `parse_type_string` -
//! `lex_parse`/`token_stream_parse` intentionally stay self-contained
//! (see their own doc comments for why no `catch_unwind` is needed
//! there: a worker-thread panic already reaches `libfuzzer-sys`'s own
//! crash reporting via `.join()`'s `Err` + `resume_unwind`).
//!
//! `structured_php` goes through `project::parse_file` rather than a raw
//! `Lexer`+`Parser` pair - a superset of `lex_parse`'s own pipeline
//! (`check-project`'s real per-file path: name/reference collection,
//! ignore-comment scanning, declaration building - not just `check`'s
//! lighter lex+parse), so it's worth the same depth-guard-swallowing
//! precaution described below, applied deliberately rather than
//! silently duplicating `lex_parse`'s simpler design.

use std::path::Path;

use phpstan_rs::{project, types};

// `project::parse_file` must never crash the process on adversarial
// input.
//
// One known, *deliberate* panic exists below it and must not be
// misreported as a crash, or a fuzz target would trivially and
// permanently latch onto it as "the" finding:
// `Parser::enter_expr`'s `MAX_EXPR_DEPTH` (1000) guard - panics on
// purpose on pathologically deep nesting, documented at its own call
// site as "a normal, catchable error." Swallowed here (rather than
// left to `libfuzzer-sys`'s own default panic handling, the way
// `lex_parse` does) because `structured_php`'s own generator can - and
// does - produce genuinely deep nesting through real recursive
// structure (`if`/`while`/`match` wrapping a nested fragment), not just
// incidentally via random bytes, making this guard easy to trip
// repeatedly across a long run; treating every trip as a fresh "crash"
// would mean libFuzzer halts the session on the very first one instead
// of continuing to look for real bugs.
//
// Any *other* panic is a real bug and must still be reported as a
// crash - `std::process::abort()` directly, rather than relying on
// however `libfuzzer-sys`'s own panic hook happens to be wired on a
// *spawned* worker thread (a resumed unwind there wouldn't reach its
// crash-detection path at all).
const KNOWN_DEPTH_GUARD_MESSAGE: &str = "expression nested too deeply";

// Matches `check.rs`'s own `WORKER_STACK_SIZE`: a default thread stack
// (~2-8MiB) overflows - uncatchably - well before `MAX_EXPR_DEPTH` (1000)
// or an unbounded-recursion bug in `parse_type_string` (see below) ever
// gets a chance to trip its own guard or reveal itself as a real,
// interesting finding.
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Runs `project::parse_file` on `source` under the guards described
/// above. Call this from `structured_php`'s own `fuzz_target!` closure.
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
        // `is_stub: false` - a generated source string is treated the
        // same way an ordinary, real (non-stub) user file would be.
        let _ = project::parse_file(Path::new("fuzz.php"), source, false);
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

/// Runs `types::parse_type_string` (the PHPDoc type-string parser,
/// `src/types/parse.rs`) on `source` on the same 64MiB worker stack as
/// `guarded_parse` - a PHPDoc type string can nest arbitrarily deep
/// (`array<array<array<...>>>`), and this parser has no `MAX_EXPR_DEPTH`-
/// style guard of its own (confirmed: no `panic!`/depth guard anywhere in
/// `src/types/parse.rs`, `conditional.rs`, or `templates.rs`), so an
/// ordinary default thread stack would report "just ran out of stack"
/// long before any real, interesting unbounded-recursion bug. Unlike
/// `guarded_parse`, nothing is swallowed here: there's no deliberate/
/// known panic to allowlist, so any panic caught is a genuine bug -
/// `Thread::join`'s own `Result` already reports one without needing a
/// manual `catch_unwind` here too.
pub fn guarded_parse_type(source: String) {
    let result = std::thread::Builder::new()
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || {
            let _ = types::parse_type_string(&source);
        })
        .expect("failed to spawn fuzz worker thread")
        .join();
    if result.is_err() {
        std::process::abort();
    }
}
