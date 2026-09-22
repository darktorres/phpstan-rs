use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use output::OutputFormat;
use phpstan_rs::{
    baseline, checker, composer, config, doctrine_functions, ignore_errors, output, php_version,
    project, restricted_symbols, sql_reflection_config, sql_reflection_reflector,
    sql_reflection_schema, sql_reflection_schema_drift, token::Span,
};
use rayon::prelude::*;

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

/// Reads and parses one file, tolerating a read/parse failure by
/// returning `None` rather than letting it crash the whole run - same end
/// state as never having attempted it. Pure: touches no `Project`/shared
/// registry state at all, so calling this for many files concurrently
/// (the point of splitting it out from `Project::load_file`) is safe -
/// deliberately unaware of `analyzed`/context-vs-first-party status, which
/// only matters once a parsed file reaches the sequential insert loop
/// below (`Project::insert_parsed_file`'s own `analyzed` parameter).
/// `catch_unwind` guards the lexer's own, separate, still-panic-based
/// error handling (the parser itself no longer panics on ordinary
/// malformed syntax) - the identical precaution `check.rs`'s own lex+parse
/// worker threads already take, needed here for the same reason.
fn read_and_parse_file(path: &Path, is_stub: bool) -> Option<(String, project::ParsedFile)> {
    let bytes = fs::read(path).ok()?;
    let source = String::from_utf8_lossy(&bytes).into_owned();
    let parsed = std::panic::catch_unwind(|| project::parse_file(path, &source, is_stub))
        .ok()?
        .ok()?;
    Some((source, parsed))
}

/// Collects both file lists (`analyze_files`, sorted; `context_files`,
/// sorted+deduped, seeded with `scan_files`) in one measured call, so the
/// real cost of the whole single-threaded directory walk - entirely
/// before this module's own rayon pool is built - shows up as a single,
/// correctly-attributed number in the hotpath report (`collect_php_files`
/// itself is deliberately left unmeasured - see its own doc comment for
/// why a self-recursive function's own aggregate would be misleading).
/// Small enough on a real monorepo that parallelizing isn't worth it next
/// to the already-parallel parse/check phases, though collection order
/// is discarded immediately by the `sort()`/`dedup()` calls right here,
/// so it would be safe to if this ever became the dominant cost on a
/// corpus with far more directories than files.
#[hotpath::measure]
fn collect_analyze_and_context_files(
    dirs: &[PathBuf],
    scan_dirs: &[PathBuf],
    scan_files: &[PathBuf],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut analyze_files = Vec::new();
    for dir in dirs {
        project::collect_php_files(dir, &mut analyze_files);
    }
    analyze_files.sort();

    let mut context_files: Vec<PathBuf> = scan_files.to_vec();
    for dir in scan_dirs {
        project::collect_php_files(dir, &mut context_files);
    }
    context_files.sort();
    context_files.dedup();

    (analyze_files, context_files)
}

