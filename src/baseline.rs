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

use std::collections::HashMap;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identifier: Option<String>,
    /// How many identical diagnostics this entry absorbs; more than that
    /// are reported again.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    count: usize,
}

fn one() -> usize {
    1
}

fn is_one(n: &usize) -> bool {
    *n == 1
}

/// How a diagnostic finds its baseline entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchMode {
    /// `(file, location, message)`.
    #[default]
    Message,
    /// `(file, location, message)` first, then `(file, location,
    /// identifier)`, so an entry keeps matching after its message's
    /// wording changes.
    Identifier,
}

/// The on-disk shapes: a list of entries, or an index of other baseline
/// files (paths relative to the index) plus optional entries of its own.
#[derive(Deserialize)]
#[serde(untagged)]
enum BaselineFile {
    Entries(Vec<Entry>),
    Index {
        includes: Vec<String>,
        #[serde(default)]
        entries: Vec<Entry>,
    },
}

#[derive(Serialize)]
struct IndexFile<'a> {
    includes: &'a [String],
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

/// One entry per distinct `(file, location, message, identifier)`, with
/// its occurrence count, sorted so regenerating an unchanged codebase is
/// byte-identical.
fn entries_for(results: &HashMap<PathBuf, Vec<Diagnostic>>) -> Vec<Entry> {
    let mut counts: HashMap<Entry, usize> = HashMap::new();
    for (file, diagnostics) in results {
        let file = normalize_path(file).display().to_string();
        for d in diagnostics {
            let key = Entry {
                file: file.clone(),
                location: d.location.clone(),
                message: d.message.clone(),
                identifier: (!d.identifier.is_empty()).then(|| d.identifier.clone()),
                count: 1,
            };
            *counts.entry(key).or_default() += 1;
        }
    }
    let mut entries: Vec<Entry> = counts
        .into_iter()
        .map(|(mut entry, count)| {
            entry.count = count;
            entry
        })
        .collect();
    entries.sort_by(|a, b| {
        (&a.file, &a.location, &a.message, &a.identifier).cmp(&(
            &b.file,
            &b.location,
            &b.message,
            &b.identifier,
        ))
    });
    entries
}

fn write_entries(path: &Path, entries: &[Entry]) -> io::Result<()> {
    let json =
        serde_json::to_string_pretty(entries).expect("Entry is always representable as JSON");
    fs::write(path, json)
}

/// Writes every diagnostic in `results` to `path` as a baseline file and
/// returns the number of diagnostics recorded.
pub fn generate(path: &Path, results: &HashMap<PathBuf, Vec<Diagnostic>>) -> io::Result<usize> {
    let entries = entries_for(results);
    write_entries(path, &entries)?;
    Ok(entries.iter().map(|e| e.count).sum())
}

/// `generate`, split into one file per identifier in a directory named
/// after `path`'s stem, with `path` itself an index including them all.
/// Stale `.json` files left in that directory by an earlier split are
/// removed.
pub fn generate_per_identifier(
    path: &Path,
    results: &HashMap<PathBuf, Vec<Diagnostic>>,
) -> io::Result<usize> {
    let entries = entries_for(results);
    let parent = path.parent().unwrap_or(Path::new(""));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "baseline".to_string());
    let dir = parent.join(&stem);
    fs::create_dir_all(&dir)?;
    for existing in fs::read_dir(&dir)?.flatten() {
        let p = existing.path();
        if p.extension().is_some_and(|e| e == "json") {
            fs::remove_file(p)?;
        }
    }
    let mut by_identifier: std::collections::BTreeMap<String, Vec<Entry>> = Default::default();
    for entry in &entries {
        let name = entry
            .identifier
            .clone()
            .filter(|id| {
                id.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
            })
            .unwrap_or_else(|| "_unidentified".to_string());
        by_identifier.entry(name).or_default().push(entry.clone());
    }
    let mut includes = Vec::new();
    for (name, group) in &by_identifier {
        let file_name = format!("{name}.json");
        write_entries(&dir.join(&file_name), group)?;
        includes.push(format!("{stem}/{file_name}"));
    }
    let json = serde_json::to_string_pretty(&IndexFile {
        includes: &includes,
    })
    .expect("an index is always representable as JSON");
    fs::write(path, json)?;
    Ok(entries.iter().map(|e| e.count).sum())
}

