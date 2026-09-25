//! Lexing and parsing throughput over the vendored phpstorm-stubs (real
//! PHP, present wherever the repository is checked out). Sources are read
//! once, outside the measured closures.

use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use syntax::lexer::Lexer;
use syntax::parser::Parser;

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

fn sources() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/phpstorm-stubs");
    let mut files = Vec::new();
    php_files(&dir, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no PHP files under {}; is the vendor directory checked out?",
        dir.display()
    );
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

fn bench(c: &mut Criterion) {
    let sources = sources();
    c.bench_function("lex phpstorm-stubs", |b| {
        b.iter(|| {
            for source in &sources {
                std::hint::black_box(Lexer::new(source).tokenize_with_spans());
            }
        })
    });
    c.bench_function("lex and parse phpstorm-stubs", |b| {
        b.iter(|| {
            for source in &sources {
                let tokens = Lexer::new(source).tokenize_with_spans();
                let _ = std::hint::black_box(Parser::new(tokens).parse_program());
            }
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