// Past clippy's default 7-argument threshold - every argument here is
// already its own independently-meaningful CLI-level setting (not a
// group crying out for a struct), so silencing the lint for this one,
// single-call-site
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
    check_function_name_case: bool,
    check_deprecated_usage: bool,
    check_empty_method_bodies: bool,
    check_missing_checked_exceptions: bool,
    check_strict_printf_placeholder_types: bool,
    check_too_wide_throw_type_in_protected_and_public_methods: bool,
    check_too_wide_return_type_in_protected_and_public_methods: bool,
    check_too_wide_by_ref_type_in_protected_and_public_methods: bool,
    check_method_throw_type_covariance: bool,
    check_benevolent_union_types: bool,
    check_missing_callable_signature: bool,
    check_duplicate_function_declarations: bool,
    check_duplicate_class_declarations: bool,
    check_boolean_trap: bool,
    check_array_key_cast: bool,
    check_parent_constructor_call: bool,
    check_promoted_property_suggestions: bool,
    check_boolean_conditions: bool,
    check_dynamic_properties: bool,
    check_swapped_arguments: bool,
    check_never_return_suggestions: bool,
    check_switch_missing_default: bool,
    check_throw_without_message: bool,
    check_throw_base_class: bool,
    check_new_exception_discarded: bool,
    check_risky_tests: bool,
    check_dead_enum_cases: bool,
    check_dead_code: bool,
    check_circular_dependencies: bool,
    check_dead_abstract_classes: bool,
    check_dead_members: bool,
    check_renamed_named_arguments: bool,
    configured_level: Option<u8>,
    stub_dirs: &[PathBuf],
    php_version: Option<php_version::PhpVersion>,
    generate_baseline_path: Option<&Path>,
    baseline_path: Option<&Path>,
    ignore_error_entries: &[ignore_errors::IgnoreErrorEntry],
    restricted_symbol_entries: &[restricted_symbols::RestrictedSymbolEntry],
    doctrine_custom_function_entries: &[doctrine_functions::DoctrineCustomFunctionEntry],
    sql_reflection_config: Option<sql_reflection_config::SqlReflectionConfig>,
    sql_reflection_schema: Option<sql_reflection_schema::SchemaCatalog>,
    phpstan_api_check_enabled: bool,
    format: OutputFormat,
) {
    // Deterministic load order matters now in a way it didn't for
    // --check (parse-only): a real corpus can genuinely declare the same
    // fully-qualified class name twice (e.g. multiple vendored copies of
    // the same library bundled by different tools, none php-scoper-
    // prefixed relative to each other) - readdir order is filesystem-
    // dependent, so without sorting, which declaration
    // "wins" the registry slot (and therefore which one's real methods
    // are visible) could silently vary between runs on different
    // machines or filesystems - `collect_analyze_and_context_files`'s own
    // sort()/dedup() calls handle this.
    let (analyze_files, context_files) =
        collect_analyze_and_context_files(dirs, scan_dirs, scan_files);
    if analyze_files.is_empty() {
        let dirs: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        eprintln!("no .php files found under {}", dirs.join(", "));
        return;
    }

    let mut project = project::Project::new();
    project.set_strict_mixed_arrays(strict_mixed_arrays);
    project.set_check_uninitialized_properties(check_uninitialized_properties);
    project.set_check_internal_usage(check_internal_usage);
    project.set_check_function_name_case(check_function_name_case);
    project.set_check_deprecated_usage(check_deprecated_usage);
    project.set_check_empty_method_bodies(check_empty_method_bodies);
    project.set_check_missing_checked_exceptions(check_missing_checked_exceptions);
    project.set_check_strict_printf_placeholder_types(check_strict_printf_placeholder_types);
    project.set_check_too_wide_throw_type_in_protected_and_public_methods(
        check_too_wide_throw_type_in_protected_and_public_methods,
    );
    project.set_check_too_wide_return_type_in_protected_and_public_methods(
        check_too_wide_return_type_in_protected_and_public_methods,
    );
    project.set_check_too_wide_by_ref_type_in_protected_and_public_methods(
        check_too_wide_by_ref_type_in_protected_and_public_methods,
    );
    project.set_check_method_throw_type_covariance(check_method_throw_type_covariance);
    project.set_check_benevolent_union_types(check_benevolent_union_types);
    project.set_check_missing_callable_signature(check_missing_callable_signature);
    project.set_check_duplicate_function_declarations(check_duplicate_function_declarations);
    project.set_check_duplicate_class_declarations(check_duplicate_class_declarations);
    project.set_check_boolean_trap(check_boolean_trap);
    project.set_check_array_key_cast(check_array_key_cast);
    project.set_check_parent_constructor_call(check_parent_constructor_call);
    project.set_check_promoted_property_suggestions(check_promoted_property_suggestions);
    project.set_check_boolean_conditions(check_boolean_conditions);
    project.set_check_dynamic_properties(check_dynamic_properties);
    project.set_check_swapped_arguments(check_swapped_arguments);
    project.set_check_never_return_suggestions(check_never_return_suggestions);
    project.set_check_switch_missing_default(check_switch_missing_default);
    project.set_check_throw_without_message(check_throw_without_message);
    project.set_check_throw_base_class(check_throw_base_class);
    project.set_check_new_exception_discarded(check_new_exception_discarded);
    project.set_check_risky_tests(check_risky_tests);
    project.set_check_dead_enum_cases(check_dead_enum_cases);
    project.set_check_dead_code(check_dead_code);
    project.set_check_circular_dependencies(check_circular_dependencies);
    project.set_check_dead_abstract_classes(check_dead_abstract_classes);
    project.set_check_dead_members(check_dead_members);
    project.set_check_renamed_named_arguments(check_renamed_named_arguments);
    project.set_configured_level(configured_level);
    project.set_restricted_symbols(restricted_symbol_entries);
    project.set_doctrine_custom_functions(doctrine_custom_function_entries);
    project.set_sql_reflection_config(sql_reflection_config);
    project.set_sql_reflection_schema(sql_reflection_schema);
    project.set_php_version(php_version);
    project.set_phpstan_api_check_enabled(phpstan_api_check_enabled);

    // A bare rayon pool uses the default (small) OS worker-thread stack -
    // the sibling `check` command's own `WORKER_STACK_SIZE` precedent
    // (check.rs) records a real, non-adversarial stack overflow at
    // ~200-300 levels of nested parens on a default-sized thread stack,
    // well short of the parser's own `MAX_EXPR_DEPTH=1000` guard -
    // relevant to both phases below, since parsing recurses
    // through the same shape of nesting and the checker's own recursive
    // AST walk (`checker/expr.rs` etc.) has no depth guard of its own
    // either. One call-local pool (not `build_global()`) shared by both
    // phases keeps this stack-size choice scoped to `check-project`
    // instead of silently sizing every other future rayon caller in this
    // binary, and avoids paying pool-construction cost twice.
    const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(WORKER_STACK_SIZE)
        .build()
        .expect("failed to build check-project worker pool");

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    // Lexing+parsing each file is embarrassingly parallel - a pure
    // computation over one file's own source text, with no dependency on
    // (or mutation of) `project`'s shared registry at all (see
    // `read_and_parse_file`/`ParsedFile`'s own doc comments). Once
    // `check_file` below is parallelized, sequential lexing+parsing
    // becomes the dominant remaining cost on a large real-world monorepo,
    // which is what this loop exists to close.
    //
    // `context_files` and `analyze_files` are parsed in one combined
    // `par_iter()` call, not two separate ones - each `pool.install(||
    // ...collect())` is a full synchronization barrier (every worker must
    // finish its share of one list before any worker starts the other),
    // and the two lists are rarely balanced (a real corpus's own vendor/
    // context tree is typically a different size than its first-party
    // analyzed tree), so threads that drain their share of the smaller
    // list early sit idle instead of picking up the other list's work.
    // `par_iter().collect()` on an indexed slice preserves original
    // order, so `combined_parsed.split_off(context_files.len())` cleanly
    // recovers each list's own results in their own original order - the
    // downstream `zip(context_files.iter(), ...)`/`zip(analyze_files.
    // iter(), ...)` insert loops below are unchanged either way.
    // Every real on-disk file reachable through a configured
    // `stub_paths` entry (a directory, walked, or a single file listed
    // directly - `collect_php_files` already handles both) - tags
    // `DuplicateClassSites`' own `is_stub` provenance per file, so a
    // designated stub override (e.g. `Imagick.stub.php`) is never itself
    // flagged by `check_duplicate_class_declarations` and never counts
    // toward flagging the real declaration it overrides. `stub_dirs` is
    // already unioned into `scan_dirs` by the caller (so these files load
    // as context exactly as before) - this is purely an additional,
    // parallel tagging pass, not a second file-collection pass.
    let mut stub_files_vec = Vec::new();
    for dir in stub_dirs {
        project::collect_php_files(dir, &mut stub_files_vec);
    }
    let stub_files: std::collections::HashSet<PathBuf> = stub_files_vec.into_iter().collect();

    let combined_files: Vec<&PathBuf> = context_files.iter().chain(analyze_files.iter()).collect();
    let mut combined_parsed: Vec<Option<(String, project::ParsedFile)>> = pool.install(|| {
        combined_files
            .par_iter()
            .map(|p| read_and_parse_file(p, stub_files.contains(p.as_path())))
            .collect()
    });
    let analyze_parsed = combined_parsed.split_off(context_files.len());
    let context_parsed = combined_parsed;
    std::panic::set_hook(default_hook);

    // Context files insert first: `Registry::index` resolves a
    // fully-qualified-name collision with plain last-write-wins (a bare
    // `HashMap::insert`, no existence check, no diagnostic), and while
    // the vendor-only filter in `composer::discover_vendor_context`
    // keeps a project's own first-party directories out of the context
    // set, it can't prevent a genuinely different vendor package
    // declaring a class under the same name as something in the user's
    // own code (PHP allows unnamespaced global classes). Inserting
    // context first guarantees the analyzed declaration - the one
    // actually being reported on - always wins the registry slot on any
    // such collision (`analyzed: true` for `analyze_files`, `false` for
    // `context_files` - see `Project::insert_parsed_file`'s own `analyzed`
    // parameter doc comment for why this has to reach all the way down to
    // class indexing). This insertion loop is the one part of loading
    // that stays sequential: `Registry::index` is order-sensitive by
    // design, and `par_iter().collect()` above already preserved each
    // parsed result's original (sorted) index order, so this `zip`
    // inserts in exactly the same order the old fully-sequential loop
    // did.
    let mut skipped_context = 0usize;
    for (path, parsed) in context_files.iter().zip(context_parsed) {
        match parsed {
            Some((source, parsed)) => {
                project.insert_parsed_file(path.clone(), parsed, source, false)
            }
            None => skipped_context += 1,
        }
    }
    // Source length, captured here (before `source` is moved into
    // `insert_parsed_file`) rather than re-read from disk afterward -
    // used only to order the check phase's own `par_iter()` below by
    // descending size, so a handful of unusually large files (a real
    // corpus's own long tail) get scheduled onto the pool first instead
    // of last, shrinking the straggler tail rayon's work-stealing alone
    // can't fully avoid when the default alphabetical order happens to
    // put them late. A rough proxy for real check cost (AST complexity
    // matters more than byte count), not a precise one - good enough to
    // shrink the tail without needing real per-file timing data.
    let mut analyze_sizes: HashMap<PathBuf, usize> = HashMap::with_capacity(analyze_files.len());
    let mut skipped_analyze = 0usize;
    for (path, parsed) in analyze_files.iter().zip(analyze_parsed) {
        match parsed {
            Some((source, parsed)) => {
                analyze_sizes.insert(path.clone(), source.len());
                project.insert_parsed_file(path.clone(), parsed, source, true)
            }
            None => skipped_analyze += 1,
        }
    }
    project.finish_loading();

    let mut analyze_files_by_size = analyze_files.clone();
    analyze_files_by_size
        .sort_by_key(|path| std::cmp::Reverse(analyze_sizes.get(path).copied().unwrap_or(0)));

    // Mirrors `check.rs`'s own catch_unwind-with-silenced-hook pattern: a
    // panic while checking one file is reported and skipped rather than
    // aborting every other in-flight file's work, and (unlike a bare
    // `unwrap_or_default()` on the join result) the panic is preserved as
    // a real per-file error rather than silently discarded along with
    // whatever else that worker thread hadn't finished yet. The hook is
    // process-global, so this silences panics from every pool worker
    // thread, not just whichever one calls `install`. `Project`/`Registry`
    // have no interior mutability (no `Cell`/`RefCell`/etc. anywhere), so
    // `&project` is `UnwindSafe` with no `AssertUnwindSafe` wrapper
    // needed - a panic mid-check can't have left it half-mutated.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    // The collect half of the two-pass (collect-then-consume) checking
    // architecture - a trait-donated property write discovered while
    // checking *any* loaded file must reach `check_too_wide_property_
    // types` in whichever file declares the host using it, which a
    // single per-file pass can't guarantee (see `Project::
    // collect_property_assign_facts`'s own doc comment). Run before the
    // real, diagnostic-producing pass below, which consumes the merge via
    // `check_file_with_property_assign_facts` instead of `check_file`.
    // `Registry.sql_reflector`'s only interior mutability is a plain
    // `OnceLock` - if its own lazy-init closure panics mid-connection-
    // attempt, the cell is left simply uninitialized (not corrupted;
    // `OnceLock::get_or_init`'s own contract), so a later observer
    // (including a `catch_unwind` recovering from an unrelated panic
    // elsewhere in the same closure) sees either "not yet initialized"
    // or a real, valid reflector - never a torn/partial one. Asserting
    // `UnwindSafe` here is that specific, checked claim, not a blanket
    // "trust me."
    let property_assign_facts = Arc::new(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            project.collect_property_assign_facts()
        }))
        .unwrap_or_default(),
    );
    // Widened to optionally carry Tier-2 member-ref/escaped-enum data
    // alongside each file's diagnostics - `None` (the `check_dead_enum_
    // cases`-off common case) costs nothing extra. Folding this into the
    // primary pass, rather than letting `check_whole_program`'s own
    // `ensure_member_index_built` call derive it via a wholly separate
    // second `Checker` pass over every file afterward, is what keeps a
    // whole-program check's own real cost to the per-node tracking work
    // below, not a doubling of total check time - see `Project::check_
    // whole_program`'s own doc comment.
    type MemberRefData = (
        Vec<(Span, String, String)>,
        HashSet<String>,
        HashSet<String>,
    );
    type CheckOutcome = Result<(Vec<checker::Diagnostic>, Option<MemberRefData>), String>;
    let needs_tier_2 = check_dead_enum_cases || check_dead_members;
    let checked_results: Vec<(PathBuf, CheckOutcome)> = pool.install(|| {
        analyze_files_by_size
            .par_iter()
            .map(|path| {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if needs_tier_2 {
                        let (diagnostics, member_refs, escaped_enums, escaped_member_classes) =
                            project.check_file_with_property_assign_facts_collecting_member_refs(
                                path,
                                &property_assign_facts,
                            );
                        (
                            diagnostics,
                            Some((member_refs, escaped_enums, escaped_member_classes)),
                        )
                    } else {
                        (
                            project.check_file_with_property_assign_facts(
                                path,
                                &property_assign_facts,
                            ),
                            None,
                        )
                    }
                }));
                let outcome = outcome.map_err(|payload| {
                    payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown panic".to_string())
                });
                (path.clone(), outcome)
            })
            .collect()
    });
    std::panic::set_hook(default_hook);

    let mut results: HashMap<PathBuf, Vec<checker::Diagnostic>> = HashMap::new();
    let mut check_panics: Vec<(PathBuf, String)> = Vec::new();
    for (path, outcome) in checked_results {
        match outcome {
            Ok((diagnostics, member_data)) => {
                if let Some((member_refs, escaped_enums, escaped_member_classes)) = member_data {
                    project.seed_member_refs(
                        &path,
                        member_refs,
                        escaped_enums,
                        escaped_member_classes,
                    );
                }
                results.insert(path, diagnostics);
            }
            Err(msg) => check_panics.push((path, msg)),
        }
    }
    check_panics.sort();
    if needs_tier_2 {
        project.mark_member_index_built();
    }

    for (path, diags) in project.check_whole_program() {
        let entry = results.entry(path).or_default();
        entry.extend(diags);
        // `check_file_impl` always leaves its own per-file diagnostics
        // sorted by span - a plain `.extend()` here would silently break
        // that invariant for any file that has both ordinary and
        // whole-program diagnostics (the new ones would always print
        // last, regardless of their own line).
        entry.sort_by_key(|d| (d.span.start.line, d.span.start.col));
    }

    let mut results = apply_level_filter(results, level);
    ignore_errors::filter(&mut results, ignore_error_entries);
    restricted_symbols::filter_by_path(&mut results);

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
            Ok(baseline) => {
                for stale in baseline::filter(&mut results, &baseline) {
                    eprintln!("baseline entry no longer matches any diagnostic: {stale}");
                }
            }
            Err(e) => eprintln!("failed to load baseline {}: {e}", baseline_path.display()),
        }
    }

    let checked = analyze_files.len() - skipped_analyze - check_panics.len();

    // Text is the only format carrying anything beyond raw diagnostics
    // (the "N more file(s) loaded for context"/"N file(s) skipped"/"N
    // file(s) errored while checking" prose) - `json`/`checkstyle` exist
    // so a CI pipeline can consume clean, uninterrupted structured data
    // on stdout, so they report only the diagnostics themselves (`json`'s
    // own `totals.files_checked` is the one piece of that extra
    // bookkeeping structured output still carries, already reflecting
    // `checked`'s subtraction of both skipped and panicked files - the
    // rest would need their own stable fields to add without guessing at
    // what a consumer wants, so they're left for a real request rather
    // than invented here).
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
            for (path, msg) in &check_panics {
                println!("{}: internal error while checking: {msg}", path.display());
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
            if !check_panics.is_empty() {
                extras.push(format!(
                    "{} file(s) errored while checking",
                    check_panics.len()
                ));
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
    check_function_name_case: bool,
    check_deprecated_usage: bool,
    check_empty_method_bodies: bool,
    check_missing_checked_exceptions: bool,
    check_strict_printf_placeholder_types: bool,
    check_too_wide_throw_type_in_protected_and_public_methods: bool,
    check_too_wide_return_type_in_protected_and_public_methods: bool,
    check_too_wide_by_ref_type_in_protected_and_public_methods: bool,
    check_method_throw_type_covariance: bool,
    check_benevolent_union_types: bool,
    check_missing_callable_signature: bool,
    check_duplicate_function_declarations: bool,
    check_duplicate_class_declarations: bool,
    check_boolean_trap: bool,
    check_array_key_cast: bool,
    check_parent_constructor_call: bool,
    check_promoted_property_suggestions: bool,
    check_boolean_conditions: bool,
    check_dynamic_properties: bool,
    check_swapped_arguments: bool,
    check_never_return_suggestions: bool,
    check_switch_missing_default: bool,
    check_throw_without_message: bool,
    check_throw_base_class: bool,
    check_new_exception_discarded: bool,
    check_risky_tests: bool,
    check_dead_enum_cases: bool,
    check_dead_code: bool,
    check_circular_dependencies: bool,
    check_dead_abstract_classes: bool,
    check_dead_members: bool,
    check_renamed_named_arguments: bool,
    php_version: Option<String>,
    generate_baseline: Option<PathBuf>,
    baseline: Option<PathBuf>,
    scan_dirs: Vec<PathBuf>,
    stub_paths: Vec<PathBuf>,
    no_composer: bool,
    config: Option<PathBuf>,
    format: Option<OutputFormat>,
) {
    // A config file's own `paths`/`baseline`/`scan_paths` are
    // conventionally relative to the config file itself (matching every
    // comparable format - .gitignore, tsconfig.json, real PHPStan's own
    // neon includes), not to whatever directory the invoking shell
    // happens to be in. Captured here, before `config` is shadowed by
    // the loaded `config::Config` below, since the parsed struct itself
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
    // `configured_level` is the raw, pre-collapse value - `None` when
    // neither the CLI flag nor the config key set one at all, distinct
    // from `level` below (which always collapses to a concrete `u8`,
    // defaulting to `MAX_LEVEL`, purely for diagnostic filtering).
    let configured_level = level.or(config.level);
    let level = level.or(config.level).unwrap_or(MAX_LEVEL);
    let strict_mixed_arrays = strict_mixed_arrays || config.strict_mixed_arrays.unwrap_or(false);
    let check_uninitialized_properties =
        check_uninitialized_properties || config.check_uninitialized_properties.unwrap_or(false);
    let check_internal_usage = check_internal_usage || config.check_internal_usage.unwrap_or(false);
    let check_function_name_case =
        check_function_name_case || config.check_function_name_case.unwrap_or(false);
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
    let check_too_wide_throw_type_in_protected_and_public_methods =
        check_too_wide_throw_type_in_protected_and_public_methods
            || config
                .check_too_wide_throw_type_in_protected_and_public_methods
                .unwrap_or(false);
    let check_too_wide_return_type_in_protected_and_public_methods =
        check_too_wide_return_type_in_protected_and_public_methods
            || config
                .check_too_wide_return_type_in_protected_and_public_methods
                .unwrap_or(false);
    let check_too_wide_by_ref_type_in_protected_and_public_methods =
        check_too_wide_by_ref_type_in_protected_and_public_methods
            || config
                .check_too_wide_by_ref_type_in_protected_and_public_methods
                .unwrap_or(false);
    let check_method_throw_type_covariance = check_method_throw_type_covariance
        || config.check_method_throw_type_covariance.unwrap_or(false);
    let check_benevolent_union_types =
        check_benevolent_union_types || config.check_benevolent_union_types.unwrap_or(false);
    let check_missing_callable_signature = check_missing_callable_signature
        || config.check_missing_callable_signature.unwrap_or(false);
    let check_duplicate_function_declarations = check_duplicate_function_declarations
        || config
            .check_duplicate_function_declarations
            .unwrap_or(false);
    let check_duplicate_class_declarations = check_duplicate_class_declarations
        || config.check_duplicate_class_declarations.unwrap_or(false);
    let check_boolean_trap = check_boolean_trap || config.check_boolean_trap.unwrap_or(false);
    let check_array_key_cast = check_array_key_cast || config.check_array_key_cast.unwrap_or(false);
    let check_parent_constructor_call =
        check_parent_constructor_call || config.check_parent_constructor_call.unwrap_or(false);
    let check_promoted_property_suggestions = check_promoted_property_suggestions
        || config.check_promoted_property_suggestions.unwrap_or(false);
    let check_boolean_conditions =
        check_boolean_conditions || config.check_boolean_conditions.unwrap_or(false);
    let check_dynamic_properties =
        check_dynamic_properties || config.check_dynamic_properties.unwrap_or(false);
    let check_swapped_arguments =
        check_swapped_arguments || config.check_swapped_arguments.unwrap_or(false);
    let check_never_return_suggestions =
        check_never_return_suggestions || config.check_never_return_suggestions.unwrap_or(false);
    let check_switch_missing_default =
        check_switch_missing_default || config.check_switch_missing_default.unwrap_or(false);
    let check_throw_without_message =
        check_throw_without_message || config.check_throw_without_message.unwrap_or(false);
    let check_throw_base_class =
        check_throw_base_class || config.check_throw_base_class.unwrap_or(false);
    let check_new_exception_discarded =
        check_new_exception_discarded || config.check_new_exception_discarded.unwrap_or(false);
    let check_risky_tests = check_risky_tests || config.check_risky_tests.unwrap_or(false);
    let check_dead_enum_cases =
        check_dead_enum_cases || config.check_dead_enum_cases.unwrap_or(false);
    let check_dead_code = check_dead_code || config.check_dead_code.unwrap_or(false);
    let check_circular_dependencies =
        check_circular_dependencies || config.check_circular_dependencies.unwrap_or(false);
    let check_dead_abstract_classes =
        check_dead_abstract_classes || config.check_dead_abstract_classes.unwrap_or(false);
    let check_dead_members = check_dead_members || config.check_dead_members.unwrap_or(false);
    let check_renamed_named_arguments =
        check_renamed_named_arguments || config.check_renamed_named_arguments.unwrap_or(false);
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
    let restricted_symbol_entries = config.restricted_symbols.unwrap_or_default();
    let doctrine_custom_function_entries = config.doctrine_custom_functions.unwrap_or_default();
    let sql_reflection_config = config.sql_reflection;
    // A configured `schema_file` is a real, reachable-or-not schema
    // source the same way a live connection would be - unreadable or
    // unparseable-into-any-table is a hard error at startup, not a
    // silent fall-through to today's fallback (the same fail-fast
    // precedent a malformed `restricted_symbols` entry already has).
    let sql_reflection_schema = match sql_reflection_config
        .as_ref()
        .and_then(|c| c.schema_file.as_ref().map(|f| (f.clone(), c.driver)))
    {
        Some((path, driver)) => {
            let path = resolve_against_config(path);
            match sql_reflection_schema::parse_ddl_file(&path, driver) {
                Ok(catalog) => Some(catalog),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        None => None,
    };
    // Schema drift check: only meaningful when both a static schema
    // *and* a live-connection config are configured together - a live
    // connection is otherwise never opened eagerly at startup
    // (`SqlReflectorHandle`'s own lazy-on-first-use design), but this
    // check's whole point is comparing the two sources once, up front,
    // so the eagerness here is scoped to exactly the case where the
    // user already asked for both.
    if let Some(schema) = &sql_reflection_schema
        && let Some(config) = &sql_reflection_config
    {
        // `SqlReflectorHandle::get` already refuses a schema_file-only
        // config with no real live intent (no `database`/`path`) -
        // returning `None` there, same as it would for any other
        // real caller, so nothing further needs to be checked here.
        let handle = sql_reflection_reflector::SqlReflectorHandle::default();
        if let Some(reflector) = handle.get(Some(config)) {
            for warning in
                sql_reflection_schema_drift::schema_drift_warnings(schema, reflector, config.driver)
            {
                eprintln!("{warning}");
            }
        }
    }

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

    // Mirrors `scan_dirs`'s own resolution exactly (CLI flag wins, else
    // the config key, resolved against the config file's own directory)
    // - a `stub_paths` entry is unioned into `scan_dirs` below
    // so it still loads for type-resolution context exactly as before
    // (nothing about the existing merge-into-registry behavior changes),
    // while also being kept separately so `run_check_project` can walk it
    // a second time to tag `DuplicateClassSites`' own `is_stub`
    // provenance per file.
    let stub_dirs: Vec<PathBuf> = if !stub_paths.is_empty() {
        stub_paths
    } else {
        config
            .stub_paths
            .unwrap_or_default()
            .into_iter()
            .map(resolve_against_config)
            .collect()
    };
    scan_dirs.extend(stub_dirs.iter().cloned());

    let mut scan_files = Vec::new();
    // Gated the same way as the vendor-context load just above (both
    // need the project's own `composer.json`, and `--no-composer`
    // already means "don't consult Composer at all for this run") - a
    // project with no real `phpstan/phpstan`/`phpstan/extension-
    // installer` dependency has no backward-compatibility promise to
    // check anything against.
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
        check_function_name_case,
        check_deprecated_usage,
        check_empty_method_bodies,
        check_missing_checked_exceptions,
        check_strict_printf_placeholder_types,
        check_too_wide_throw_type_in_protected_and_public_methods,
        check_too_wide_return_type_in_protected_and_public_methods,
        check_too_wide_by_ref_type_in_protected_and_public_methods,
        check_method_throw_type_covariance,
        check_benevolent_union_types,
        check_missing_callable_signature,
        check_duplicate_function_declarations,
        check_duplicate_class_declarations,
        check_boolean_trap,
        check_array_key_cast,
        check_parent_constructor_call,
        check_promoted_property_suggestions,
        check_boolean_conditions,
        check_dynamic_properties,
        check_swapped_arguments,
        check_never_return_suggestions,
        check_switch_missing_default,
        check_throw_without_message,
        check_throw_base_class,
        check_new_exception_discarded,
        check_risky_tests,
        check_dead_enum_cases,
        check_dead_code,
        check_circular_dependencies,
        check_dead_abstract_classes,
        check_dead_members,
        check_renamed_named_arguments,
        configured_level,
        &stub_dirs,
        php_version,
        generate_baseline.as_deref(),
        baseline.as_deref(),
        &ignore_error_entries,
        &restricted_symbol_entries,
        &doctrine_custom_function_entries,
        sql_reflection_config,
        sql_reflection_schema,
        phpstan_api_check_enabled,
        format.unwrap_or_default(),
    )
}
