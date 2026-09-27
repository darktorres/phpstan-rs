use std::fs;
use std::path::PathBuf;

use lexer::Lexer;
use parser::Parser;
use phpstan_rs::project;
use rayon::prelude::*;
use syntax::{lexer, parser};

/// `phpstan-rs check <file-or-dir>...` - lex+parse every .php file under
/// the given paths in a single process (no per-file fork/exec), split
/// across the worker pool, report a pass/fail summary. Panics are the
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

    // One chunk per worker, on threads with the stack the parser needs.
    let pool = project::worker_pool();
    let chunk_size = files.len().div_ceil(pool.current_num_threads()).max(1);
    let failures: Vec<(PathBuf, String)> = pool.install(|| {
        files
            .par_chunks(chunk_size)
            .flat_map_iter(check_chunk)
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
            let (tokens, _, warnings) = Lexer::new(&source).tokenize_with_comments();
            for w in &warnings {
                eprintln!(
                    "warning: {}:{}:{}: {}",
                    path.display(),
                    w.span.start.line,
                    w.span.start.col,
                    w.message
                );
            }
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
