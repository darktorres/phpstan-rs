//! The corpus gate's runner (`cargo xtask corpus`), not shipped: one
//! analysis of a gate configuration, finalized into every view the gate
//! snapshots, through the same library calls `check-project` makes.
//!
//! `gate-runner [--config <file>] [--no-composer] [--level <n>]
//! [--baseline <file>] [--dump-types] --out <dir> [<path>...]` writes to
//! `<dir>`:
//! - `report.json`/`report.err`: `check-project --format json`'s stdout
//!   and stderr;
//! - `emission.json`/`emission.err`: the same with `--emission-order`;
//! - `baseline.json`/`baseline.err` with `--baseline`: the same with
//!   `--baseline <file>`;
//! - `types.txt` with `--dump-types`: `--dump-types`' file.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use phpstan_rs::app::check_project::{self, CheckRequest, CheckRun};
use phpstan_rs::output::render_json;
use phpstan_rs::project::session::{AnalysisResult, DiagnosticOrder};

struct Args {
    request: CheckRequest,
    baseline: Option<PathBuf>,
    dump_types: bool,
    out: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut request = CheckRequest::default();
    let mut baseline = None;
    let mut dump_types = false;
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--config" => request.config_file = Some(PathBuf::from(value()?)),
            "--no-composer" => request.no_composer = true,
            "--level" => {
                let level = value()?;
                request.level = Some(level.parse().map_err(|_| format!("bad level {level}"))?);
            }
            "--baseline" => baseline = Some(PathBuf::from(value()?)),
            "--dump-types" => dump_types = true,
            "--out" => out = Some(PathBuf::from(value()?)),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            path => request.paths.push(PathBuf::from(path)),
        }
    }
    Ok(Args {
        request,
        baseline,
        dump_types,
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
    let config = check_project::load_config(&args.request)?;
    let run = check_project::prepare(&args.request, config, phpstan_rs::project::worker_pool())?;
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
    write(&out.join(format!("{name}.err")), &err)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}
