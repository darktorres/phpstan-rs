use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser as ClapParser, Subcommand};

use checker::Checker;
use lexer::Lexer;
use output::OutputFormat;
use parser::Parser;
use phpstan_rs::{
    baseline, checker, composer, config, lexer, lsp, output, parser, php_stubs, project, registry,
};
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
        project::collect_php_files(p, &mut files);
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let chunk_size = files.len().div_ceil(num_threads).max(1);
    // A bare `scope.spawn` uses Rust's default new-thread stack size
    // (~2MiB on Linux) - smaller than the ~8MiB a main thread gets, and
    // smaller than what the parser's own MAX_EXPR_DEPTH guard needs to
    // safely trip before physically exhausting the stack (confirmed
    // directly: a real, realistic ~200-300 levels of nested parentheses
    // - not an extreme adversarial input - crashed with an uncatchable
    // stack overflow here, well short of that guard's 1000-level limit,
    // while the same input only hit the guard's own intended, graceful
    // panic under check-project's larger main-thread stack). Matching
    // the main thread's own default budget here closes that gap.
    const WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;
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

/// `phpstan-rs incremental-bench <dir> [<file-to-touch>]` - loads every
/// .php file under `dir` into a `Project` (a full load, timed), then
/// re-parses and re-checks just one file via `Project::update_file` (the
/// incremental path, also timed) and reports both, to make the whole
/// point of Phase 7's incremental-analysis work concrete: an edit to one
/// file costs O(that file), not O(the project). Picks the first file in
/// directory-walk order when none is named explicitly.
fn run_incremental_bench(dir: &Path, file_to_touch: Option<&Path>) {
    let mut files = Vec::new();
    project::collect_php_files(dir, &mut files);
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
        project.load_file(path.clone(), source).unwrap();
    }
    project.finish_loading();
    let full_load = full_load_start.elapsed();

    let target_source = sources
        .iter()
        .find(|(p, _)| p == &target)
        .map(|(_, s)| s.clone())
        .unwrap();

    let incremental_start = std::time::Instant::now();
    let results = project.update_file(&target, &target_source).unwrap();
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
            println!("  {}", output::diagnostic_line(d));
            if let Some(tip) = &d.tip {
                println!("      💡 {tip}");
            }
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

/// Loads one file into `project`, tolerating a parse failure rather than
/// letting it crash the whole run. Returns `false` if the file couldn't
/// be read or failed to load, in which case it's simply absent from
/// `project` - same end state as never having attempted it, since the
/// only place a `ParseError` can originate in `Project::load_file`'s
/// pipeline is `parse()`, called before any registry/`self.files`
/// mutation.
fn try_load_file(project: &mut project::Project, path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let source = String::from_utf8_lossy(&bytes).into_owned();
    project.load_file(path.to_path_buf(), &source).is_ok()
}

