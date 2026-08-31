mod ast;
mod baseline;
mod checker;
mod composer;
mod config;
mod deps;
mod docblock;
mod lexer;
mod lsp;
mod parser;
mod php_stubs;
mod project;
mod registry;
mod stubs;
mod token;
mod types;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser as ClapParser, Subcommand};

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

/// `phpstan-rs check <file-or-dir>...` - lex+parse every .php file under
/// the given paths in a single process (no per-file fork/exec), split
/// across worker threads, report a pass/fail summary. Panics are the
/// parser's only error-reporting mechanism today, so each file's lex+parse
/// runs under catch_unwind with the default panic hook silenced for the
/// batch, to avoid both a crashed whole run and thousands of lines of
/// unwind noise (the hook is process-global, so this also covers panics
/// from the worker threads, not just the main one).
fn run_check(paths: &[PathBuf]) {
    let mut files = Vec::new();
    for p in paths {
        collect_php_files(p, &mut files);
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
            let tokens = Lexer::new(&source).tokenize_with_spans();
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

/// `phpstan-rs incremental-bench <dir> [<file-to-touch>]` - loads every
/// .php file under `dir` into a `Project` (a full load, timed), then
/// re-parses and re-checks just one file via `Project::update_file` (the
/// incremental path, also timed) and reports both, to make the whole
/// point of Phase 7's incremental-analysis work concrete: an edit to one
/// file costs O(that file), not O(the project). Picks the first file in
/// directory-walk order when none is named explicitly.
fn run_incremental_bench(dir: &Path, file_to_touch: Option<&Path>) {
    let mut files = Vec::new();
    collect_php_files(dir, &mut files);
    if files.is_empty() {
        eprintln!("no .php files found under {}", dir.display());
        return;
    }
    files.sort();

    let target = match file_to_touch {
        Some(f) => f.to_path_buf(),
        None => files[0].clone(),
    };
    if !files.contains(&target) {
        eprintln!(
            "{} is not among the files under {}",
            target.display(),
            dir.display()
        );
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
            println!(
                "  {}:{}: {}: {}",
                d.span.start.line, d.span.start.col, d.location, d.message
            );
        }
    }
}

/// `phpstan-rs check-project <dir>` - loads every .php file under `dir`
/// into a `Project` and runs the full checker over all of them at once
/// (`Project::check_all`), printing every diagnostic. Unlike `check`
/// (parse-only, no semantic checking) or `incremental-bench` (one file
/// plus its dependents), this is the whole-corpus semantic-diagnostics
/// view - the tool needed to spot-check new diagnostic rules for false
/// positives against a real codebase before trusting them.
/// The highest level this project's own `Diagnostic::level` tagging uses
/// (see `checker::Diagnostic`'s own doc comment) - `--level` left unset
/// means "report everything up to and including this", preserving this
/// tool's original always-report-everything behavior rather than
/// silently adopting real PHPStan's own stricter level-0 default.
const MAX_LEVEL: u8 = 6;

/// Filters every file's diagnostics down to `level` and below, in place -
/// shared by both the normal report path and `--generate-baseline` (a
/// baseline should record exactly what a normal run at this level would
/// currently show, not every diagnostic regardless of level).
fn apply_level_filter(
    mut results: HashMap<PathBuf, Vec<checker::Diagnostic>>,
    level: u8,
) -> HashMap<PathBuf, Vec<checker::Diagnostic>> {
    for diagnostics in results.values_mut() {
        diagnostics.retain(|d| d.level <= level);
    }
    results
}

/// Loads one file into `project`, tolerating a lex/parse panic (this
/// parser's only error-reporting mechanism today - see
/// `check_chunk`'s own doc comment) rather than letting it crash the
/// whole run. Returns `false` if the file couldn't be read or failed to
/// load, in which case it's simply absent from `project` - same
/// end state as never having attempted it, since the only place a panic
/// can originate in `Project::load_file`'s pipeline is `parse()`,
/// called before any registry/`self.files` mutation.
fn try_load_file(project: &mut project::Project, path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let source = String::from_utf8_lossy(&bytes).into_owned();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        project.load_file(path.to_path_buf(), &source);
    }))
    .is_ok()
}

