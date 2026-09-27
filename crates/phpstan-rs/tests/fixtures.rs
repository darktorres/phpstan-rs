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
//! An expectation lists one diagnostic per entry, or is the single line
//! `no diagnostics`; an empty file is not an expectation.
//!
//! `PHPSTAN_RS_BLESS=1 cargo test --test fixtures` rewrites every
//! expectation from the current output. `PHPSTAN_RS_DUMP_TYPES=<dir>`
//! also writes each fixture's `--dump-types` output to `<dir>`, its paths
//! relative to the fixture and each line prefixed with the fixture's name.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use config::ConfigFile;
use diagnostics::Diagnostic;
use libtest_mimic::{Arguments, Failed, Trial};
use phpstan_rs::app::config_schema;
use phpstan_rs::project::session::Session;
use phpstan_rs::project::worker_pool;

fn main() {
    let args = Arguments::from_args();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    // One pool for the whole suite: every fixture's parse and check runs
    // on it, alongside the fixtures libtest-mimic runs in parallel.
    let pool = worker_pool();
    let mut trials = Vec::new();
    collect(&root, &root, &pool, &mut trials);
    trials.sort_by(|a, b| a.name().cmp(b.name()));
    libtest_mimic::run(&args, trials).exit();
}

fn collect(root: &Path, dir: &Path, pool: &Arc<rayon::ThreadPool>, trials: &mut Vec<Trial>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // A multi-file fixture's own directory holds no fixtures.
            if !path.with_extension("expect").is_file() {
                collect(root, &path, pool, trials);
            }
        } else if path.extension().is_some_and(|e| e == "expect") {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .with_extension("")
                .display()
                .to_string();
            let pool = Arc::clone(pool);
            trials.push(Trial::test(name.clone(), move || {
                run_fixture(&name, &path, pool)
            }));
        }
    }
}

fn run_fixture(name: &str, expect_file: &Path, pool: Arc<rayon::ThreadPool>) -> Result<(), Failed> {
    let stem = expect_file.with_extension("");
    let config = fixture_config(&stem.with_extension("toml"))?;
    let (session, base) = Session::for_fixture(&stem, &config, pool)?;
    let single_file = stem.with_extension("php").is_file();
    let dump = std::env::var_os("PHPSTAN_RS_DUMP_TYPES")
        .map(|dir| PathBuf::from(dir).join(name.replace('/', "__")));
    let analysis = session.run(dump.is_some())?;
    if let (Some(dump), Some(text)) = (&dump, &analysis.type_dump) {
        write_type_dump(name, &base, text, &dump.with_extension("types"))?;
    }
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
    let report = analysis.finalize(&session.finalize_options());
    let actual = render(&report.results, &base, single_file);
    if std::env::var_os("PHPSTAN_RS_BLESS").is_some() {
        std::fs::write(expect_file, &actual)?;
        return Ok(());
    }
    let expected = std::fs::read_to_string(expect_file)?;
    if expected.is_empty() {
        return Err(format!(
            "{} is empty; write `{NO_DIAGNOSTICS}` for a fixture that expects none",
            expect_file.display()
        )
        .into());
    }
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

/// The fixture's config, `path` when it exists, checked against the
/// application's schema. An old key a schema moves is an error here: a
/// fixture is written in the current layout.
fn fixture_config(path: &Path) -> Result<ConfigFile, Failed> {
    if !path.is_file() {
        return Ok(ConfigFile::empty());
    }
    let schema = config_schema();
    let config = ConfigFile::load(path, &schema)?;
    config.validate(&schema)?;
    if let Some(warning) = config.warnings().first() {
        return Err(format!("{}: {warning}", path.display()).into());
    }
    Ok(config)
}

/// Writes `text`, the session's type dump, to `out`: each line's path
/// relative to `base` and prefixed with the fixture's name.
fn write_type_dump(name: &str, base: &Path, text: &str, out: &Path) -> Result<(), Failed> {
    let prefix = format!("{}/", base.display());
    let mut rewritten = String::new();
    for line in text.lines() {
        let line = line.strip_prefix(&prefix).unwrap_or(line);
        writeln!(rewritten, "{name}: {line}").unwrap();
    }
    std::fs::write(out, rewritten)?;
    Ok(())
}

const NO_DIAGNOSTICS: &str = "no diagnostics";

/// One entry per diagnostic, files in path order, diagnostics in output
/// order, or `no diagnostics`. A single-file fixture omits the path.
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
                "{}:{}-{}:{} {} L{} | {}",
                s.start.line,
                s.start.col,
                s.end.line,
                s.end.col,
                d.identifier,
                d.level,
                one_line(&portable(&d.message, base)),
            )
            .unwrap();
            for tip in &d.tips {
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
    if out.is_empty() {
        out = format!("{NO_DIAGNOSTICS}\n");
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
