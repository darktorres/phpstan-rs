use std::fs;
use std::path::PathBuf;

use lexer::Lexer;
use parser::Parser;
use phpstan_rs::{lexer, parser, project};

/// `phpstan-rs check <file-or-dir>...` - lex+parse every .php file under
/// the given paths in a single process (no per-file fork/exec), split
/// across worker threads, report a pass/fail summary. Panics are the
/// parser's only error-reporting mechanism today, so each file's lex+parse
/// runs under catch_unwind with the default panic hook silenced for the
/// batch, to avoid both a crashed whole run and thousands of lines of
/// unwind noise (the hook is process-global, so this also covers panics
/// from the worker threads, not just the main one).
pub(crate) fn run_check(paths: &[PathBuf]) {
    let mut files = Vec::new();
    for p in paths {
        project::collect_php_files(p, &mut files);
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let chunk_size = files.len().div_ceil(num_threads).max(1);
    // A bare `scope.spawn` uses Rust's default new-thread stack size
    // (~2MiB on Linux) - smaller than what the parser's own
    // MAX_EXPR_DEPTH guard (1000 levels) needs to safely trip before
    // physically exhausting the stack, and a debug build's much larger
    // per-frame stack usage through the parser's ~20-function-deep
    // precedence-climbing chain needs considerably more still. 64MiB
    // (this project's own `RUST_MIN_STACK` value elsewhere) gives real
    // headroom for both build profiles; `RUST_MIN_STACK` itself only
    // affects a *default*, unsized `thread::spawn`/`Builder`, never a
    // smaller floor on an explicit `.stack_size()` call like this one,
    // so it has to be set here too.
    const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;
    let failures: Vec<(PathBuf, String)> = std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk_size)
            .map(|chunk| {
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK_SIZE)
                    .spawn_scoped(scope, || check_chunk(chunk))
                    .expect("failed to spawn worker thread")
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });

    std::panic::set_hook(default_hook);

    println!("{} files checked, {} failed", files.len(), failures.len());
    for (path, msg) in &failures {
        println!("{}: {}", path.display(), msg);
    }
}

pub(crate) fn check_chunk(chunk: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut failures = Vec::new();
    for path in chunk {
        // PHP source isn't required to be UTF-8 - identifiers and string
        // literals are just opaque byte sequences to the real engine, so
        // e.g. a class literally named a single Latin-1 byte (real,
        // observed in the wild: symfony/cache's ValueWrapper.php) is valid
        // PHP. read_to_string's strict UTF-8 requirement would reject such
        // a file outright; a lossy conversion parses it instead, at the
        // cost of that one identifier's exact byte content.
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                failures.push((path.clone(), format!("read error: {e}")));
                continue;
            }
        };
        let source = String::from_utf8_lossy(&bytes).into_owned();
        // The parser itself no longer panics on ordinary malformed syntax
        // (it returns a ParseError) - the catch_unwind here now only
        // guards against the lexer's own, separate, still-panic-based
        // error handling (out of scope for this parser-only conversion).
        let result = std::panic::catch_unwind(|| {
            let tokens = Lexer::new(&source).tokenize_with_spans();
            Parser::new(tokens).parse_program()
        });
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => failures.push((path.clone(), e.to_string())),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown panic".to_string());
                failures.push((path.clone(), msg));
            }
        }
    }
    failures
}
