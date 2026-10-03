//! `front-end-dump`: what the lexer and the parser make of every PHP file
//! under some directories, as one line for each file, so a change to the
//! front end that must not change what it makes is checked by comparing
//! two such listings.
//!
//! Each file's line is its path and a hash of its dump: every token with
//! its span, every non-doc comment, every lexer warning, and the parse
//! (the tree, the facts, the comments, the lines holding code, the `goto`
//! labels) or the parse error. `--show <file>` prints one file's whole
//! dump, to see what a differing hash differs in.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syntax::lexer::Lexer;
use syntax::name::Interner;

fn php_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            php_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "php") {
            out.push(path);
        }
    }
}

/// What the front end makes of `source`, as text.
pub(crate) fn dump(source: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let (tokens, comments, warnings) = Lexer::from_bytes(source).tokenize_with_comments();
    for token in &tokens {
        let _ = writeln!(text, "token {token:?}");
    }
    for comment in &comments {
        let _ = writeln!(text, "comment {comment:?}");
    }
    for warning in &warnings {
        let _ = writeln!(text, "warning {warning:?}");
    }
    let interner = Arc::new(Interner::new());
    match syntax::parse(source, &interner) {
        Ok(parsed) => {
            let _ = writeln!(text, "parsed {parsed:?}");
        }
        Err(error) => {
            let _ = writeln!(text, "error {error:?}");
        }
    }
    text
}

fn digest(text: &str) -> String {
    let mut hasher = std::hash::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x} {}", hasher.finish(), text.len())
}

pub(crate) fn front_end_dump(args: &[String]) -> Result<(), String> {
    if let [flag, file] = args
        && flag == "--show"
    {
        let source = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        print!("{}", dump(&source));
        return Ok(());
    }
    let mut files = Vec::new();
    for dir in args {
        php_files(Path::new(dir), &mut files);
    }
    files.sort();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let chunk = files.len().div_ceil(threads).max(1);
    let lines: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = files
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|path| match std::fs::read(path) {
                            Ok(source) => {
                                format!("{}\t{}", path.display(), digest(&dump(&source)))
                            }
                            Err(e) => format!("{}\terror {e}", path.display()),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .collect()
    });
    for line in lines {
        println!("{line}");
    }
    Ok(())
}
