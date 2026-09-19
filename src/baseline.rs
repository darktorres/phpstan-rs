//! A baseline mechanism, a simplified version of real PHPStan's own
//! `baseline.neon`: `--generate-baseline` records every
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

/// A baseline entry's own `file` key must not depend on whatever
/// `PathBuf` spelling the current run happened to produce - two
/// invocations that check the exact same real file but spell its path
/// differently (`check-project .` vs `check-project ./.`, or a config
/// file's own `paths` entry vs an equivalent positional CLI argument)
/// must still produce the same key. Lexically resolves `.`/`..`
/// segments and redundant separators - deliberately not
/// `fs::canonicalize`, which would fail outright for a path that no
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
#[derive(Clone, PartialEq)]
pub struct Baseline(HashSet<Entry>);

impl Baseline {
    pub fn load(path: &Path) -> io::Result<Baseline> {
        let json = fs::read_to_string(path)?;
        let entries: Vec<Entry> = serde_json::from_str(&json)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Baseline(entries.into_iter().collect()))
    }

    fn get<'a>(&'a self, file: &Path, d: &Diagnostic) -> Option<&'a Entry> {
        self.0.get(&Entry {
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
///
/// Returns every baseline entry this run never matched against any
/// current diagnostic, formatted `file:location: message` - mirrors
/// real PHPStan's own on-by-default `reportUnmatchedIgnoredErrors`, so
/// a baseline entry left behind by since-fixed/deleted code is
/// surfaced instead of silently masking nothing forever.
pub fn filter(results: &mut HashMap<PathBuf, Vec<Diagnostic>>, baseline: &Baseline) -> Vec<String> {
    let mut matched: HashSet<&Entry> = HashSet::new();
    for (file, diagnostics) in results.iter_mut() {
        diagnostics.retain(|d| match baseline.get(file, d) {
            Some(entry) => {
                matched.insert(entry);
                false
            }
            None => true,
        });
    }
    baseline
        .0
        .iter()
        .filter(|e| !matched.contains(e))
        .map(|e| format!("{}:{}: {}", e.file, e.location, e.message))
        .collect()
}

#[cfg(test)]
mod tests;
