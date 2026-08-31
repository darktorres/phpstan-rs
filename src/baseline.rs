//! Phase 11 Stage 6's baseline mechanism, a simplified version of real
//! PHPStan's own `baseline.neon`: `--generate-baseline` records every
//! current diagnostic into a file; a later run passed `--baseline` filters
//! out anything already recorded, so existing debt doesn't block CI while
//! new issues are still caught.
//!
//! Keyed by `(file, location, message)` - deliberately not by line/column,
//! so a baselined finding survives incidental code shifts elsewhere in the
//! same file (matching real PHPStan's own rationale for not just diffing
//! raw diagnostic output). `location` alone isn't enough: many diagnostics
//! share the generic `"<main>"` label for top-level script code across
//! different files, so the file path has to be part of the key too.
//!
//! Stored as JSON via `serde_json` (already a real dependency from the LSP
//! work, so this needed no new one) rather than a `.neon` file - this
//! project doesn't have a NEON parser and one isn't otherwise justified.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::checker::Diagnostic;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct Entry {
    file: String,
    location: String,
    message: String,
}

/// Finding 32: a baseline entry's own `file` key was built directly from
/// whatever `PathBuf` the current run happened to produce - two
/// invocations that check the exact same real file but spell its path
/// differently (`check-project .` vs `check-project ./.`, or a config
/// file's own `paths` entry vs an equivalent positional CLI argument)
/// produced two different keys for the same diagnostic, silently making
/// every previously-baselined finding reappear as "new". Lexically
/// resolves `.`/`..` segments and redundant separators - deliberately
/// not `fs::canonicalize`, which would fail outright for a path that no
/// longer exists (checking a file, baselining it, then later reporting
/// against a moved/renamed baseline) and could also resolve symlinks
/// differently across machines/checkouts, neither of which this is
/// trying to solve.
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match result.components().next_back() {
                Some(std::path::Component::Normal(_)) => {
                    result.pop();
                }
                _ => result.push(".."),
            },
            other => result.push(other.as_os_str()),
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

