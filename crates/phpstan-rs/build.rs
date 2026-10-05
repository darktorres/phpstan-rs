//! Sets `PHPSTAN_RS_SOURCE_HASH`: a hash of every workspace crate's
//! sources, build script and manifest, the lockfile and the vendored stub
//! sources - the
//! key the stub registry cache (`registry::stub_cache`) is stored under,
//! so a build whose code or stubs differ in any way never reads another's.
//!
//! Writes the builtin-runs table (`registry/data/builtin-runs-8.5.tsv`)
//! as static arrays sorted by name, `builtin_runs.rs` in `OUT_DIR`, which
//! `registry::effects` includes; a malformed line fails the build.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// One run of the table: the argument's position, whether the run is
/// through its elements, and the run as `registry::effects` builds it.
type Run = (u32, bool, String);

/// The table's sets and maps, each sorted by name.
#[derive(Default)]
struct Table {
    probed: Vec<String>,
    runs: BTreeMap<String, Vec<Run>>,
    holding: Vec<String>,
    held_wrappers: BTreeMap<String, Vec<String>>,
}

/// Reads the table, failing on a line that is not
/// `builtin<TAB>place<TAB>kind<TAB>detail`, lowercased: `probed` with no
/// place or detail; `calls` with no detail; `method` or `wrapper` with
/// one; a place an argument's position, `[]` after it for its elements,
/// and `new:` before it for one the object was made with, on a method.
fn read_table(path: &Path) -> Table {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let mut table = Table::default();
    for (number, line) in text.lines().enumerate() {
        let fail =
            |why: &str| -> ! { panic!("{}:{}: {why}: {line:?}", path.display(), number + 1) };
        let fields: Vec<&str> = line.split('\t').collect();
        let &[builtin, place, kind, detail] = fields.as_slice() else {
            fail("not four fields");
        };
        if builtin.is_empty() || builtin != builtin.to_ascii_lowercase() {
            fail("a builtin's name is lowercased");
        }
        if detail != detail.to_ascii_lowercase() {
            fail("a method's name is lowercased");
        }
        if kind == "probed" {
            if !place.is_empty() || !detail.is_empty() {
                fail("a probed line has no place or detail");
            }
            table.probed.push(builtin.to_owned());
            continue;
        }
        let run = match kind {
            "calls" if detail.is_empty() => "BuiltinRun::Calls".to_owned(),
            "method" if !detail.is_empty() => format!("BuiltinRun::Method({detail:?})"),
            "wrapper" if !detail.is_empty() => format!("BuiltinRun::Wrapper({detail:?})"),
            "calls" => fail("a calls line has no detail"),
            "method" | "wrapper" => fail("a method or wrapper line names its method"),
            _ => fail("the kind is probed, calls, method or wrapper"),
        };
        let (made, at) = match place.strip_prefix("new:") {
            Some(at) => (true, at),
            None => (false, place),
        };
        let (at, elements) = match at.strip_suffix("[]") {
            Some(at) => (at, true),
            None => (at, false),
        };
        let Ok(at) = at.parse::<u32>() else {
            fail("the place is an argument's position");
        };
        if made {
            let Some((class, _)) = builtin.split_once("::") else {
                fail("only a method's object is made with an argument");
            };
            if kind == "wrapper" {
                let held = table.held_wrappers.entry(class.to_owned()).or_default();
                if !held.iter().any(|method| method == detail) {
                    held.push(detail.to_owned());
                }
            } else {
                table.holding.push(class.to_owned());
            }
            continue;
        }
        table
            .runs
            .entry(builtin.to_owned())
            .or_default()
            .push((at, elements, run));
    }
    for names in [&mut table.probed, &mut table.holding] {
        names.sort_unstable();
        names.dedup();
    }
    table
}

/// Writes the table's arrays for `registry::effects` to include.
fn write_table(table: &Table, out: &Path) {
    let mut code = String::new();
    let _ = writeln!(code, "static PROBED: &[&str] = &[");
    for name in &table.probed {
        let _ = writeln!(code, "    {name:?},");
    }
    let _ = writeln!(
        code,
        "];\nstatic RUNS: &[(&str, &[(Position, BuiltinRun)])] = &["
    );
    for (name, runs) in &table.runs {
        let _ = write!(code, "    ({name:?}, &[");
        for (at, elements, run) in runs {
            let _ = write!(
                code,
                "(Position {{ at: {at}, elements: {elements} }}, {run}), "
            );
        }
        let _ = writeln!(code, "]),");
    }
    let _ = writeln!(code, "];\nstatic HOLDING: &[&str] = &[");
    for name in &table.holding {
        let _ = writeln!(code, "    {name:?},");
    }
    let _ = writeln!(code, "];\nstatic HELD_WRAPPERS: &[(&str, &[&str])] = &[");
    for (name, methods) in &table.held_wrappers {
        let _ = writeln!(code, "    ({name:?}, &{methods:?}),");
    }
    let _ = writeln!(code, "];");
    fs::write(out, code).unwrap_or_else(|e| panic!("writing {}: {e}", out.display()));
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let table = read_table(&manifest_dir.join("src/registry/data/builtin-runs-8.5.tsv"));
    write_table(&table, &out_dir.join("builtin_runs.rs"));
    let workspace = manifest_dir.join("../..");
    let mut inputs = vec![
        workspace.join("vendor/phpstorm-stubs"),
        workspace.join("vendor/phpstan-src/src"),
        workspace.join("Cargo.lock"),
        workspace.join("Cargo.toml"),
    ];
    let mut crates: Vec<PathBuf> = fs::read_dir(workspace.join("crates"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    crates.sort();
    for krate in crates {
        for part in ["src", "Cargo.toml", "build.rs", "index.tsv"] {
            let path = krate.join(part);
            if path.exists() {
                inputs.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for input in &inputs {
        println!("cargo::rerun-if-changed={}", input.display());
        if input.is_dir() {
            collect_files(input, &mut files);
        } else {
            files.push(input.clone());
        }
    }
    files.sort();
    let mut hasher = DefaultHasher::new();
    for path in &files {
        path.strip_prefix(&workspace)
            .unwrap_or(path)
            .hash(&mut hasher);
        if let Ok(bytes) = fs::read(path) {
            bytes.hash(&mut hasher);
        }
    }
    std::env::var("TARGET")
        .unwrap_or_default()
        .hash(&mut hasher);
    println!(
        "cargo::rustc-env=PHPSTAN_RS_SOURCE_HASH={:016x}",
        hasher.finish()
    );
}
