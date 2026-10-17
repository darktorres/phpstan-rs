//! The corpus gate's runner (`cargo xtask corpus`), not shipped: one
//! analysis of a gate configuration, finalized into every view the gate
//! snapshots, through the same library calls `check-project` makes.
//!
//! `gate-runner [--config <file>] [--set <key=value>]... [--no-composer]
//! [--level <n>] [--baseline <file>] [--dump-types] --out <dir> [<path>...]` writes to
//! `<dir>`:
//! - `report.json`/`report.err`: `check-project --format json`'s stdout
//!   and stderr;
//! - `emission.json`/`emission.err`: the same with `--emission-order`;
//! - `baseline.json`/`baseline.err` with `--baseline`: the same with
//!   `--baseline <file>`;
//! - `types.txt` with `--dump-types`: `--dump-types`' file.
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
    type_drift: Option<(PathBuf, PathBuf)>,
    fixture_root: Option<PathBuf>,
    out: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut request = CheckRequest::default();
    let mut baseline = None;
    let mut dump_types = false;
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

/// The built-in stubs of this run, on the process's worker pool. Made
/// before anything else is read, so that they build beside it.
fn stubs() -> phpstan_rs::registry::stub_set::Stubs {
    phpstan_rs::registry::stub_set::Stubs::new(
        phpstan_rs::registry::stub_set::StubSource::from_environment(),
        phpstan_rs::project::worker_pool(),
    )
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    if let Some((old, new)) = &args.type_drift {
        return drift(&args, old, new);
    }
    let stubs = stubs();
    let config = check_project::load_config(&args.request)?;
    let run = check_project::prepare_cli(&args.request, &config, stubs)?;
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let analysis = run.session.run(args.dump_types);
    std::panic::set_hook(default_hook);
    let analysis = analysis?;
    if let Some(dump) = &analysis.type_dump {
        write(&args.out.join("types.txt"), dump)?;
    }
    let notices: Vec<String> = run
        .notices()
        .into_iter()
        .chain(analysis.notices())
        .collect();

    write_view(&args.out, "report", &run, &analysis, &notices, |o| o)?;
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
        gate::type_drift_of_fixtures(root, &old, &new, stubs())?
    } else if args.request.config_file.is_some() || !args.request.paths.is_empty() {
        let stubs = stubs();
        let config = check_project::load_config(&args.request)?;
        let run = check_project::prepare_cli(&args.request, &config, stubs)?;
        let loaded = run.session.load()?;
        gate::type_drift_in_project(&loaded.project, &old, &new)
    } else {
        gate::type_drift_structural(&old, &new)
    };
    write(&args.out.join("type-drift.txt"), &drift.report())
}

/// Writes `<name>.json` and `<name>.err`: the report `adjust` makes of the
/// run's default finalization, as `check-project --format json` prints
/// it, after the `notices` printed before it.
fn write_view(
    out: &Path,
    name: &str,
    run: &CheckRun,
    analysis: &AnalysisResult,
    notices: &[String],
    adjust: impl FnOnce(
        phpstan_rs::project::session::Finalize,
    ) -> phpstan_rs::project::session::Finalize,
) -> Result<(), String> {
    let report = analysis.finalize_copy(&adjust(run.finalize_options(None)));
    let mut err = String::new();
    for line in notices.iter().chain(&report.notices()) {
        err.push_str(line);
        err.push('\n');
    }
    write(
        &out.join(format!("{name}.json")),
        &format!("{}\n", render_json(&report.results, analysis.checked)),
    )?;
    write(&out.join(format!("{name}.err")), &err)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}
