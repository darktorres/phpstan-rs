//! `check-project`: the command line parsed into an
//! `app::check_project::CheckRequest`, the config file loaded with its
//! `--set` assignments, the run resolved and analysed by the library,
//! then baselined, fixed and printed.

use std::path::{Path, PathBuf};

use output::OutputFormat;
use phpstan_rs::app::check_project::{self, CheckRequest, CheckRun};
use phpstan_rs::project::session::AnalysisResult;
use phpstan_rs::{baseline, output};

/// Load every .php file under the given paths and run the full checker
/// over all of them, reporting every diagnostic.
#[derive(clap::Args)]
pub(crate) struct CheckProjectArgs {
    // Not `required = true`: a TOML config file's own `paths` can
    // supply these instead (see `run_check_project`'s resolution
    // logic) - the "must know what to check" requirement is
    // enforced there, once the config (if any) has been consulted,
    // not statically by clap.
    pub(crate) dirs: Vec<PathBuf>,
    /// Report up to and including this level (see
    /// diagnostic::Diagnostic's own doc comment) - unset means
    /// "report everything".
    #[arg(long)]
    pub(crate) level: Option<u8>,
    /// Target PHP version, as "major.minor" (e.g. "8.1") - gates the
    /// portability rules that check "X is only supported on PHP Y.Z
    /// and later" (see `php_version::PhpFeature`). Unset means no
    /// such rule has anything to check against yet.
    #[arg(long)]
    pub(crate) php_version: Option<String>,
    /// Sets a config key, overriding the config file: `--set
    /// ext.extras.check_boolean_trap=true`, `--set level=5`. The value is
    /// read as TOML, or as a string when it isn't. Repeatable.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    pub(crate) sets: Vec<String>,
    #[arg(long)]
    pub(crate) generate_baseline: Option<PathBuf>,
    #[arg(long)]
    pub(crate) baseline: Option<PathBuf>,
    /// Matches a baseline entry by its identifier when its message
    /// text no longer matches, so rewording doesn't resurface it.
    #[arg(long)]
    pub(crate) baseline_match_identifier: bool,
    /// Writes `--generate-baseline` as one file per identifier in a
    /// directory named after the baseline file, which becomes an
    /// index including them.
    #[arg(long)]
    pub(crate) baseline_per_identifier: bool,
    /// Applies every suggested fix (the same ones the language server
    /// offers as quick fixes) to the files on disk.
    #[arg(long)]
    pub(crate) fix: bool,
    /// Writes every tracked variable's type after each statement of
    /// every analysed file to this path (a regression-gate snapshot).
    #[arg(long, hide = true)]
    pub(crate) dump_types: Option<PathBuf>,
    /// Writes every expression's inferred and native type in the final
    /// pass of each analysed file to this path, a dump with its manifest
    /// beside it (a regression-gate snapshot).
    #[arg(long, hide = true)]
    pub(crate) dump_expr_types: Option<PathBuf>,
    /// Writes every PHPDoc resolution the check makes of each analysed
    /// file to this path, a dump with its manifest beside it (a
    /// regression-gate snapshot).
    #[arg(long, hide = true)]
    pub(crate) dump_doc_resolutions: Option<PathBuf>,
    /// Writes the front end's dump of every file and embedded stub to this
    /// path, a dump with its manifest beside it, and stops (a
    /// regression-gate snapshot, O1).
    #[arg(long, hide = true)]
    pub(crate) dump_front_end: Option<PathBuf>,
    /// Runs the edit replay over the analysed files, writing its dumps and
    /// verdict into this directory, and stops; fails when a step leaves
    /// the project other than a fresh analysis finds it. Edits the files:
    /// the tree must be a copy with a `.phpstan-rs-replay` file at its
    /// root.
    #[arg(long, hide = true)]
    pub(crate) edit_replay: Option<PathBuf>,
    /// The seed the edit replay draws its files with.
    #[arg(long, hide = true, default_value_t = 1)]
    pub(crate) replay_seed: u64,
    /// Writes the registry dump of what the load leaves to this path, a
    /// dump with its manifest beside it, and stops (a regression-gate
    /// snapshot).
    #[arg(long, hide = true)]
    pub(crate) dump_registry: Option<PathBuf>,
    /// Writes both stub bases whole, which the registry dump prints its
    /// project over, to this path, a dump with its manifest beside it, and
    /// stops (a regression-gate snapshot).
    #[arg(long, hide = true)]
    pub(crate) dump_stub_bases: Option<PathBuf>,
    /// Another run's dumps: each dump above is written only where it
    /// differs from the dump of the same name in this directory, when it
    /// has one, and the edit replay's from those of the replay written
    /// into it.
    #[arg(long, hide = true)]
    pub(crate) dump_reference: Option<PathBuf>,
    /// Prints each file's diagnostics in the order the checks emitted them
    /// (a determinism check), not the usual total order.
    #[arg(long, hide = true)]
    pub(crate) emission_order: bool,
    /// Extra directories loaded for type-resolution context only -
    /// never analyzed or reported on. Repeatable.
    #[arg(long = "scan-dir")]
    pub(crate) scan_dirs: Vec<PathBuf>,
    /// A directory (walked) or single file whose class-like
    /// declarations are designated overrides, never a genuine
    /// accidental duplicate (e.g. a vendored `Imagick.stub.php`) -
    /// see `check_duplicate_class_declarations` above. Repeatable.
    /// Also loaded for type-resolution context, exactly like
    /// `--scan-dir` (unioned into it internally).
    #[arg(long = "stub-path")]
    pub(crate) stub_paths: Vec<PathBuf>,
    /// Disables the automatic Composer-autoloader-based `vendor/`
    /// context load (see `composer::vendor::discover_vendor_context`).
    /// `--scan-dir` still works.
    #[arg(long)]
    pub(crate) no_composer: bool,
    /// The TOML config file (`phpstan-rs.toml`): its keys are what
    /// `--set` sets, and each flag above, when given, overrides the key
    /// it names.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    /// Output format: `text` (default, human-readable), `json`, or
    /// `checkstyle` (XML) - the latter two for CI pipelines that want
    /// to consume diagnostics as structured data rather than parsing
    /// plain text. CLI-only, matching real PHPStan's own
    /// `--error-format`: unlike `level`/`baseline`/etc., this isn't a
    /// `config.rs` TOML key, since it's how one particular invocation
    /// wants its output shaped, not a standing project setting.
    #[arg(long)]
    pub(crate) format: Option<OutputFormat>,
}

