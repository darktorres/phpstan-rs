//! The fixture suite: every fixture under `tests/fixtures` is analysed
//! through `project::session::Session`, exactly as `check-project` would,
//! and its diagnostics compared against the recorded expectation.
//!
//! A fixture is defined by `X.expect`. Its subject is `X.php`, or the
//! directory `X/` for a multi-file fixture; `X.toml`, when present, is its
//! `phpstan-rs.toml` (paths in it are relative to the subject's
//! directory). Without `paths`, the subject itself is analysed; without
//! `no_composer`, composer discovery is off. Its `requires_feature`, a
//! list of cargo features, skips the fixture in a build without one; its
//! `runtime_warnings` and `runtime_packages` are the runtime oracle's
//! (`tests/runtime_oracle.rs`).
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

use diagnostics::Diagnostic;
use libtest_mimic::{Arguments, Failed, Trial};
use phpstan_rs::gate::{self, FixtureConfig};
use phpstan_rs::project::worker_pool;
use rayon::prelude::*;
use rustc_hash::FxHashSet;

fn main() {
    let args = Arguments::from_args();
    // The stubs load while the fixtures are listed.
    phpstan_rs::project::Project::warm_stubs();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    // One pool for the whole suite: the listing, and every fixture's parse
    // and check, run on it, alongside the fixtures libtest-mimic runs in
    // parallel.
    let pool = worker_pool();
    let trials: Vec<Trial> = pool.install(|| {
        let mut listed = list(&root, "");
        // Names are unique, so the order is the same however it is sorted.
        listed.par_sort_unstable_by(|a, b| a.name.cmp(&b.name));
        listed
            .into_par_iter()
            .map(|fixture| {
                let ignored = fixture.has_config && requires_missing_feature(&fixture.expect);
                let pool = Arc::clone(&pool);
                let Fixture { name, expect, .. } = fixture;
                Trial::test(name.clone(), move || run_fixture(&name, &expect, pool))
                    .with_ignored_flag(ignored)
            })
            .collect()
    });
    libtest_mimic::run(&args, trials).exit();
}

/// A fixture found by `list`.
struct Fixture {
    /// Its path under the fixture root, without the extension.
    name: String,
    /// Its `.expect` file.
    expect: PathBuf,
    /// Whether a `.toml` sits beside it.
    has_config: bool,
}

/// Every fixture under `dir`, `prefix` being `dir`'s own name with the
/// separator, or empty at the root. One listing of each directory,
/// subdirectories listed in parallel.
fn list(dir: &Path, prefix: &str) -> Vec<Fixture> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stems = FxHashSet::default();
    let mut configs = FxHashSet::default();
    let mut directories = Vec::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if kind.is_dir() {
            directories.push(file_name.to_string());
        } else if let Some(stem) = file_name.strip_suffix(".expect") {
            stems.insert(stem.to_string());
        } else if let Some(stem) = file_name.strip_suffix(".toml") {
            configs.insert(stem.to_string());
        }
    }
    // A multi-file fixture's own directory holds no fixtures: it is named
    // like a listed `.expect` file.
    let nested: Vec<String> = directories
        .into_iter()
        .filter(|name| !stems.contains(name))
        .collect();
    let mut fixtures: Vec<Fixture> = stems
        .iter()
        .map(|stem| Fixture {
            name: format!("{prefix}{stem}"),
            expect: dir.join(format!("{stem}.expect")),
            has_config: configs.contains(stem),
        })
        .collect();
    let below: Vec<Vec<Fixture>> = nested
        .par_iter()
        .map(|name| list(&dir.join(name), &format!("{prefix}{name}/")))
        .collect();
    fixtures.extend(below.into_iter().flatten());
    fixtures
}

/// Whether the fixture whose expectation is `expect_file`, which has a
/// config, requires a cargo feature this build lacks. The config is read
/// in full only when it sets `requires_feature`.
fn requires_missing_feature(expect_file: &Path) -> bool {
    let config_file = expect_file.with_extension("toml");
    if !gate::declares_required_feature(&config_file) {
        return false;
    }
    match gate::fixture_config(&config_file) {
        Ok(config) => config
            .requires_feature
            .iter()
            .any(|f| !enabled(f).unwrap_or(true)),
        Err(_) => false,
    }
}

fn run_fixture(name: &str, expect_file: &Path, pool: Arc<rayon::ThreadPool>) -> Result<(), Failed> {
    let stem = expect_file.with_extension("");
    let config = fixture_config(&stem.with_extension("toml"))?;
    let (session, base) = gate::load_fixture(&stem, &config, pool)?;
    let single_file = stem.with_extension("php").is_file();
    let dump = std::env::var_os("PHPSTAN_RS_DUMP_TYPES")
        .map(|dir| PathBuf::from(dir).join(name.replace('/', "__")));
    let analysis = session.run(phpstan_rs::project::Records {
        types: dump.is_some(),
        expr_types: false,
        doc_resolutions: false,
    })?;
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

const FEATURES: &[(&str, bool)] = &[("pcre-validate", cfg!(feature = "pcre-validate"))];

/// Whether the cargo feature `name` is on in this build.
fn enabled(name: &str) -> Result<bool, String> {
    FEATURES
        .iter()
        .find(|(feature, _)| *feature == name)
        .map(|(_, on)| *on)
        .ok_or_else(|| format!("requires_feature: unknown feature `{name}`"))
}

/// The fixture's config, each feature it requires a known one.
fn fixture_config(path: &Path) -> Result<FixtureConfig, Failed> {
    let config = gate::fixture_config(path)?;
    for feature in &config.requires_feature {
        enabled(feature)?;
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
