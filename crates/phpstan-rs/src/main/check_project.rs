//! `check-project`: the command line parsed into an
//! `app::check_project::CheckRequest`, the config file loaded with its
//! `--set` assignments, the run resolved and analysed by the library,
//! then baselined, fixed and printed.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

use output::OutputFormat;
use phpstan_rs::app::check_project::{self, CheckRequest, CheckRun};
use phpstan_rs::project::session::{AnalysisResult, apply_fixes_to_disk};
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
    // Made first, so that the stubs build beside the listing and the parse.
    let stubs = phpstan_rs::registry::stub_set::Stubs::new(
        phpstan_rs::registry::stub_set::StubSource::from_environment(),
        phpstan_rs::project::worker_pool(),
    );
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
    let mut run = match check_project::prepare_cli(&request, &config, stubs) {
        Ok(run) => run,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    for notice in run.notices() {
        eprintln!("{notice}");
    }
    if args.fix {
        run.session.keep_read_digests();
    }
    // A file whose parse or check panics is reported per file; don't also
    // print each caught panic's default message.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let analysis = run.session.run(args.dump_types.is_some());
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
    report(
        &run,
        analysis,
        args.generate_baseline.as_deref(),
        args.fix,
        args.format.unwrap_or_default(),
    );
    // The process ends here: freeing the project's tables one by one, on
    // this thread, would only delay it.
    std::mem::forget(run);
}

/// Whether the friendly report is written with colour: to a terminal, unless
/// `NO_COLOR` is set to something (<https://no-color.org>).
pub(crate) fn wants_colour(stdout_is_terminal: bool, no_color: Option<&OsStr>) -> bool {
    stdout_is_terminal && no_color.is_none_or(OsStr::is_empty)
}

/// Writes the report to stdout in one piece. A coloured report's escapes
/// reach a Windows console only through `anstream`, which turns on the
/// console's virtual-terminal processing, or translates the escapes for a
/// console that has none; elsewhere they pass as they are. A closed pipe ends
/// the run quietly with status 141, the status of a process that SIGPIPE
/// ended; any other failure is printed and ends the run with status 1.
fn write_stdout(report: &[u8], colour: bool) {
    let written = if colour {
        anstream::AutoStream::always(std::io::stdout()).write_all(report)
    } else {
        std::io::stdout().lock().write_all(report)
    }
    .and_then(|()| std::io::stdout().flush());
    match written {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => std::process::exit(141),
        Err(e) => {
            eprintln!("failed to write the report: {e}");
            std::process::exit(1);
        }
    }
}

fn report(
    run: &CheckRun,
    analysis: AnalysisResult,
    generate_baseline_path: Option<&Path>,
    apply_fixes: bool,
    format: OutputFormat,
) {
    let check_panics = analysis.check_panics.clone();
    let (checked, skipped_analyze, skipped_context, context_loaded) = (
        analysis.checked,
        analysis.skipped_analyze,
        analysis.skipped_context,
        analysis.context_loaded,
    );
    let baseline_per_identifier = run.baseline_per_identifier;
    if let Some(generate_baseline_path) = generate_baseline_path {
        let checked = analysis.checked_paths();
        let results = analysis.finalize(&run.finalize_options(None)).results;
        let generated = if baseline_per_identifier {
            baseline::generate_per_identifier(generate_baseline_path, &results, &checked)
        } else {
            baseline::generate(generate_baseline_path, &results, &checked)
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
    let read_digests = if apply_fixes {
        analysis.read_digests()
    } else {
        Default::default()
    };
    let report = analysis.finalize(&run.finalize_options(baseline));
    for notice in report.notices() {
        eprintln!("{notice}");
    }
    let results = report.results;

    if apply_fixes {
        let outcome = apply_fixes_to_disk(&results, &read_digests);
        for path in &outcome.changed_since_analysis {
            eprintln!("skipped {}: changed since it was analysed", path.display());
        }
        for failure in &outcome.failures {
            eprintln!("{failure}");
        }
        eprintln!(
            "applied {} fix(es) in {} file(s)",
            outcome.applied, outcome.fixed_files
        );
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
    // The report is built whole and written once, so a closed pipe is found
    // by one write, not by whichever `println!` meets it first.
    let mut out: Vec<u8> = Vec::new();
    let mut colour = false;
    match format {
        OutputFormat::Text | OutputFormat::Friendly => {
            let mut paths: Vec<&PathBuf> = results.keys().collect();
            paths.sort();

            let mut total = 0usize;
            if format == OutputFormat::Friendly {
                use std::io::IsTerminal;
                colour = wants_colour(
                    std::io::stdout().is_terminal(),
                    std::env::var_os("NO_COLOR").as_deref(),
                );
                out.extend_from_slice(output::render_friendly(&results, colour).as_bytes());
                total = results.values().map(Vec::len).sum();
            } else {
                for path in paths {
                    for d in &results[path] {
                        writeln!(out, "{}:{}", path.display(), output::diagnostic_line(d))
                            .expect("a vector takes every write");
                        if let Some(tip) = d.tip_text() {
                            writeln!(out, "    💡 {tip}").expect("a vector takes every write");
                        }
                        total += 1;
                    }
                }
            }
            for (path, msg) in &check_panics {
                writeln!(
                    out,
                    "{}: internal error while checking: {msg}",
                    path.display()
                )
                .expect("a vector takes every write");
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
            writeln!(out, "{summary}").expect("a vector takes every write");
        }
        OutputFormat::Json => {
            output::write_json(&mut out, &results, checked)
                .and_then(|()| writeln!(out))
                .expect("a vector takes every write");
        }
        OutputFormat::Checkstyle => {
            out.extend_from_slice(output::render_checkstyle(&results).as_bytes());
        }
        OutputFormat::Junit => {
            out.extend_from_slice(output::render_junit(&results).as_bytes());
        }
    }
    write_stdout(&out, colour);
}
