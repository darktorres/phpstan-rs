//! An item's validation as one command (`cargo xtask validate [--step
//! <name>]...`): the `gate` and `oracle` builds, the workspace tests, the
//! runtime oracle against `php`, the pass with the `gate` build, recorded by
//! commit, the pass with the `oracle` build, and the comparison with the
//! previous snapshot, in that order. Each step runs under the process's own
//! low priority with the load noted, its progress is written for `cargo xtask
//! progress validate`, and its outcome goes to the summary
//! `.corpus-runs/gate/validate/<commit>.tsv`, one line per step: the
//! summary the ledger's V row records. A step that fails stops the run;
//! `--step` runs only the steps it names and keeps the others' lines, so
//! what failed runs again without what passed.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use super::pass::{self, PassSetup};
use super::profile::loadavg;
use super::progress::{Group, Progress, status_path};
use super::store::Store;
use super::{
    Timings, build_profile, cargo, compare, gate_dir, head_hash, require_clean_tree, run,
    test_timed, write,
};

/// The steps, in the order they run.
const STEPS: [&str; 6] = [
    "build",
    "tests",
    "runtime-oracle",
    "gate-pass",
    "oracle-pass",
    "compare",
];

/// One line of the summary.
#[derive(Clone, Debug, PartialEq)]
struct Outcome {
    step: String,
    passed: bool,
    seconds: f64,
    load: String,
}

pub(crate) fn validate_command(args: &[String]) -> Result<(), String> {
    let selected = selected_steps(args)?;
    require_clean_tree()?;
    let hash = head_hash()?;
    lower_priority()?;
    let mut timings = Timings::new();
    let mut progress = Progress::new(
        status_path(&gate_dir(), "validate"),
        "validate",
        vec![Group {
            label: "steps".into(),
            total: selected.len(),
            indent: 0,
        }],
    );
    let mut outcomes = read_summary(&hash)?;
    let mut result = Ok(());
    for step in STEPS.into_iter().filter(|step| selected.contains(step)) {
        progress.start(step);
        let load = loadavg();
        let started = Instant::now();
        let ran = run_step(step, &hash, &mut timings);
        let seconds = started.elapsed().as_secs_f64();
        progress.finish(
            step,
            &[0],
            ran.as_ref().err().map(|e| format!("{step}: {e}")),
        );
        outcomes.retain(|outcome| outcome.step != step);
        outcomes.push(Outcome {
            step: step.to_string(),
            passed: ran.is_ok(),
            seconds,
            load,
        });
        write_summary(&hash, &outcomes)?;
        if let Err(error) = ran {
            result = Err(format!("{step}: {error}"));
            break;
        }
    }
    progress.end(&result);
    timings.write(&hash, "validate")?;
    result?;
    eprintln!("validated {hash}: {}", summary_path(&hash).display());
    Ok(())
}

/// The steps the command line names, or all of them.
fn selected_steps(args: &[String]) -> Result<Vec<&'static str>, String> {
    let mut selected = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg != "--step" {
            return Err(format!("unknown argument {arg}"));
        }
        let name = args.next().ok_or("--step takes a step")?;
        let step = STEPS
            .into_iter()
            .find(|step| step == name)
            .ok_or_else(|| format!("no step {name:?}; the steps are {}", STEPS.join(", ")))?;
        selected.push(step);
    }
    if selected.is_empty() {
        selected = STEPS.to_vec();
    }
    Ok(selected)
}

/// Lowers this process's priority, which every job it starts inherits.
fn lower_priority() -> Result<(), String> {
    run(Command::new("renice")
        .args(["-n", "19", "-p"])
        .arg(std::process::id().to_string())
        .stdout(std::process::Stdio::null()))
}

