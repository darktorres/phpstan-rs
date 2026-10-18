//! A baseline mechanism, a simplified version of real PHPStan's own
//! `baseline.neon`: `--generate-baseline` records every
//! current diagnostic into a file; a later run passed `--baseline` filters
//! out anything already recorded, so existing debt doesn't block CI while
//! new issues are still caught.
//!
//! Keyed like PHPStan's: `(file, message)`, with the identifier and an
//! occurrence count - deliberately not by line/column, so a baselined
//! finding survives incidental code shifts elsewhere in the same file. The
//! file is spelled as PHPStan's baselines spell it, with `/` between its
//! names on every platform (`diagnostics::report_path`), so a baseline made
//! on one platform matches on another. An entry's file is read as
//! configured path text (`diagnostics::path_text_names`): a `\` in it is a
//! separator, so an entry written with one, by hand or on Windows, names
//! the file all the same.
//!
//! Stored as JSON rather than a `.neon` file - this project doesn't have a
//! NEON parser and one isn't otherwise justified. The format is versioned
//! (`VERSION`); a version-1 file (keyed by a symbolic `location` as well)
//! still loads, its entries merged per new key, and `convert` rewrites it.

use rustc_hash::FxHashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use diagnostics::{Diagnostic, path_text_names, report_path, slash_spelling};
use serde::{Deserialize, Serialize};

/// The current on-disk format version.
const VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct Entry {
    file: String,
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

/// A version-1 entry: keyed by a symbolic `location` (`Class::method`,
/// `<main>`) as well.
#[derive(Deserialize)]
struct EntryV1 {
    file: String,
    #[allow(dead_code)]
    location: String,
    message: String,
    #[serde(default)]
    identifier: Option<String>,
    #[serde(default = "one")]
    count: usize,
}

impl From<EntryV1> for Entry {
    fn from(e: EntryV1) -> Entry {
        Entry {
            file: e.file,
            message: e.message,
            identifier: e.identifier,
            count: e.count,
        }
    }
}

/// How a diagnostic finds its baseline entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchMode {
    /// `(file, message)`.
    #[default]
    Message,
    /// `(file, message)` first, then `(file, identifier)`, so an entry
    /// keeps matching after its message's wording changes.
    Identifier,
}

/// The on-disk shapes: entries, or an index of other baseline files
/// (paths relative to the index) plus optional entries of its own.
#[derive(Serialize, Deserialize)]
struct BaselineFile {
    version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    includes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<Entry>,
}

/// The version-1 shapes, which carried no version: a list of entries, or
/// an index.
#[derive(Deserialize)]
#[serde(untagged)]
enum BaselineFileV1 {
    Entries(Vec<EntryV1>),
    Index {
        includes: Vec<String>,
        #[serde(default)]
        entries: Vec<EntryV1>,
    },
}

/// Reads one baseline file, of either version, as the current shape.
fn read_file(path: &Path) -> io::Result<BaselineFile> {
    let json = fs::read_to_string(path)?;
    let invalid = |e: serde_json::Error| io::Error::new(io::ErrorKind::InvalidData, e);
    let value: serde_json::Value = serde_json::from_str(&json).map_err(invalid)?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(v) if v == u64::from(VERSION) => serde_json::from_value(value).map_err(invalid),
        Some(v) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported baseline version {v}"),
        )),
        None => {
            let (includes, entries) = match serde_json::from_value(value).map_err(invalid)? {
                BaselineFileV1::Entries(entries) => (Vec::new(), entries),
                BaselineFileV1::Index { includes, entries } => (includes, entries),
            };
            Ok(BaselineFile {
                version: VERSION,
                includes,
                entries: merge(entries.into_iter().map(Entry::from).collect()),
            })
        }
    }
}

/// Entries with the same key folded into one, their counts added, in
/// first-seen order.
fn merge(entries: Vec<Entry>) -> Vec<Entry> {
    let mut merged: FxHashMap<Entry, usize> = FxHashMap::default();
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
    order
        .into_iter()
        .map(|key| {
            let count = merged[&key];
            Entry { count, ..key }
        })
        .collect()
}

