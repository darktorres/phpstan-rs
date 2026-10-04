//! The corpus gate's runner (`cargo xtask corpus`), not shipped: one
//! analysis of a gate configuration, finalized into every view the gate
//! snapshots, through the same library calls `check-project` makes.
//!
//! `gate-runner [--config <file>] [--set <key=value>]... [--no-composer]
//! [--level <n>] [--baseline <file>] [--dump-types] [--dump-front-end]
//! [--dump-registry] [--dump-stub-bases] [--dump-expr-types]
//! [--dump-doc-resolutions] --out <dir> [<path>...]` loads once, checks
//! once and writes to `<dir>`:
//! - `report.json`/`report.err`: `check-project --format json`'s stdout
//!   and stderr;
//! - `emission.json`/`emission.err`: the same with `--emission-order`;
//! - `baseline.json`/`baseline.err` with `--baseline`: the same with
//!   `--baseline <file>`;
//! - `types.txt` with `--dump-types`: `--dump-types`' file;
//! - `diagnostics.txt`: every field of every diagnostic of the report, one
//!   line each, with the UTF-16 columns of its ends
//!   (`AnalysisResult::diagnostic_view`);
//! - `front-end/` with `--dump-front-end`: the front end's dump of every
//!   file, from a parse of its own;
//! - `registry.txt.gz` with `--dump-registry`: the registry dump of what
//!   the load leaves, taken before the check;
//! - `stub-bases.txt.gz` with `--dump-stub-bases`: the stub bases whole;
//! - `expr-types.txt.gz` and `doc-resolutions.txt.gz` with
//!   `--dump-expr-types` and `--dump-doc-resolutions`: the check's
//!   records.
//!
//! `gate-runner --type-drift <old> <new> [--fixture-root <dir>] [<config
//! arguments>] --out <dir>` relates two recorded type dumps instead of
//! analysing, and writes `type-drift.txt`: with configuration arguments
//! within that project's loaded class hierarchy, with `--fixture-root`
//! each fixture within its own, and with neither by name only.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use phpstan_rs::app::check_project::{self, CheckRequest, CheckRun};
use phpstan_rs::gate;
use phpstan_rs::output::render_json;
use phpstan_rs::project::session::{AnalysisResult, DiagnosticOrder};

