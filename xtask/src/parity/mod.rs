//! `cargo xtask parity`: the scoreboard (`docs/design/scoreboard.md`).
//!
//! Each suite extracts its expected cases from PHPStan's side (cached by
//! key), runs the same inputs through the `phpstan-rs` binary's hidden
//! `debug` subcommand, and compares the two case by case. A full run
//! rewrites `parity/score.json`; every run writes the per-case results under
//! the cache directory and prints a one-line summary.

mod digest;
mod oracle;
mod score;

use std::path::{Path, PathBuf};
use std::process::Command;

use oracle::Oracle;
use score::{CaseResult, Exclusions};

/// The suites, in the summary's order.
const SUITES: [&str; 2] = [digest::LEXER.name, digest::PARSER.name];

const USAGE: &str =
    "usage: cargo xtask parity [--suite <name>]... [--filter <glob>] [--show <case id>]";

struct Options {
    suites: Vec<String>,
    filter: Option<String>,
    show: Option<String>,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let options = parse_args(args)?;
    let oracle = Oracle::open(root)?;
    let binary = build_binary(root)?;
    if let Some(id) = &options.show {
        let [suite] = options.suites.as_slice() else {
            return Err("--show needs exactly one --suite".to_owned());
        };
        return match digest_suite(suite) {
            Some(digest) => digest::show(digest, &oracle, &binary, id),
            None => Err(format!("--show is not available for suite {suite}")),
        };
    }
    let exclusions = Exclusions::read(&root.join("parity/exclusions.md"))?;
    let mut results: Vec<CaseResult> = Vec::new();
    for suite in &options.suites {
        let cases = match digest_suite(suite) {
            Some(digest) => digest::run(digest, &oracle, &binary)?,
            None => unreachable!("suite names are checked when parsing"),
        };
        results.extend(score::judge(
            suite,
            cases,
            &exclusions,
            options.filter.as_deref(),
        ));
    }
    let partial = options.filter.is_some() || options.suites.len() < SUITES.len();
    let results_path = score::write_results(&oracle, root, &results, partial)?;
    let scores = score::tally(&results);
    if options.filter.is_none() {
        score::write_score(&root.join("parity/score.json"), &scores)?;
    }
    println!("{}", score::summary(&scores));
    println!("results: {}", results_path.display());
    Ok(())
}

fn digest_suite(name: &str) -> Option<&'static digest::Suite> {
    [&digest::LEXER, &digest::PARSER]
        .into_iter()
        .find(|suite| suite.name == name)
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        suites: Vec::new(),
        filter: None,
        show: None,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = || {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--suite" => {
                let suite = value()?;
                if !SUITES.contains(&suite.as_str()) {
                    return Err(format!(
                        "unknown suite {suite}; the suites are {}",
                        SUITES.join(", ")
                    ));
                }
                if !options.suites.contains(&suite) {
                    options.suites.push(suite);
                }
            }
            "--filter" => options.filter = Some(value()?),
            "--show" => options.show = Some(value()?),
            _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
        }
    }
    if options.suites.is_empty() {
        options.suites = SUITES.iter().map(|&s| s.to_owned()).collect();
    }
    Ok(options)
}

/// Builds the release `phpstan-rs` binary, so the scoreboard measures what
/// ships, and returns its path.
fn build_binary(root: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(root)
        .args(["build", "--quiet", "--release", "--package", "phpstan-rs"])
        .status()
        .map_err(|error| format!("cargo build: {error}"))?;
    if !status.success() {
        return Err(format!("cargo build of phpstan-rs failed: {status}"));
    }
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    Ok(target
        .join("release")
        .join(format!("phpstan-rs{}", std::env::consts::EXE_SUFFIX)))
}
