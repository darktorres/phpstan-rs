mod ast;
mod checker;
mod deps;
mod docblock;
mod lexer;
mod parser;
mod php_stubs;
mod project;
mod registry;
mod stubs;
mod token;
mod types;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use checker::Checker;
use lexer::Lexer;
use parser::Parser;
use registry::{FunctionSig, Registry};

const SAMPLE: &str = r#"<?php

namespace App;

class Greeter
{
    private string $prefix;

    public function __construct(string $prefix = "Hello")
    {
        $this->prefix = $prefix;
    }

    public function greet(string $name): string
    {
        if ($name === "") {
            return $this->prefix . ", stranger!";
        }
        return $this->prefix . ", " . $name . "!";
    }
}

function sum(array $numbers): int
{
    $total = 0;
    foreach ($numbers as $n) {
        $total += $n;
    }
    return $total;
}

$greeter = new Greeter();
$names = ["Ada", "Grace", "Alan"];
foreach ($names as $i => $name) {
    echo $greeter->greet($name), "\n";
}

echo sum([1, 2, 3]) ?? 0;
"#;

/// `phpstan-rs --check <file-or-dir>...` - lex+parse every .php file under
/// the given paths in a single process (no per-file fork/exec), split
/// across worker threads, report a pass/fail summary. Panics are the
/// parser's only error-reporting mechanism today, so each file's lex+parse
/// runs under catch_unwind with the default panic hook silenced for the
/// batch, to avoid both a crashed whole run and thousands of lines of
/// unwind noise (the hook is process-global, so this also covers panics
/// from the worker threads, not just the main one).
fn run_check(paths: &[String]) {
    let mut files = Vec::new();
    for p in paths {
        collect_php_files(Path::new(p), &mut files);
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let chunk_size = files.len().div_ceil(num_threads).max(1);
    let failures: Vec<(PathBuf, String)> = std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk_size)
            .map(|chunk| scope.spawn(|| check_chunk(chunk)))
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

fn check_chunk(chunk: &[PathBuf]) -> Vec<(PathBuf, String)> {
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
        let result = std::panic::catch_unwind(|| {
            let tokens = Lexer::new(&source).tokenize();
            Parser::new(tokens).parse_program();
        });
        if let Err(payload) = result {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".to_string());
            failures.push((path.clone(), msg));
        }
    }
    failures
}

fn collect_php_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_php_files(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|e| e == "php") {
        out.push(path.to_path_buf());
    }
}