struct Args {
    request: CheckRequest,
    baseline: Option<PathBuf>,
    dump_types: bool,
    dump_front_end: bool,
    dump_registry: bool,
    dump_stub_bases: bool,
    dump_expr_types: bool,
    dump_doc_resolutions: bool,
    type_drift: Option<(PathBuf, PathBuf)>,
    fixture_root: Option<PathBuf>,
    out: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut request = CheckRequest::default();
    let mut baseline = None;
    let mut dump_types = false;
    let (mut dump_front_end, mut dump_registry, mut dump_stub_bases) = (false, false, false);
    let (mut dump_expr_types, mut dump_doc_resolutions) = (false, false);
    let mut type_drift = None;
    let mut fixture_root = None;
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--config" => request.config_file = Some(PathBuf::from(value()?)),
            "--set" => request.sets.push(value()?),
            "--no-composer" => request.no_composer = true,
            "--level" => {
                let level = value()?;
                request.level = Some(level.parse().map_err(|_| format!("bad level {level}"))?);
            }
            "--baseline" => baseline = Some(PathBuf::from(value()?)),
            "--dump-types" => dump_types = true,
            "--dump-front-end" => dump_front_end = true,
            "--dump-registry" => dump_registry = true,
            "--dump-stub-bases" => dump_stub_bases = true,
            "--dump-expr-types" => dump_expr_types = true,
            "--dump-doc-resolutions" => dump_doc_resolutions = true,
            "--type-drift" => {
                let old = PathBuf::from(value()?);
                let new = PathBuf::from(args.next().ok_or("--type-drift needs two dumps")?);
                type_drift = Some((old, new));
            }
            "--fixture-root" => fixture_root = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            path => request.paths.push(PathBuf::from(path)),
        }
    }
    Ok(Args {
        request,
        baseline,
        dump_types,
        dump_front_end,
        dump_registry,
        dump_stub_bases,
        dump_expr_types,
        dump_doc_resolutions,
        type_drift,
        fixture_root,
        out: out.ok_or("--out is required")?,
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gate-runner: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    if let Some((old, new)) = &args.type_drift {
        return drift(&args, old, new);
    }
    let config = check_project::load_config(&args.request)?;
    let run = check_project::prepare(&args.request, &config, phpstan_rs::project::worker_pool())?;
    if args.dump_front_end {
        run.session.dump_front_end(&args.out.join("front-end"))?;
    }
    if args.dump_stub_bases {
        run.session
            .dump_stub_bases(&args.out.join("stub-bases.txt.gz"))?;
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let loaded = run.session.load();
    std::panic::set_hook(default_hook);
    let loaded = loaded?;
    if args.dump_registry {
        write_gzip(
            &args.out.join("registry.txt.gz"),
            &loaded.project.registry_dump(),
        )?;
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let analysis = run.session.analyse(
        loaded,
        phpstan_rs::project::Records {
            types: args.dump_types,
            expr_types: args.dump_expr_types,
            doc_resolutions: args.dump_doc_resolutions,
        },
    );
    std::panic::set_hook(default_hook);
    if let Some(dump) = &analysis.type_dump {
        write(&args.out.join("types.txt"), dump)?;
    }
    for (name, dump) in [
        ("expr-types.txt.gz", &analysis.expr_type_dump),
        ("doc-resolutions.txt.gz", &analysis.doc_resolution_dump),
    ] {
        if let Some(dump) = dump {
            write_gzip(&args.out.join(name), dump)?;
        }
    }
    let notices: Vec<String> = run
        .notices()
        .into_iter()
        .chain(analysis.notices())
        .collect();

    let report = write_view(&args.out, "report", &run, &analysis, &notices, |o| o)?;
    write(
        &args.out.join("diagnostics.txt"),
        &analysis.diagnostic_view(&report),
    )?;
    write_view(&args.out, "emission", &run, &analysis, &notices, |mut o| {
        o.order = DiagnosticOrder::Emission;
        o
    })?;
    if let Some(path) = &args.baseline {
        let baseline_run = CheckRun {
            baseline_path: Some(path.clone()),
            ..run
        };
        let (baseline, load_error) = match baseline_run.load_baseline() {
            Some(Ok(baseline)) => (Some(baseline), None),
            Some(Err(line)) => (None, Some(line)),
            None => (None, None),
        };
        let notices: Vec<String> = notices.iter().cloned().chain(load_error).collect();
        write_view(
            &args.out,
            "baseline",
            &baseline_run,
            &analysis,
            &notices,
            |mut o| {
                o.baseline = baseline;
                o
            },
        )?;
    }
    Ok(())
}

/// Relates the dumps `old` and `new` and writes `type-drift.txt`.
fn drift(args: &Args, old: &Path, new: &Path) -> Result<(), String> {
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))
    };
    let (old, new) = (read(old)?, read(new)?);
    let drift = if let Some(root) = &args.fixture_root {
        gate::type_drift_of_fixtures(root, &old, &new, phpstan_rs::project::worker_pool())?
    } else if args.request.config_file.is_some() || !args.request.paths.is_empty() {
        let config = check_project::load_config(&args.request)?;
        let run =
            check_project::prepare(&args.request, &config, phpstan_rs::project::worker_pool())?;
        let loaded = run.session.load()?;
        gate::type_drift_in_project(&loaded.project, &old, &new)
    } else {
        gate::type_drift_structural(&old, &new)
    };
    write(&args.out.join("type-drift.txt"), &drift.report())
}

/// Writes `<name>.json` and `<name>.err`: the report `adjust` makes of the
/// run's default finalization, as `check-project --format json` prints
/// it, after the `notices` printed before it. Gives the report.
fn write_view(
    out: &Path,
    name: &str,
    run: &CheckRun,
    analysis: &AnalysisResult,
    notices: &[String],
    adjust: impl FnOnce(
        phpstan_rs::project::session::Finalize,
    ) -> phpstan_rs::project::session::Finalize,
) -> Result<phpstan_rs::project::session::Report, String> {
    let report = analysis.finalize(&adjust(run.finalize_options(None)));
    let mut err = String::new();
    for line in notices.iter().chain(&report.notices()) {
        err.push_str(line);
        err.push('\n');
    }
    write(
        &out.join(format!("{name}.json")),
        &format!("{}\n", render_json(&report.results, analysis.checked)),
    )?;
    write(&out.join(format!("{name}.err")), &err)?;
    Ok(report)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn write_gzip(path: &Path, text: &str) -> Result<(), String> {
    phpstan_rs::project::session::write_gzip(path, text.as_bytes())
}
