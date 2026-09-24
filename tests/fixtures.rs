//! The fixture suite: every fixture under `tests/fixtures` is analysed
//! through `project::session::Session`, exactly as `check-project` would,
//! and its diagnostics compared against the recorded expectation.
//!
//! A fixture is defined by `X.expect`. Its subject is `X.php`, or the
//! directory `X/` for a multi-file fixture; `X.toml`, when present, is its
//! `phpstan-rs.toml` (paths in it are relative to the subject's
//! directory). Without `paths`, the subject itself is analysed; without
//! `no_composer`, composer discovery is off.
//!
//! `PHPSTAN_RS_BLESS=1 cargo test --test fixtures` rewrites every
//! expectation from the current output.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use libtest_mimic::{Arguments, Failed, Trial};
use phpstan_rs::checker::Diagnostic;
use phpstan_rs::project::session::Session;

fn main() {
    let args = Arguments::from_args();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut trials = Vec::new();
    collect(&root, &root, &mut trials);
    trials.sort_by(|a, b| a.name().cmp(b.name()));
    libtest_mimic::run(&args, trials).exit();
}

fn collect(root: &Path, dir: &Path, trials: &mut Vec<Trial>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // A multi-file fixture's own directory holds no fixtures.
            if !path.with_extension("expect").is_file() {
                collect(root, &path, trials);
            }
        } else if path.extension().is_some_and(|e| e == "expect") {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .with_extension("")
                .display()
                .to_string();
            trials.push(Trial::test(name, move || run_fixture(&path)));
        }
    }
}

fn run_fixture(expect_file: &Path) -> Result<(), Failed> {
    let stem = expect_file.with_extension("");
    let (session, base) = Session::for_fixture(&stem)?;
    let single_file = stem.with_extension("php").is_file();
    let analysis = session.run(None)?;
    if analysis.skipped_analyze > 0 {
        return Err(format!(
            "{} analysed file(s) failed to parse",
            analysis.skipped_analyze
        )
        .into());
    }
    if let Some((path, message)) = analysis.check_panics.first() {
        return Err(format!("{} panicked: {message}", path.display()).into());
    }
    let actual = render(&analysis.results, &base, single_file);
    if std::env::var_os("PHPSTAN_RS_BLESS").is_some() {
        std::fs::write(expect_file, &actual)?;
        return Ok(());
    }
    let expected = std::fs::read_to_string(expect_file)?;
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "diagnostics differ from {}:\n{}",
            expect_file.display(),
            diff(&expected, &actual)
        )
        .into())
    }
}

/// One entry per diagnostic, files in path order, diagnostics in output
/// order. A single-file fixture omits the path.
fn render(
    results: &rustc_hash::FxHashMap<PathBuf, Vec<Diagnostic>>,
    base: &Path,
    single_file: bool,
) -> String {
    let mut paths: Vec<&PathBuf> = results.keys().collect();
    paths.sort();
    let mut out = String::new();
    for path in paths {
        let relative = path
            .strip_prefix(base)
            .unwrap_or(path)
            .display()
            .to_string();
        for d in &results[path] {
            if !single_file {
                write!(out, "{relative}:").unwrap();
            }
            let s = &d.span;
            writeln!(
                out,
                "{}:{}-{}:{} {} L{} {} | {}",
                s.start.line,
                s.start.col,
                s.end.line,
                s.end.col,
                d.identifier,
                d.level,
                d.location,
                one_line(&portable(&d.message, base)),
            )
            .unwrap();
            if let Some(tip) = &d.tip {
                writeln!(out, "    tip: {}", one_line(&portable(tip, base))).unwrap();
            }
            if let Some(fix) = &d.fix {
                writeln!(
                    out,
                    "    fix: {} => {:?}",
                    one_line(&fix.description),
                    fix.replacement
                )
                .unwrap();
            }
            if d.non_ignorable {
                writeln!(out, "    non-ignorable").unwrap();
            }
        }
    }
    out
}

fn one_line(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

/// Messages that print absolute paths (e.g. an include's resolved path)
/// name the fixture's directory as `<fixture>` and its ancestors as
/// `<fixture>/..`, `<fixture>/../..`, so expectations don't depend on
/// where the repository is checked out.
fn portable(text: &str, base: &Path) -> String {
    let mut out = text.to_string();
    let mut placeholder = String::from("<fixture>");
    let mut dir = Some(base);
    while let Some(d) = dir {
        let spelled = d.display().to_string();
        if spelled.len() > 1 {
            out = out.replace(&spelled, &placeholder);
        }
        placeholder.push_str("/..");
        dir = d.parent();
    }
    out
}

fn diff(expected: &str, actual: &str) -> String {
    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    for line in &expected {
        if !actual.contains(line) {
            writeln!(out, "- {line}").unwrap();
        }
    }
    for line in &actual {
        if !expected.contains(line) {
            writeln!(out, "+ {line}").unwrap();
        }
    }
    if out.is_empty() {
        out.push_str("(same lines, different order)\n");
    }
    out
}