fn run_check_project(
    dirs: &[PathBuf],
    scan_dirs: &[PathBuf],
    scan_files: &[PathBuf],
    level: u8,
    strict_mixed_arrays: bool,
    generate_baseline_path: Option<&Path>,
    baseline_path: Option<&Path>,
) {
    let mut analyze_files = Vec::new();
    for dir in dirs {
        collect_php_files(dir, &mut analyze_files);
    }
    if analyze_files.is_empty() {
        let dirs: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        eprintln!("no .php files found under {}", dirs.join(", "));
        return;
    }
    // Deterministic load order matters now in a way it didn't for
    // --check (parse-only): a real corpus can genuinely declare the same
    // fully-qualified class name twice (verified: multiple vendored
    // copies of nikic/php-parser bundled by different tools, none of
    // them php-scoper-prefixed relative to each other) - readdir order
    // is filesystem-dependent, so without sorting, which declaration
    // "wins" the registry slot (and therefore which one's real methods
    // are visible) could silently vary between runs on different
    // machines or filesystems.
    analyze_files.sort();

    let mut context_files: Vec<PathBuf> = scan_files.to_vec();
    for dir in scan_dirs {
        collect_php_files(dir, &mut context_files);
    }
    context_files.sort();
    context_files.dedup();

    let mut project = project::Project::new();
    project.set_strict_mixed_arrays(strict_mixed_arrays);

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    // Context files load first: `Registry::index` resolves a
    // fully-qualified-name collision with plain last-write-wins (a bare
    // `HashMap::insert`, no existence check, no diagnostic), and while
    // the vendor-only filter in `composer::discover_vendor_context`
    // keeps a project's own first-party directories out of the context
    // set, it can't prevent a genuinely different vendor package
    // declaring a class under the same name as something in the user's
    // own code (PHP allows unnamespaced global classes). Loading
    // context first guarantees the analyzed declaration - the one
    // actually being reported on - always wins the registry slot on any
    // such collision.
    let skipped_context = context_files
        .iter()
        .filter(|path| !try_load_file(&mut project, path))
        .count();
    let skipped_analyze = analyze_files
        .iter()
        .filter(|path| !try_load_file(&mut project, path))
        .count();
    std::panic::set_hook(default_hook);
    project.finish_loading();

    let results: HashMap<PathBuf, Vec<checker::Diagnostic>> = analyze_files
        .iter()
        .map(|path| (path.clone(), project.check_file(path)))
        .collect();
    let mut results = apply_level_filter(results, level);

    if let Some(generate_baseline_path) = generate_baseline_path {
        match baseline::generate(generate_baseline_path, &results) {
            Ok(count) => println!(
                "baseline written to {}: {count} entry(s)",
                generate_baseline_path.display()
            ),
            Err(e) => eprintln!(
                "failed to write baseline to {}: {e}",
                generate_baseline_path.display()
            ),
        }
        return;
    }

    if let Some(baseline_path) = baseline_path {
        match baseline::Baseline::load(baseline_path) {
            Ok(baseline) => baseline::filter(&mut results, &baseline),
            Err(e) => eprintln!("failed to load baseline {}: {e}", baseline_path.display()),
        }
    }

    let mut paths: Vec<&PathBuf> = results.keys().collect();
    paths.sort();

    let mut total = 0usize;
    for path in paths {
        for d in &results[path] {
            println!(
                "{}:{}:{}: {}: {}",
                path.display(),
                d.span.start.line,
                d.span.start.col,
                d.location,
                d.message
            );
            total += 1;
        }
    }
    let checked = analyze_files.len() - skipped_analyze;
    let mut summary = format!("\n{checked} file(s) checked, {total} diagnostic(s)");
    let mut extras = Vec::new();
    let context_loaded = context_files.len() - skipped_context;
    if context_loaded > 0 {
        extras.push(format!("{context_loaded} more file(s) loaded for context"));
    }
    let skipped_total = skipped_analyze + skipped_context;
    if skipped_total > 0 {
        extras.push(format!("{skipped_total} file(s) skipped: parse error"));
    }
    if !extras.is_empty() {
        summary.push_str(&format!(" ({})", extras.join(", ")));
    }
    println!("{summary}");
}