/// Writes every diagnostic in `results` to `path` as a baseline file.
/// Sorted (by file, then location, then message) so regenerating an
/// unchanged codebase produces a byte-identical file - real HashMap
/// iteration order would otherwise make every regeneration a spurious
/// diff, defeating the "commit the baseline file" workflow this exists
/// for. Returns the number of entries written.
pub fn generate(path: &Path, results: &HashMap<PathBuf, Vec<Diagnostic>>) -> io::Result<usize> {
    let mut entries: Vec<Entry> = results
        .iter()
        .flat_map(|(file, diagnostics)| {
            let file = normalize_path(file).display().to_string();
            diagnostics.iter().map(move |d| Entry {
                file: file.clone(),
                location: d.location.clone(),
                message: d.message.clone(),
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        (&a.file, &a.location, &a.message).cmp(&(&b.file, &b.location, &b.message))
    });
    let count = entries.len();
    let json =
        serde_json::to_string_pretty(&entries).expect("Entry is always representable as JSON");
    fs::write(path, json)?;
    Ok(count)
}

/// A loaded baseline - opaque outside this module, exposing only
/// `contains` for filtering.
pub struct Baseline(HashSet<Entry>);

impl Baseline {
    pub fn load(path: &Path) -> io::Result<Baseline> {
        let json = fs::read_to_string(path)?;
        let entries: Vec<Entry> = serde_json::from_str(&json)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Baseline(entries.into_iter().collect()))
    }

    fn contains(&self, file: &Path, d: &Diagnostic) -> bool {
        self.0.contains(&Entry {
            file: normalize_path(file).display().to_string(),
            location: d.location.clone(),
            message: d.message.clone(),
        })
    }
}

/// Drops every diagnostic already present in `baseline` from `results`,
/// in place - new diagnostics (not in the baseline) are kept, and files
/// whose diagnostics are now empty stay in the map (a file with zero
/// diagnostics is still real information: "this file was checked").
pub fn filter(results: &mut HashMap<PathBuf, Vec<Diagnostic>>, baseline: &Baseline) {
    for (file, diagnostics) in results.iter_mut() {
        diagnostics.retain(|d| !baseline.contains(file, d));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::{Position, Span};

    fn diag(location: &str, message: &str) -> Diagnostic {
        Diagnostic {
            location: location.to_string(),
            message: message.to_string(),
            span: Span {
                start: Position { line: 1, col: 1 },
                end: Position { line: 1, col: 1 },
            },
            level: 0,
        }
    }

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "phpstan_rs_baseline_test_{label}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn generate_then_load_filters_out_every_recorded_diagnostic() {
        let mut results = HashMap::new();
        results.insert(
            PathBuf::from("a.php"),
            vec![
                diag("Foo::bar", "undefined variable $x"),
                diag("Foo::baz", "undefined variable $y"),
            ],
        );
        results.insert(
            PathBuf::from("b.php"),
            vec![diag("<main>", "undefined variable $z")],
        );

        let path = temp_path("roundtrip");
        let count = generate(&path, &results).unwrap();
        assert_eq!(count, 3);

        let baseline = Baseline::load(&path).unwrap();
        filter(&mut results, &baseline);
        assert!(results[&PathBuf::from("a.php")].is_empty());
        assert!(results[&PathBuf::from("b.php")].is_empty());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn filter_keeps_a_new_diagnostic_not_in_the_baseline() {
        let mut baselined = HashMap::new();
        baselined.insert(
            PathBuf::from("a.php"),
            vec![diag("Foo::bar", "undefined variable $x")],
        );
        let path = temp_path("keep_new");
        generate(&path, &baselined).unwrap();
        let baseline = Baseline::load(&path).unwrap();

        let mut results = HashMap::new();
        results.insert(
            PathBuf::from("a.php"),
            vec![
                diag("Foo::bar", "undefined variable $x"),
                diag("Foo::bar", "undefined variable $newOne"),
            ],
        );
        filter(&mut results, &baseline);
        assert_eq!(results[&PathBuf::from("a.php")].len(), 1);
        assert!(
            results[&PathBuf::from("a.php")][0]
                .message
                .contains("$newOne")
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn the_same_location_and_message_in_a_different_file_is_not_baselined() {
        // `<main>`'s own generic label is shared across every file's
        // top-level script diagnostics - the file path has to be part of
        // the key, or baselining one file's finding would silently
        // suppress an unrelated file's identical-looking one too.
        let mut baselined = HashMap::new();
        baselined.insert(
            PathBuf::from("a.php"),
            vec![diag("<main>", "undefined variable $x")],
        );
        let path = temp_path("cross_file");
        generate(&path, &baselined).unwrap();
        let baseline = Baseline::load(&path).unwrap();

        let mut results = HashMap::new();
        results.insert(
            PathBuf::from("b.php"),
            vec![diag("<main>", "undefined variable $x")],
        );
        filter(&mut results, &baseline);
        assert_eq!(results[&PathBuf::from("b.php")].len(), 1);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn generating_twice_over_the_same_results_produces_a_byte_identical_file() {
        let mut results = HashMap::new();
        results.insert(
            PathBuf::from("z.php"),
            vec![diag("Z::m", "one"), diag("Z::m", "two")],
        );
        results.insert(PathBuf::from("a.php"), vec![diag("A::m", "three")]);

        let path = temp_path("deterministic");
        generate(&path, &results).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        generate(&path, &results).unwrap();
        let second = fs::read_to_string(&path).unwrap();
        assert_eq!(first, second);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_differently_spelled_but_identical_path_is_still_matched() {
        let mut baselined = HashMap::new();
        baselined.insert(
            PathBuf::from("./t.php"),
            vec![diag("f", "undefined variable $x")],
        );
        let path = temp_path("path_spelling");
        generate(&path, &baselined).unwrap();
        let baseline = Baseline::load(&path).unwrap();

        let mut results = HashMap::new();
        results.insert(
            PathBuf::from("././t.php"),
            vec![diag("f", "undefined variable $x")],
        );
        filter(&mut results, &baseline);
        assert!(results[&PathBuf::from("././t.php")].is_empty());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn normalize_path_resolves_dot_and_dotdot_segments() {
        assert_eq!(normalize_path(Path::new("./t.php")), PathBuf::from("t.php"));
        assert_eq!(
            normalize_path(Path::new("././t.php")),
            PathBuf::from("t.php")
        );
        assert_eq!(
            normalize_path(Path::new("src/../t.php")),
            PathBuf::from("t.php")
        );
        assert_eq!(normalize_path(Path::new(".")), PathBuf::from("."));
        assert_eq!(normalize_path(Path::new("./.")), PathBuf::from("."));
    }
}