/// A loaded baseline - opaque outside this module.
#[derive(Clone, PartialEq)]
pub struct Baseline {
    entries: Vec<Entry>,
    mode: MatchMode,
}

type MessageKey = (String, String, String);
type IdentifierKey = (String, String, String);

impl Baseline {
    /// Reads a baseline file, following an index's `includes`. Entries
    /// written without a count (and repeated) add up.
    pub fn load(path: &Path) -> io::Result<Baseline> {
        let mut entries = Vec::new();
        load_into(path, &mut entries, 0)?;
        let mut merged: HashMap<Entry, usize> = HashMap::new();
        let mut order: Vec<Entry> = Vec::new();
        for entry in entries {
            let count = entry.count;
            let key = Entry { count: 1, ..entry };
            match merged.get_mut(&key) {
                Some(total) => *total += count,
                None => {
                    merged.insert(key.clone(), count);
                    order.push(key);
                }
            }
        }
        let entries = order
            .into_iter()
            .map(|key| {
                let count = merged[&key];
                Entry { count, ..key }
            })
            .collect();
        Ok(Baseline {
            entries,
            mode: MatchMode::Message,
        })
    }

    pub fn with_match_mode(mut self, mode: MatchMode) -> Baseline {
        self.mode = mode;
        self
    }
}

fn load_into(path: &Path, out: &mut Vec<Entry>, depth: usize) -> io::Result<()> {
    if depth > 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "baseline includes nest too deeply",
        ));
    }
    let json = fs::read_to_string(path)?;
    let file: BaselineFile =
        serde_json::from_str(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    match file {
        BaselineFile::Entries(entries) => out.extend(entries),
        BaselineFile::Index { includes, entries } => {
            out.extend(entries);
            let base = path.parent().unwrap_or(Path::new(""));
            for include in includes {
                load_into(&base.join(include), out, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// Drops every diagnostic `baseline` absorbs from `results`, each entry
/// absorbing at most its recorded count; a diagnostic beyond that stays.
///
/// Returns the entries that absorbed fewer diagnostics than recorded,
/// formatted `file:location: message` (with the expected and actual
/// counts when some but not all occurred) - so an entry left behind by
/// fixed code is reported instead of silently masking nothing.
pub fn filter(results: &mut HashMap<PathBuf, Vec<Diagnostic>>, baseline: &Baseline) -> Vec<String> {
    let mut by_message: HashMap<MessageKey, Vec<usize>> = HashMap::new();
    let mut by_identifier: HashMap<IdentifierKey, Vec<usize>> = HashMap::new();
    for (i, e) in baseline.entries.iter().enumerate() {
        by_message
            .entry((e.file.clone(), e.location.clone(), e.message.clone()))
            .or_default()
            .push(i);
        if let Some(id) = &e.identifier {
            by_identifier
                .entry((e.file.clone(), e.location.clone(), id.clone()))
                .or_default()
                .push(i);
        }
    }
    let mut remaining: Vec<usize> = baseline.entries.iter().map(|e| e.count).collect();
    let mut files: Vec<&PathBuf> = results.keys().collect();
    files.sort();
    let files: Vec<PathBuf> = files.into_iter().cloned().collect();
    for file in files {
        let normalized = normalize_path(&file).display().to_string();
        let Some(diagnostics) = results.get_mut(&file) else {
            continue;
        };
        diagnostics.retain(|d| {
            let mut take = |candidates: Option<&Vec<usize>>| -> bool {
                let Some(slot) = candidates
                    .into_iter()
                    .flatten()
                    .copied()
                    .find(|&i| remaining[i] > 0)
                else {
                    return false;
                };
                remaining[slot] -= 1;
                true
            };
            let by_text =
                by_message.get(&(normalized.clone(), d.location.clone(), d.message.clone()));
            if take(by_text) {
                return false;
            }
            if baseline.mode == MatchMode::Identifier
                && take(by_identifier.get(&(
                    normalized.clone(),
                    d.location.clone(),
                    d.identifier.clone(),
                )))
            {
                return false;
            }
            true
        });
    }
    baseline
        .entries
        .iter()
        .zip(&remaining)
        .filter(|(_, left)| **left > 0)
        .map(|(e, left)| {
            let occurred = e.count - left;
            if occurred == 0 {
                format!("{}:{}: {}", e.file, e.location, e.message)
            } else {
                format!(
                    "{}:{}: {} (expected {} occurrence(s), found {occurred})",
                    e.file, e.location, e.message, e.count
                )
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