pub(crate) fn run_check_project_command(args: CheckProjectArgs) {
    let request = CheckRequest {
        config_file: args.config.clone(),
        sets: args.sets.clone(),
        paths: args.dirs.clone(),
        scan_paths: args.scan_dirs.clone(),
        stub_paths: args.stub_paths.clone(),
        level: args.level,
        php_version: args.php_version.clone(),
        no_composer: args.no_composer,
        emission_order: args.emission_order,
        baseline: args.baseline.clone(),
        baseline_match_identifier: args.baseline_match_identifier,
        baseline_per_identifier: args.baseline_per_identifier,
    };
    let config = match check_project::load_config(&request) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let run = match check_project::prepare(&request, &config, phpstan_rs::project::worker_pool()) {
        Ok(run) => run,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    for notice in run.notices() {
        eprintln!("{notice}");
    }
    let reference = |out: &Path| -> Option<PathBuf> {
        let dir = args.dump_reference.as_ref()?;
        let manifest = dumps::manifest_path(&dir.join(out.file_name()?));
        manifest.is_file().then_some(manifest)
    };
    if let Some(out) = &args.dump_front_end {
        match run.session.dump_front_end(out, reference(out).as_deref()) {
            Ok(dump) => eprintln!(
                "front end: {} files, {} unions, in {}",
                dump.files,
                dump.unions,
                out.display()
            ),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(out) = &args.edit_replay {
        match run
            .session
            .edit_replay(out, args.replay_seed, args.dump_reference.as_deref())
        {
            Ok(verdict) => eprint!("{verdict}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(out) = &args.dump_stub_bases {
        if let Err(e) = run.session.dump_stub_bases(out, reference(out).as_deref()) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        eprintln!("stub bases: {}", out.display());
        return;
    }
    if let Some(out) = &args.dump_registry {
        if let Err(e) = run.session.dump_registry(out, reference(out).as_deref()) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        eprintln!("registry: {}", out.display());
        return;
    }
    // A file whose parse or check panics is reported per file; don't also
    // print each caught panic's default message.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let analysis = run.session.run(phpstan_rs::project::Records {
        types: args.dump_types.is_some(),
        expr_types: args.dump_expr_types.is_some(),
        doc_resolutions: args.dump_doc_resolutions.is_some(),
    });
    std::panic::set_hook(default_hook);
    let analysis = match analysis {
        Ok(analysis) => analysis,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    for notice in analysis.notices() {
        eprintln!("{notice}");
    }
    if let (Some(out), Some(dump)) = (&args.dump_types, &analysis.type_dump)
        && let Err(e) = std::fs::write(out, dump)
    {
        eprintln!("failed to write type dump {}: {e}", out.display());
    }
    for (out, dump) in [
        (&args.dump_expr_types, &analysis.expr_type_dump),
        (&args.dump_doc_resolutions, &analysis.doc_resolution_dump),
    ] {
        if let (Some(out), Some(dump)) = (out, dump)
            && let Err(e) = phpstan_rs::project::session::write_dump(
                out,
                dumps::Kind::ByPrefix(b':'),
                reference(out).as_deref(),
                |w| w.write_all(dump.as_bytes()),
            )
        {
            eprintln!("failed to write {e}");
        }
    }
    report(
        &run,
        &analysis,
        args.generate_baseline.as_deref(),
        args.fix,
        args.format.unwrap_or_default(),
    );
    // The process ends here: freeing the project's tables one by one, on
    // this thread, would only delay it.
    std::mem::forget(analysis);
    std::mem::forget(run);
}

fn report(
    run: &CheckRun,
    analysis: &AnalysisResult,
    generate_baseline_path: Option<&Path>,
    apply_fixes: bool,
    format: OutputFormat,
) {
    let AnalysisResult {
        checked,
        skipped_analyze,
        skipped_context,
        context_loaded,
        check_panics,
        ..
    } = analysis;
    let (checked, skipped_analyze, skipped_context, context_loaded) = (
        *checked,
        *skipped_analyze,
        *skipped_context,
        *context_loaded,
    );
    let baseline_per_identifier = run.baseline_per_identifier;
    if let Some(generate_baseline_path) = generate_baseline_path {
        let results = analysis.finalize(&run.finalize_options(None)).results;
        let generated = if baseline_per_identifier {
            baseline::generate_per_identifier(generate_baseline_path, &results)
        } else {
            baseline::generate(generate_baseline_path, &results)
        };
        match generated {
            Ok(count) => println!(
                "baseline written to {}: {count} diagnostic(s)",
                generate_baseline_path.display()
            ),
            Err(e) => eprintln!(
                "failed to write baseline to {}: {e}",
                generate_baseline_path.display()
            ),
        }
        return;
    }

    let baseline = match run.load_baseline() {
        Some(Ok(baseline)) => Some(baseline),
        Some(Err(line)) => {
            eprintln!("{line}");
            None
        }
        None => None,
    };
    let report = analysis.finalize(&run.finalize_options(baseline));
    for notice in report.notices() {
        eprintln!("{notice}");
    }
    let results = report.results;

    if apply_fixes {
        let mut fixed_files = 0usize;
        let mut applied_total = 0usize;
        let mut paths: Vec<&PathBuf> = results.keys().collect();
        paths.sort();
        for path in paths {
            let diagnostics = &results[path];
            if !diagnostics.iter().any(|d| d.fix.is_some()) {
                continue;
            }
            let Ok(source) = std::fs::read(path) else {
                eprintln!("failed to read {} to apply fixes", path.display());
                continue;
            };
            let (fixed, applied) =
                diagnostics::fix::apply_fixes(&source, diagnostics::fix::fixes_of(diagnostics));
            if applied == 0 || fixed == source {
                continue;
            }
            match std::fs::write(path, fixed) {
                Ok(()) => {
                    fixed_files += 1;
                    applied_total += applied;
                }
                Err(e) => eprintln!("failed to write fixes to {}: {e}", path.display()),
            }
        }
        eprintln!("applied {applied_total} fix(es) in {fixed_files} file(s)");
    }

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
        OutputFormat::Text | OutputFormat::Friendly => {
            let mut paths: Vec<&PathBuf> = results.keys().collect();
            paths.sort();

            let mut total = 0usize;
            if format == OutputFormat::Friendly {
                use std::io::IsTerminal;
                let colour = std::io::stdout().is_terminal()
                    && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
                print!("{}", output::render_friendly(&results, colour));
                total = results.values().map(Vec::len).sum();
            } else {
                for path in paths {
                    for d in &results[path] {
                        println!("{}:{}", path.display(), output::diagnostic_line(d));
                        if let Some(tip) = d.tip_text() {
                            println!("    💡 {tip}");
                        }
                        total += 1;
                    }
                }
            }
            for (path, msg) in check_panics {
                println!("{}: internal error while checking: {msg}", path.display());
            }
            let mut summary = format!("\n{checked} file(s) checked, {total} diagnostic(s)");
            let mut extras = Vec::new();
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
            use std::io::Write as _;
            let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
            let written = output::write_json(&mut stdout, &results, checked)
                .and_then(|()| writeln!(stdout))
                .and_then(|()| stdout.flush());
            if let Err(e) = written {
                eprintln!("failed to write the JSON report: {e}");
            }
        }
        OutputFormat::Checkstyle => {
            print!("{}", output::render_checkstyle(&results));
        }
        OutputFormat::Junit => {
            print!("{}", output::render_junit(&results));
        }
    }
}
