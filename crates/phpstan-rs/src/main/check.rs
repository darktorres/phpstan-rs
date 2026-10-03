use std::fs;
use std::path::PathBuf;

use phpstan_rs::project;
use rayon::prelude::*;

/// `phpstan-rs check <file-or-dir>...` - parse every .php file under the
/// given paths in a single process (no per-file fork/exec), split across
/// the worker pool, report a pass/fail summary. Each file's parse runs
/// under catch_unwind with the default panic hook silenced for the batch,
/// so a bug that panics fails that file rather than the run (the hook is
/// process-global, so this also covers the worker threads).
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
        // PHP source isn't required to be UTF-8 (symfony/cache's
        // ValueWrapper.php names a class with a single Latin-1 byte), so
        // the file is parsed as bytes.
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                failures.push((path.clone(), format!("read error: {e}")));
                continue;
            }
        };
        let result = std::panic::catch_unwind(|| {
            let parsed = syntax::parse(&bytes)?;
            for w in &parsed.warnings {
                eprintln!(
                    "warning: {}:{}:{}: {}",
                    path.display(),
                    parsed.line_index.line_col(w.span.start).line,
                    parsed.line_index.line_col(w.span.start).col,
                    w.message
                );
            }
            Ok::<_, syntax::parser::ParseError>(())
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