#[derive(ClapParser)]
#[command(subcommand_required = true, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lex+parse every .php file under the given paths, reporting a
    /// pass/fail summary.
    Check { paths: Vec<PathBuf> },
    /// Load every .php file under the given paths and run the full
    /// checker over all of them, reporting every diagnostic.
    CheckProject {
        // Not `required = true`: a TOML config file's own `paths` can
        // supply these instead (see `run_check_project`'s resolution
        // logic) - the "must know what to check" requirement is
        // enforced there, once the config (if any) has been consulted,
        // not statically by clap.
        dirs: Vec<PathBuf>,
        /// Report up to and including this level (see
        /// checker::Diagnostic's own doc comment) - unset means
        /// "report everything".
        #[arg(long)]
        level: Option<u8>,
        #[arg(long)]
        strict_mixed_arrays: bool,
        #[arg(long)]
        generate_baseline: Option<PathBuf>,
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Extra directories loaded for type-resolution context only -
        /// never analyzed or reported on. Repeatable.
        #[arg(long = "scan-dir")]
        scan_dirs: Vec<PathBuf>,
        /// Disables the automatic Composer-autoloader-based `vendor/`
        /// context load (see `composer::discover_vendor_context`).
        /// `--scan-dir` still works.
        #[arg(long)]
        no_composer: bool,
        /// TOML config file (see src/config.rs) supplying defaults for
        /// paths/level/baseline/strict_mixed_arrays/scan_paths/
        /// no_composer - any flag above, when given, overrides the same
        /// key in the config.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Loads every .php file under `dir` into a Project (timed), then
    /// re-parses/re-checks just one file via the incremental path (also
    /// timed), reporting both.
    IncrementalBench {
        dir: PathBuf,
        file_to_touch: Option<PathBuf>,
    },
    /// Runs the LSP server over stdio.
    Lsp,
    /// Parses the given file (or a built-in sample with none given) and
    /// dumps tokens/AST/resolved signatures/diagnostics - a debug tool,
    /// not the normal way to use this checker.
    Demo { file: Option<PathBuf> },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { paths } => run_check(&paths),
        Command::CheckProject {
            dirs,
            level,
            strict_mixed_arrays,
            generate_baseline,
            baseline,
            scan_dirs,
            no_composer,
            config,
        } => {
            let config = match config {
                Some(path) => match config::Config::load(&path) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                },
                None => None,
            };
            let config = config.unwrap_or_default();

            let dirs = if !dirs.is_empty() {
                dirs
            } else {
                config.paths.unwrap_or_default()
            };
            if dirs.is_empty() {
                eprintln!("no paths given on the command line or in the config file");
                std::process::exit(1);
            }
            let level = level.or(config.level).unwrap_or(MAX_LEVEL);
            let strict_mixed_arrays =
                strict_mixed_arrays || config.strict_mixed_arrays.unwrap_or(false);
            let baseline = baseline.or(config.baseline);

            let mut scan_dirs = if !scan_dirs.is_empty() {
                scan_dirs
            } else {
                config.scan_paths.unwrap_or_default()
            };
            let no_composer = no_composer || config.no_composer.unwrap_or(false);

            let mut scan_files = Vec::new();
            if !no_composer {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let vendor_context = composer::discover_vendor_context(&cwd);
                scan_dirs.extend(vendor_context.dirs);
                scan_files.extend(vendor_context.files);
            }

            run_check_project(
                &dirs,
                &scan_dirs,
                &scan_files,
                level,
                strict_mixed_arrays,
                generate_baseline.as_deref(),
                baseline.as_deref(),
            )
        }
        Command::IncrementalBench { dir, file_to_touch } => {
            run_incremental_bench(&dir, file_to_touch.as_deref())
        }
        Command::Lsp => lsp::run(),
        Command::Demo { file } => run_demo(file.as_deref()),
    }
}

fn run_demo(file: Option<&Path>) {
    let source = if let Some(path) = file {
        // See check_chunk's comment on why this is a lossy conversion, not
        // a strict one.
        let bytes =
            fs::read(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        println!("(no file given, parsing built-in sample)\n");
        SAMPLE.to_string()
    };

    let tokens = Lexer::new(&source).tokenize_with_spans();
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
    registry.merge_stub_classes(php_stubs::generated_stub_classes());
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
            println!(
                "{}:{}: {}: {}",
                d.span.start.line, d.span.start.col, d.location, d.message
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    // main.rs had 0% test coverage before this - collect_php_files,
    // check_chunk, and format_signature are all real, deterministic
    // logic that doesn't touch stdout, so they're tested directly here.
    // run_check/run_check_project/run_incremental_bench and the default
    // no-args mode all print straight to stdout instead of returning
    // anything - those are covered by tests/cli.rs instead, which spawns
    // the actual compiled binary and checks its real output, rather than
    // refactoring working CLI-glue code just to make it unit-testable in
    // process.

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "phpstan_rs_main_test_{label}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn write(&self, rel: &str, contents: &str) -> PathBuf {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, contents).unwrap();
            path
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn collect_php_files_recurses_into_subdirectories_and_skips_non_php() {
        let dir = TempDir::new("collect");
        dir.write("a.php", "<?php");
        dir.write("nested/b.php", "<?php");
        dir.write("notes.txt", "not php");

        let mut files = Vec::new();
        collect_php_files(&dir.0, &mut files);
        files.sort();

        assert_eq!(files, vec![dir.0.join("a.php"), dir.0.join("nested/b.php")]);
    }

    #[test]
    fn check_chunk_reports_a_read_error_for_a_missing_file_without_panicking() {
        let dir = TempDir::new("missing");
        let missing = dir.0.join("does_not_exist.php");
        let failures = check_chunk(std::slice::from_ref(&missing));
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, missing);
        assert!(failures[0].1.contains("read error"));
    }

    #[test]
    fn check_chunk_reports_no_failure_for_valid_php() {
        let dir = TempDir::new("valid");
        let path = dir.write("a.php", "<?php function f(): int { return 1; }");
        assert!(check_chunk(&[path]).is_empty());
    }

    #[test]
    fn format_signature_includes_params_variadic_marker_and_return_type() {
        let sig = FunctionSig {
            params: vec![
                registry::ParamSig {
                    name: "x".to_string(),
                    ty: types::Type::Int,
                    variadic: false,
                    has_default: false,
                    by_ref: false,
                },
                registry::ParamSig {
                    name: "rest".to_string(),
                    ty: types::Type::String,
                    variadic: true,
                    has_default: false,
                    by_ref: false,
                },
            ],
            return_type: types::Type::Bool,
        };
        assert_eq!(
            format_signature("f", &sig),
            "f(int $x, string ...$rest): bool"
        );
    }
}