fn run_step(step: &str, hash: &str, timings: &mut Timings) -> Result<(), String> {
    match step {
        "build" => timings.time("builds", |_| {
            build_profile("gate")?;
            build_profile("oracle")
        }),
        "tests" => timings.time("tests", |t| {
            test_timed(
                t,
                "",
                &["--workspace", "--lib", "--bins", "--tests", "--", "--quiet"],
            )
        }),
        "runtime-oracle" => timings.time("runtime oracle", |_| {
            run(cargo().args([
                "test",
                "-p",
                "phpstan-rs",
                "--profile",
                "gate",
                "--test",
                "runtime_oracle",
                "--quiet",
            ]))
        }),
        "gate-pass" => pass::run_pass(
            &PassSetup {
                full: true,
                fixtures: true,
                profile: "gate",
                hash: Some(hash.to_string()),
            },
            timings,
        ),
        "oracle-pass" => pass::run_pass(
            &PassSetup {
                full: true,
                fixtures: true,
                profile: "oracle",
                hash: None,
            },
            timings,
        ),
        "compare" => {
            if has_earlier_snapshot()? {
                compare(&["--full".to_string()])
            } else {
                eprintln!("no earlier snapshot: this is the first, compared with none");
                Ok(())
            }
        }
        other => Err(format!("no step {other}")),
    }
}

/// Whether a commit before HEAD has a snapshot to compare with.
fn has_earlier_snapshot() -> Result<bool, String> {
    let snapshots = Store::open()?.snapshots()?;
    let ancestors = super::git(&["rev-list", "HEAD~1"]).unwrap_or_default();
    Ok(ancestors
        .lines()
        .any(|ancestor| snapshots.contains_key(ancestor)))
}

fn summary_path(hash: &str) -> PathBuf {
    gate_dir().join("validate").join(format!("{hash}.tsv"))
}

fn write_summary(hash: &str, outcomes: &[Outcome]) -> Result<(), String> {
    write(&summary_path(hash), format_summary(outcomes).as_bytes())
}

/// The summary's lines, in the steps' order: `step`, `passed` or `failed`,
/// seconds, and the load when the step started.
fn format_summary(outcomes: &[Outcome]) -> String {
    let mut text = String::new();
    for step in STEPS {
        for outcome in outcomes.iter().filter(|outcome| outcome.step == step) {
            writeln!(
                text,
                "{}\t{}\t{:.1}\t{}",
                outcome.step,
                if outcome.passed { "passed" } else { "failed" },
                outcome.seconds,
                outcome.load
            )
            .unwrap();
        }
    }
    text
}

fn parse_summary(text: &str) -> Result<Vec<Outcome>, String> {
    text.lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [step, passed, seconds, load] = fields[..] else {
                return Err(format!("a summary line has four fields: {line:?}"));
            };
            Ok(Outcome {
                step: step.to_string(),
                passed: passed == "passed",
                seconds: seconds
                    .parse()
                    .map_err(|_| format!("not seconds: {seconds:?}"))?,
                load: load.to_string(),
            })
        })
        .collect()
}

fn read_summary(hash: &str) -> Result<Vec<Outcome>, String> {
    match std::fs::read_to_string(summary_path(hash)) {
        Ok(text) => parse_summary(&text),
        Err(_) => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(args: &[&str]) -> Result<Vec<&'static str>, String> {
        selected_steps(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn every_step_runs_unless_some_are_named() {
        assert_eq!(names(&[]).unwrap(), STEPS.to_vec());
        assert_eq!(
            names(&["--step", "compare", "--step", "tests"]).unwrap(),
            vec!["compare", "tests"]
        );
        assert!(names(&["--step", "nothing"]).is_err());
        assert!(names(&["--step"]).is_err());
        assert!(names(&["tests"]).is_err());
    }

    #[test]
    fn a_summary_is_written_in_the_steps_order_and_read_back() {
        let outcome = |step: &str, passed| Outcome {
            step: step.to_string(),
            passed,
            seconds: 12.5,
            load: "1.00 0.50 0.25".to_string(),
        };
        let outcomes = [outcome("gate-pass", true), outcome("tests", false)];
        let text = format_summary(&outcomes);
        assert_eq!(
            text,
            "tests\tfailed\t12.5\t1.00 0.50 0.25\ngate-pass\tpassed\t12.5\t1.00 0.50 0.25\n"
        );
        let mut read = parse_summary(&text).unwrap();
        read.sort_by_key(|o| o.step.clone());
        assert_eq!(
            read,
            vec![outcome("gate-pass", true), outcome("tests", false)]
        );
        assert!(parse_summary("tests\tpassed\n").is_err());
    }
}