/// `phpstan-rs --incremental-bench <dir> [<file-to-touch>]` - loads every
/// .php file under `dir` into a `Project` (a full load, timed), then
/// re-parses and re-checks just one file via `Project::update_file` (the
/// incremental path, also timed) and reports both, to make the whole
/// point of Phase 7's incremental-analysis work concrete: an edit to one
/// file costs O(that file), not O(the project). Picks the first file in
/// directory-walk order when none is named explicitly.
fn run_incremental_bench(args: &[String]) {
    let Some(dir) = args.first() else {
        eprintln!("usage: phpstan-rs --incremental-bench <dir> [<file-to-touch>]");
        return;
    };
    let mut files = Vec::new();
    collect_php_files(Path::new(dir), &mut files);
    if files.is_empty() {
        eprintln!("no .php files found under {dir}");
        return;
    }
    files.sort();

    let target = match args.get(1) {
        Some(f) => PathBuf::from(f),
        None => files[0].clone(),
    };
    if !files.contains(&target) {
        eprintln!("{} is not among the files under {dir}", target.display());
        return;
    }

    let sources: Vec<(PathBuf, String)> = files
        .iter()
        .map(|p| {
            let bytes = fs::read(p).unwrap_or_else(|e| panic!("failed to read {p:?}: {e}"));
            (p.clone(), String::from_utf8_lossy(&bytes).into_owned())
        })
        .collect();

    let full_load_start = std::time::Instant::now();
    let mut project = project::Project::new();
    for (path, source) in &sources {
        project.load_file(path.clone(), source);
    }
    project.finish_loading();
    let full_load = full_load_start.elapsed();

    let target_source = sources
        .iter()
        .find(|(p, _)| p == &target)
        .map(|(_, s)| s.clone())
        .unwrap();

    let incremental_start = std::time::Instant::now();
    let results = project.update_file(&target, &target_source);
    let incremental = incremental_start.elapsed();

    println!(
        "full load:         {} files in {:?}",
        project.file_count(),
        full_load
    );
    println!(
        "incremental update: {} file(s) re-checked ({}, plus {} dependent file(s)) in {:?}",
        results.len(),
        target.display(),
        results.len().saturating_sub(1),
        incremental
    );
    if incremental.as_nanos() > 0 {
        println!(
            "speedup:            {:.0}x",
            full_load.as_secs_f64() / incremental.as_secs_f64()
        );
    }
    let mut result_paths: Vec<&PathBuf> = results.keys().collect();
    result_paths.sort();
    for path in result_paths {
        let diagnostics = &results[path];
        println!("{}: {} diagnostic(s)", path.display(), diagnostics.len());
        for d in diagnostics {
            println!("  {}: {}", d.location, d.message);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.get(1).map(String::as_str) == Some("--check") {
        run_check(&args[2..]);
        return;
    }
    if args.get(1).map(String::as_str) == Some("--incremental-bench") {
        run_incremental_bench(&args[2..]);
        return;
    }

    let source = if let Some(path) = args.get(1) {
        // See check_chunk's comment on why this is a lossy conversion, not
        // a strict one.
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        println!("(no file given, parsing built-in sample)\n");
        SAMPLE.to_string()
    };

    let tokens = Lexer::new(&source).tokenize();
    println!("-- {} tokens --", tokens.len());

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();

    println!("-- {} top-level statements --", program.len());
    for stmt in &program {
        println!("{:#?}", stmt);
    }

    let mut registry = Registry::build(&program);
    // Generated (real php-src source) takes priority over the hand-curated
    // approximations - merge_stubs only fills in names not already present.
    registry.merge_stubs(php_stubs::generated_stubs());
    registry.merge_stubs(stubs::builtin_stubs());

    println!("\n-- resolved signatures (from source) --");
    let mut fn_names: Vec<&String> = registry.functions.keys().collect();
    fn_names.sort();
    for name in &fn_names {
        println!("{}", format_signature(name, &registry.functions[*name]));
    }

    let mut class_names: Vec<&String> = registry.classes.keys().collect();
    class_names.sort();
    for class_name in &class_names {
        let info = &registry.classes[*class_name];
        let mut method_names: Vec<&String> = info.methods.keys().collect();
        method_names.sort();
        for method_name in method_names {
            let qualified = format!("{class_name}::{method_name}");
            println!(
                "{}",
                format_signature(&qualified, &info.methods[method_name])
            );
        }
    }

    println!("\n-- class hierarchy --");
    for class_name in &class_names {
        let ancestors: Vec<&String> = class_names
            .iter()
            .filter(|other| *other != class_name && registry.is_subclass_of(class_name, other))
            .copied()
            .collect();
        if ancestors.is_empty() {
            println!("{class_name}: (no declared ancestors in this file)");
        } else {
            let joined: Vec<&str> = ancestors.iter().map(|s| s.as_str()).collect();
            println!("{class_name} <: {}", joined.join(", "));
        }
    }

    if let Some(sig) = registry.lookup_function("strlen") {
        println!("\n-- built-in stub example --");
        println!("{}", format_signature("strlen", sig));
    }

    let mut checker = Checker::new(&registry);
    checker.check_program(&program);
    let diagnostics = checker.into_diagnostics();

    println!("\n-- diagnostics --");
    if diagnostics.is_empty() {
        println!("(none)");
    } else {
        for d in &diagnostics {
            println!("{}: {}", d.location, d.message);
        }
    }
}

fn format_signature(name: &str, sig: &FunctionSig) -> String {
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|p| {
            let dots = if p.variadic { "..." } else { "" };
            format!("{} {dots}${}", p.ty, p.name)
        })
        .collect();
    format!("{name}({}): {}", params.join(", "), sig.return_type)
}
