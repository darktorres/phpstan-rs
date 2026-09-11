use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use output::OutputFormat;
use phpstan_rs::{
    baseline, checker, composer, config, ignore_errors, output, php_version, project,
};

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
    check_uninitialized_properties: bool,
    check_internal_usage: bool,
    check_deprecated_usage: bool,
    check_empty_method_bodies: bool,
    check_missing_checked_exceptions: bool,
    check_strict_printf_placeholder_types: bool,
    check_array_filter_falsy_noop: bool,
    check_too_wide_throw_type_in_protected_and_public_methods: bool,
    php_version: Option<php_version::PhpVersion>,
    generate_baseline_path: Option<&Path>,
    baseline_path: Option<&Path>,
    ignore_error_entries: &[ignore_errors::IgnoreErrorEntry],
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
    project.set_check_uninitialized_properties(check_uninitialized_properties);
    project.set_check_internal_usage(check_internal_usage);
    project.set_check_deprecated_usage(check_deprecated_usage);
    project.set_check_empty_method_bodies(check_empty_method_bodies);
    project.set_check_missing_checked_exceptions(check_missing_checked_exceptions);
    project.set_check_strict_printf_placeholder_types(check_strict_printf_placeholder_types);
    project.set_check_array_filter_falsy_noop(check_array_filter_falsy_noop);
    project.set_check_too_wide_throw_type_in_protected_and_public_methods(
        check_too_wide_throw_type_in_protected_and_public_methods,
    );
    project.set_php_version(php_version);
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
    ignore_errors::filter(&mut results, ignore_error_entries);

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

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_check_project_command(
    dirs: Vec<PathBuf>,
    level: Option<u8>,
    strict_mixed_arrays: bool,
    check_uninitialized_properties: bool,
    check_internal_usage: bool,
    check_deprecated_usage: bool,
    check_empty_method_bodies: bool,
    check_missing_checked_exceptions: bool,
    check_strict_printf_placeholder_types: bool,
    check_array_filter_falsy_noop: bool,
    check_too_wide_throw_type_in_protected_and_public_methods: bool,
    php_version: Option<String>,
    generate_baseline: Option<PathBuf>,
    baseline: Option<PathBuf>,
    scan_dirs: Vec<PathBuf>,
    no_composer: bool,
    config: Option<PathBuf>,
    format: Option<OutputFormat>,
) {
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
    let strict_mixed_arrays = strict_mixed_arrays || config.strict_mixed_arrays.unwrap_or(false);
    let check_uninitialized_properties =
        check_uninitialized_properties || config.check_uninitialized_properties.unwrap_or(false);
    let check_internal_usage = check_internal_usage || config.check_internal_usage.unwrap_or(false);
    let check_deprecated_usage =
        check_deprecated_usage || config.check_deprecated_usage.unwrap_or(false);
    let check_empty_method_bodies =
        check_empty_method_bodies || config.check_empty_method_bodies.unwrap_or(false);
    let check_missing_checked_exceptions = check_missing_checked_exceptions
        || config.check_missing_checked_exceptions.unwrap_or(false);
    let check_strict_printf_placeholder_types = check_strict_printf_placeholder_types
        || config
            .check_strict_printf_placeholder_types
            .unwrap_or(false);
    let check_array_filter_falsy_noop =
        check_array_filter_falsy_noop || config.check_array_filter_falsy_noop.unwrap_or(false);
    let check_too_wide_throw_type_in_protected_and_public_methods =
        check_too_wide_throw_type_in_protected_and_public_methods
            || config
                .check_too_wide_throw_type_in_protected_and_public_methods
                .unwrap_or(false);
    let php_version = match php_version.or(config.php_version) {
        Some(s) => match php_version::parse_php_version(&s) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let baseline = baseline.or(config.baseline.map(resolve_against_config));
    let ignore_error_entries = config.ignore_errors.unwrap_or_default();

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
        check_uninitialized_properties,
        check_internal_usage,
        check_deprecated_usage,
        check_empty_method_bodies,
        check_missing_checked_exceptions,
        check_strict_printf_placeholder_types,
        check_array_filter_falsy_noop,
        check_too_wide_throw_type_in_protected_and_public_methods,
        php_version,
        generate_baseline.as_deref(),
        baseline.as_deref(),
        &ignore_error_entries,
        phpstan_api_check_enabled,
        format.unwrap_or_default(),
    )
}
