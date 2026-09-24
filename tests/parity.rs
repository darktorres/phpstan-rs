//! The upstream parity run: phpstan-rs over PHPStan's own test data, as
//! recorded from its test suite by `cargo xtask parity` (one JSON object
//! per line in the file `$PHPSTAN_RS_PARITY_RECORDS` names).
//!
//! A `types` record is one file a type-inference test checks: every
//! `assertType()` in it holds under PHPStan. A `rule` record is one
//! `RuleTestCase::analyse()` call: its files and the errors (message,
//! line) PHPStan's rule reports there. Each record becomes one line on
//! stdout, sorted:
//!
//! - `types<TAB>test<TAB>file<TAB>asserts<TAB>failed<TAB>status`
//! - `rule<TAB>test<TAB>rule<TAB>expected<TAB>exact<TAB>line<TAB>missing<TAB>status`
//!
//! where `exact` counts expected errors phpstan-rs reports with the same
//! line and message, `line` those it reports something else for on that
//! line, and `status` is `ok`, `parse` (a file failed to parse), `panic`
//! or `error`. Paths are relative to `vendor/phpstan-src`.

use std::path::{Path, PathBuf};

use phpstan_rs::checker::Diagnostic;
use phpstan_rs::config::Config;
use phpstan_rs::project::session::{Analysis, Overrides, Session};

fn main() {
    let records = std::env::var_os("PHPSTAN_RS_PARITY_RECORDS")
        .expect("PHPSTAN_RS_PARITY_RECORDS names the recorded upstream tests");
    let text = std::fs::read_to_string(&records).expect("the records file is readable");
    let upstream = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/phpstan-src");
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let record: serde_json::Value = serde_json::from_str(l).expect("a record is JSON");
            evaluate(&record, &upstream)
        })
        .collect();
    lines.sort();
    lines.dedup();
    for line in lines {
        println!("{line}");
    }
}

fn evaluate(record: &serde_json::Value, upstream: &Path) -> String {
    let test = one_field(record["test"].as_str().unwrap_or(""));
    let files: Vec<PathBuf> = record["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str().map(PathBuf::from))
        .collect();
    let analysis = analyse(&files);
    let status = match &analysis {
        Err(_) => "error",
        Ok(a) if !a.check_panics.is_empty() => "panic",
        Ok(a) if a.skipped_analyze > 0 => "parse",
        Ok(_) => "ok",
    };
    let diagnostics: Vec<&Diagnostic> = analysis
        .as_ref()
        .map(|a| a.results.values().flatten().collect())
        .unwrap_or_default();
    if record["kind"] == "types" {
        let file = files.first().cloned().unwrap_or_default();
        let source = std::fs::read_to_string(&file).unwrap_or_default();
        let asserts = source.matches("assertType(").count();
        let failed = diagnostics
            .iter()
            .filter(|d| d.identifier == "phpstanRs.assertType")
            .count();
        let shown = file.strip_prefix(upstream).unwrap_or(&file).display();
        return format!("types\t{test}\t{shown}\t{asserts}\t{failed}\t{status}");
    }
    let rule = one_field(record["rule"].as_str().unwrap_or(""));
    let expected = record["expected"].as_array().cloned().unwrap_or_default();
    let mut unused: Vec<&Diagnostic> = diagnostics.clone();
    let (mut exact, mut on_line, mut missing) = (0, 0, 0);
    let mut lined = Vec::new();
    for e in &expected {
        let line = e["line"]
            .as_u64()
            .and_then(|l| u32::try_from(l).ok())
            .unwrap_or(0);
        let message = e["message"].as_str().unwrap_or("");
        if let Some(i) = unused
            .iter()
            .position(|d| d.span.start.line == line && d.message == message)
        {
            unused.remove(i);
            exact += 1;
        } else {
            lined.push(line);
        }
    }
    for line in lined {
        if let Some(i) = unused.iter().position(|d| d.span.start.line == line) {
            unused.remove(i);
            on_line += 1;
        } else {
            missing += 1;
        }
    }
    format!(
        "rule\t{test}\t{rule}\t{}\t{exact}\t{on_line}\t{missing}\t{status}",
        expected.len()
    )
}

/// The files analysed together, every level reported, from their common
/// directory with composer discovery off.
fn analyse(files: &[PathBuf]) -> Result<Analysis, String> {
    let root = common_dir(files).ok_or("no files")?;
    let config = Config {
        paths: Some(
            files
                .iter()
                .map(|f| f.strip_prefix(&root).unwrap_or(f).to_path_buf())
                .collect(),
        ),
        no_composer: Some(true),
        ..Config::default()
    };
    let overrides = Overrides {
        report_level: Some(u8::MAX),
        ..Overrides::default()
    };
    Session::resolve(config, None, root, overrides)?.run(None)
}

fn common_dir(files: &[PathBuf]) -> Option<PathBuf> {
    let mut dir = files.first()?.parent()?.to_path_buf();
    while !files.iter().all(|f| f.starts_with(&dir)) {
        dir = dir.parent()?.to_path_buf();
    }
    Some(dir)
}

fn one_field(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}