fn write_file(path: &Path, file: &BaselineFile) -> io::Result<()> {
    let json =
        serde_json::to_string_pretty(file).expect("a baseline is always representable as JSON");
    fs::write(path, json)
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

/// One entry per distinct `(file, message, identifier)`, with
/// its occurrence count, sorted so regenerating an unchanged codebase is
/// byte-identical.
fn entries_for(results: &FxHashMap<PathBuf, Vec<Diagnostic>>) -> Vec<Entry> {
    let mut counts: FxHashMap<Entry, usize> = FxHashMap::default();
    for (file, diagnostics) in results {
        let file = report_path(&normalize_path(file)).into_owned();
        for d in diagnostics {
            let key = Entry {
                file: file.clone(),
                message: d.message.clone(),
                identifier: (!d.identifier.is_empty()).then(|| d.identifier.to_string()),
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
        (&a.file, &a.message, &a.identifier).cmp(&(&b.file, &b.message, &b.identifier))
    });
    entries
}

fn write_entries(path: &Path, entries: &[Entry]) -> io::Result<()> {
    write_file(
        path,
        &BaselineFile {
            version: VERSION,
            includes: Vec::new(),
            entries: entries.to_vec(),
        },
    )
}

/// Writes every diagnostic in `results` to `path` as a baseline file and
/// returns the number of diagnostics recorded.
pub fn generate(path: &Path, results: &FxHashMap<PathBuf, Vec<Diagnostic>>) -> io::Result<usize> {
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
    results: &FxHashMap<PathBuf, Vec<Diagnostic>>,
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
    write_file(
        path,
        &BaselineFile {
            version: VERSION,
            includes,
            entries: Vec::new(),
        },
    )?;
    Ok(entries.iter().map(|e| e.count).sum())
}

/// A loaded baseline - opaque outside this module.
#[derive(Clone, PartialEq)]
pub struct Baseline {
    entries: Vec<Entry>,
    mode: MatchMode,
}

type MessageKey = (String, String);
type IdentifierKey = (String, String);

impl Baseline {
    /// Reads a baseline file, following an index's `includes`. Entries
    /// written without a count (and repeated) add up.
    pub fn load(path: &Path) -> io::Result<Baseline> {
        let mut entries = Vec::new();
        load_into(path, &mut entries, 0)?;
        Ok(Baseline {
            entries: merge(entries),
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
    let file = read_file(path)?;
    out.extend(file.entries);
    let base = path.parent().unwrap_or(Path::new(""));
    for include in file.includes {
        load_into(&base.join(include), out, depth + 1)?;
    }
    Ok(())
}

/// Rewrites the baseline file at `path`, and every file it includes, in
/// the current format, keeping the include structure. Returns how many
/// files were rewritten.
pub fn convert(path: &Path) -> io::Result<usize> {
    convert_into(path, 0)
}

fn convert_into(path: &Path, depth: usize) -> io::Result<usize> {
    if depth > 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "baseline includes nest too deeply",
        ));
    }
    let file = read_file(path)?;
    write_file(path, &file)?;
    let base = path.parent().unwrap_or(Path::new(""));
    let mut converted = 1;
    for include in &file.includes {
        converted += convert_into(&base.join(include), depth + 1)?;
    }
    Ok(converted)
}

/// Drops every diagnostic `baseline` absorbs from `results`, each entry
/// absorbing at most its recorded count; a diagnostic beyond that stays.
///
/// Returns the entries that absorbed fewer diagnostics than recorded,
/// formatted `file: message` (with the expected and actual
/// counts when some but not all occurred) - so an entry left behind by
/// fixed code is reported instead of silently masking nothing.
pub fn filter(
    results: &mut FxHashMap<PathBuf, Vec<Diagnostic>>,
    baseline: &Baseline,
) -> Vec<String> {
    // Keyed by the files' slash spelling, which an entry's file shares with
    // every file it names; a candidate is then confirmed by its own text.
    let mut by_message: FxHashMap<MessageKey, Vec<usize>> = FxHashMap::default();
    let mut by_identifier: FxHashMap<IdentifierKey, Vec<usize>> = FxHashMap::default();
    for (i, e) in baseline.entries.iter().enumerate() {
        let file = slash_spelling(&e.file).into_owned();
        by_message
            .entry((file.clone(), e.message.clone()))
            .or_default()
            .push(i);
        if let Some(id) = &e.identifier {
            by_identifier.entry((file, id.clone())).or_default().push(i);
        }
    }
    let mut remaining: Vec<usize> = baseline.entries.iter().map(|e| e.count).collect();
    let mut files: Vec<&PathBuf> = results.keys().collect();
    files.sort();
    let files: Vec<PathBuf> = files.into_iter().cloned().collect();
    for file in files {
        let reported = report_path(&normalize_path(&file)).into_owned();
        let key = slash_spelling(&reported).into_owned();
        let Some(diagnostics) = results.get_mut(&file) else {
            continue;
        };
        diagnostics.retain(|d| {
            let mut take = |candidates: Option<&Vec<usize>>| -> bool {
                let Some(slot) = candidates.into_iter().flatten().copied().find(|&i| {
                    remaining[i] > 0 && path_text_names(&baseline.entries[i].file, &reported)
                }) else {
                    return false;
                };
                remaining[slot] -= 1;
                true
            };
            let by_text = by_message.get(&(key.clone(), d.message.clone()));
            if take(by_text) {
                return false;
            }
            if baseline.mode == MatchMode::Identifier
                && take(by_identifier.get(&(key.clone(), d.identifier.to_string())))
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
                format!("{}: {}", e.file, e.message)
            } else {
                format!(
                    "{}: {} (expected {} occurrence(s), found {occurred})",
                    e.file, e.message, e.count
                )
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