// Phase 40's own new `phpstan_api_check_enabled` gate pushed this past
// clippy's default 7-argument threshold - every argument here is already
// its own independently-meaningful CLI-level setting (not a group crying
// out for a struct), so silencing the lint for this one, single-call-site
// wiring function reads clearer than a bag-of-fields options type would
// (mirrors this project's own existing `#[allow(clippy::type_complexity)]`
// precedent in `parser/classes.rs`: a real, deliberate exception, not a
// blanket suppression).
#[allow(clippy::too_many_arguments)]
#[hotpath::measure]
fn run_check_project(
    dirs: &[PathBuf],
    scan_dirs: &[PathBuf],
    scan_files: &[PathBuf],
    level: u8,
    strict_mixed_arrays: bool,
    generate_baseline_path: Option<&Path>,
    baseline_path: Option<&Path>,
    phpstan_api_check_enabled: bool,
    format: OutputFormat,
) {
    let mut analyze_files = Vec::new();
    for dir in dirs {
        project::collect_php_files(dir, &mut analyze_files);
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
        project::collect_php_files(dir, &mut context_files);
    }
    context_files.sort();
    context_files.dedup();

    let mut project = project::Project::new();
    project.set_strict_mixed_arrays(strict_mixed_arrays);
    project.set_phpstan_api_check_enabled(phpstan_api_check_enabled);

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

    let checked = analyze_files.len() - skipped_analyze;

    // Text is the only format carrying anything beyond raw diagnostics
    // (the "N more file(s) loaded for context"/"N file(s) skipped" prose)
    // - `json`/`checkstyle` exist so a CI pipeline can consume clean,
    // uninterrupted structured data on stdout, so they report only the
    // diagnostics themselves (`json`'s own `totals.files_checked` is the
    // one piece of that extra bookkeeping structured output still
    // carries - context/skip counts would need their own stable fields to
    // add without guessing at what a consumer wants, so they're left for
    // a real request rather than invented here).
    match format {
        OutputFormat::Text => {
            let mut paths: Vec<&PathBuf> = results.keys().collect();
            paths.sort();

            let mut total = 0usize;
            for path in paths {
                for d in &results[path] {
                    println!("{}:{}", path.display(), output::diagnostic_line(d));
                    if let Some(tip) = &d.tip {
                        println!("    💡 {tip}");
                    }
                    total += 1;
                }
            }
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
        OutputFormat::Json => {
            println!("{}", output::render_json(&results, checked));
        }
        OutputFormat::Checkstyle => {
            print!("{}", output::render_checkstyle(&results));
        }
    }
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
        /// Output format: `text` (default, human-readable), `json`, or
        /// `checkstyle` (XML) - the latter two for CI pipelines that want
        /// to consume diagnostics as structured data rather than parsing
        /// plain text. CLI-only, matching real PHPStan's own
        /// `--error-format`: unlike `level`/`baseline`/etc., this isn't a
        /// `config.rs` TOML key, since it's how one particular invocation
        /// wants its output shaped, not a standing project setting.
        #[arg(long)]
        format: Option<OutputFormat>,
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

#[hotpath::main]
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
            format,
        } => {
            // Finding 8: a config file's own `paths`/`baseline`/
            // `scan_paths` are conventionally relative to the config file
            // itself (matching every comparable format - .gitignore,
            // tsconfig.json, real PHPStan's own neon includes - and this
            // project's own module-doc example), not to whatever
            // directory the invoking shell happens to be in. Captured
            // here, before `config` is shadowed by the loaded
            // `config::Config` below, since the parsed struct itself
            // doesn't carry its own source path.
            let config_dir = config
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf);
            let resolve_against_config = |p: PathBuf| match &config_dir {
                Some(dir) => dir.join(p),
                None => p,
            };

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
                config
                    .paths
                    .unwrap_or_default()
                    .into_iter()
                    .map(resolve_against_config)
                    .collect()
            };
            if dirs.is_empty() {
                eprintln!("no paths given on the command line or in the config file");
                std::process::exit(1);
            }
            let level = level.or(config.level).unwrap_or(MAX_LEVEL);
            let strict_mixed_arrays =
                strict_mixed_arrays || config.strict_mixed_arrays.unwrap_or(false);
            let baseline = baseline.or(config.baseline.map(resolve_against_config));

            let mut scan_dirs = if !scan_dirs.is_empty() {
                scan_dirs
            } else {
                config
                    .scan_paths
                    .unwrap_or_default()
                    .into_iter()
                    .map(resolve_against_config)
                    .collect()
            };
            let no_composer = no_composer || config.no_composer.unwrap_or(false);

            let mut scan_files = Vec::new();
            // Phase 40: gated the same way as the vendor-context load just
            // above (both need the project's own `composer.json`, and
            // `--no-composer` already means "don't consult Composer at
            // all for this run") - a project with no real
            // `phpstan/phpstan`/`phpstan/extension-installer` dependency
            // has no backward-compatibility promise to check anything
            // against.
            let mut phpstan_api_check_enabled = false;
            if !no_composer {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let vendor_context = composer::discover_vendor_context(&cwd);
                scan_dirs.extend(vendor_context.dirs);
                scan_files.extend(vendor_context.files);
                phpstan_api_check_enabled = composer::detect_phpstan_dependency(&cwd);
            }

            run_check_project(
                &dirs,
                &scan_dirs,
                &scan_files,
                level,
                strict_mixed_arrays,
                generate_baseline.as_deref(),
                baseline.as_deref(),
                phpstan_api_check_enabled,
                format.unwrap_or_default(),
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
    let program = parser.parse_program().unwrap();

    println!("-- {} top-level statements --", program.len());
    for stmt in &program {
        println!("{:#?}", stmt);
    }

    let mut registry = Registry::build(&program);
    // Generated (real php-src source) takes priority over the hand-curated
    // approximations - merge_stubs only fills in names not already present.
    registry.merge_stubs(php_stubs::generated_stubs());
    registry.merge_stub_classes(php_stubs::generated_stub_classes());

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
            println!("{}", output::diagnostic_line(d));
            if let Some(tip) = &d.tip {
                println!("    💡 {tip}");
            }
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
#[path = "main/tests.rs"]
mod tests;
