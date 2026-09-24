//! `cargo xtask parity`: phpstan-rs over PHPStan's own test data.
//!
//! PHPStan's test suite (`vendor/phpstan-src`, with its development
//! dependencies installed) runs its rule and type-inference tests under
//! `xtask/parity/record.php`, which records what each test checks instead
//! of checking it. `tests/parity.rs` then analyses every recorded file set
//! with phpstan-rs. Records are cached per phpstan-src commit under
//! `target/parity`.
//!
//! The result is a tracked baseline, not a gate: `gates/parity/upstream.tsv`
//! holds the last blessed run, each run prints its totals next to the
//! baseline's and counts the lines that changed, and `--bless` rewrites it.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{capture, cargo, git, root, run, write};

fn parity_dir() -> PathBuf {
    root().join("target/parity")
}

fn upstream() -> PathBuf {
    root().join("vendor/phpstan-src")
}

fn baseline_file() -> PathBuf {
    root().join("gates/parity/upstream.tsv")
}

pub fn parity(args: &[String]) -> Result<(), String> {
    let bless = args.iter().any(|a| a == "--bless");
    let records = record()?;
    let out = capture(
        cargo()
            .args(["test", "--profile", "gate", "--test", "parity"])
            .env("PHPSTAN_RS_PARITY_RECORDS", &records),
    )?;
    let current = parity_dir().join("upstream.tsv");
    write(&current, out.as_bytes())?;
    let baseline = std::fs::read_to_string(baseline_file()).unwrap_or_default();
    let before: BTreeSet<&str> = baseline.lines().filter(|l| !l.starts_with('#')).collect();
    let after: BTreeSet<&str> = out.lines().collect();
    eprintln!("baseline: {}", totals(before.iter().copied()));
    eprintln!("current:  {}", totals(after.iter().copied()));
    eprintln!(
        "{} lines changed, {} unchanged; details: {}",
        before.symmetric_difference(&after).count(),
        before.intersection(&after).count(),
        current.display()
    );
    if bless {
        let mut text = String::from(
            "# phpstan-rs over PHPStan's own test data (see tests/parity.rs for\n# the columns). Written by `cargo xtask parity --bless`.\n",
        );
        text.push_str(&out);
        write(&baseline_file(), text.as_bytes())?;
        eprintln!("wrote {}", baseline_file().display());
    }
    Ok(())
}

/// The recorded upstream tests for the checked-out phpstan-src commit.
fn record() -> Result<PathBuf, String> {
    let commit = git(&["-C", "vendor/phpstan-src", "rev-parse", "HEAD"])?;
    let records = parity_dir().join(format!("records-{commit}.jsonl"));
    if records.is_file() {
        return Ok(records);
    }
    if !upstream().join("vendor/autoload.php").is_file() {
        run(Command::new("composer")
            .args(["install", "--no-interaction", "--no-progress"])
            .current_dir(upstream()))?;
    }
    let mut tests = Vec::new();
    collect_tests(&upstream().join("tests"), &mut tests);
    tests.sort();
    let mut config = format!(
        "<?xml version=\"1.0\"?>\n<phpunit bootstrap=\"{}\" cacheResult=\"false\" executionOrder=\"default\">\n\t<extensions>\n\t\t<bootstrap class=\"PHPStan\\Testing\\PHPUnit\\PHPStanPHPUnitExtension\"/>\n\t</extensions>\n\t<testsuites>\n\t\t<testsuite name=\"parity\">\n",
        root().join("xtask/parity/record.php").display()
    );
    for test in &tests {
        writeln!(config, "\t\t\t<file>{}</file>", test.display()).unwrap();
    }
    config.push_str("\t\t</testsuite>\n\t</testsuites>\n</phpunit>\n");
    let config_file = parity_dir().join("phpunit.xml");
    write(&config_file, config.as_bytes())?;
    let partial = records.with_extension("partial");
    let _ = std::fs::remove_file(&partial);
    // Tests whose data providers come back empty fail; only the records
    // matter.
    let status = Command::new("php")
        .args(["-d", "memory_limit=-1"])
        .arg(upstream().join("vendor/bin/phpunit"))
        .arg("-c")
        .arg(&config_file)
        .arg("--no-output")
        .env("PHPSTAN_RS_PARITY_RECORDS", &partial)
        .current_dir(upstream())
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("failed to start PHPUnit: {e}"))?;
    let recorded = std::fs::read_to_string(&partial).unwrap_or_default();
    if recorded.is_empty() {
        return Err(format!("PHPUnit ({status}) recorded nothing"));
    }
    eprintln!(
        "recorded {} upstream checks ({status})",
        recorded.lines().count()
    );
    std::fs::rename(&partial, &records).map_err(|e| format!("{}: {e}", records.display()))?;
    Ok(records)
}

/// Rule tests (`*RuleTest.php`) and type-inference tests under `dir`.
fn collect_tests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "data" && name != "nsrt" {
                collect_tests(&path, out);
            }
        } else if name.ends_with("Test.php")
            && (name.ends_with("RuleTest.php")
                || std::fs::read_to_string(&path)
                    .is_ok_and(|s| s.contains("extends TypeInferenceTestCase")))
        {
            out.push(path);
        }
    }
}

/// Totals over parity lines: type-inference asserts failed, and rule
/// errors matched exactly or by line.
fn totals<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    let (mut files, mut asserts, mut failed) = (0usize, 0usize, 0usize);
    let (mut calls, mut expected, mut exact, mut on_line) = (0usize, 0usize, 0usize, 0usize);
    let mut broken = 0usize;
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        if f.last().is_some_and(|s| *s != "ok") {
            broken += 1;
        }
        match f.first() {
            Some(&"types") => {
                files += 1;
                asserts += n(3);
                failed += n(4);
            }
            Some(&"rule") => {
                calls += 1;
                expected += n(3);
                exact += n(4);
                on_line += n(5);
            }
            _ => {}
        }
    }
    format!(
        "types {files} files, {} of {asserts} asserts hold; rules {calls} calls, {exact} of {expected} errors exact, {on_line} on the line; {broken} not ok",
        asserts.saturating_sub(failed)
    )
}
