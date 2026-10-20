//! `exclude_paths`: the paths a configuration leaves out of the analysis, or
//! out of the analysis and the scan. An entry is a path, or a pattern with `*`
//! that matches across directories as PHPStan's `excludePaths` do. A path
//! matches when it or one of its directories matches, so a directory's entry
//! leaves out what is below it.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use diagnostics::CaseRule;
use serde::Deserialize;

use super::configured_path;
use crate::ignore_errors::path_glob_matches;

/// One entry of `exclude_paths`: a path, or a table that marks the path as
/// one that may be missing.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Plain(String),
    Table {
        path: String,
        #[serde(default)]
        optional: bool,
    },
}

/// The `exclude_paths` setting: a flat list, which excludes from the analysis
/// and the scan, or a table with one list for each.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ExcludePaths {
    List(Vec<Entry>),
    Table {
        #[serde(default)]
        analyse: Vec<Entry>,
        #[serde(default)]
        analyse_and_scan: Vec<Entry>,
    },
}

impl Default for ExcludePaths {
    fn default() -> Self {
        ExcludePaths::List(Vec::new())
    }
}

/// The directories the editor leaves out wherever they are, and a degrade root
/// leaves out below its root: a version-control or package directory, named at
/// the top of a path or below one. Unanchored, so a link's target is matched as
/// well as a path written below the root.
const FLOOR: &[&str] = &[".git", "*/.git", "node_modules", "*/node_modules"];

/// The floor the editor checks every path no configuration lists against.
pub(crate) fn unlisted_floor() -> Arc<Exclusions> {
    static FLOOR_SET: OnceLock<Arc<Exclusions>> = OnceLock::new();
    Arc::clone(FLOOR_SET.get_or_init(|| Arc::new(Exclusions::default().with_floor())))
}

/// The exclusions of a configuration, resolved: the patterns that each list
/// names, as absolute text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Exclusions {
    /// Left out of the analysis. Their files are still scanned.
    analyse: Vec<String>,
    /// Left out of the analysis and of the scan.
    analyse_and_scan: Vec<String>,
}

impl Exclusions {
    /// The exclusions of `patterns` as they are written, unanchored: each leaves
    /// out the paths and directories it matches, from the analysis and the scan.
    #[cfg(test)]
    pub(crate) fn anywhere(patterns: impl IntoIterator<Item = String>) -> Exclusions {
        Exclusions {
            analyse: Vec::new(),
            analyse_and_scan: patterns.into_iter().collect(),
        }
    }

    /// `self` with the floor added to what it leaves out of the analysis and
    /// the scan.
    pub(crate) fn with_floor(mut self) -> Exclusions {
        self.analyse_and_scan
            .extend(FLOOR.iter().map(|text| text.to_string()));
        self
    }

    /// The exclusions `settings` names, its entries resolved against `base`
    /// (absolute), with `exists` telling whether a literal path is on disk.
    /// An entry that names nothing is an error, and the others still apply.
    pub(crate) fn resolve(
        settings: &ExcludePaths,
        base: &Path,
        exists: &dyn Fn(&Path) -> bool,
    ) -> (Exclusions, Vec<String>) {
        let mut exclusions = Exclusions::default();
        let mut errors = Vec::new();
        let (analyse, analyse_and_scan): (&[Entry], &[Entry]) = match settings {
            ExcludePaths::List(entries) => (&[], entries),
            ExcludePaths::Table {
                analyse,
                analyse_and_scan,
            } => (analyse, analyse_and_scan),
        };
        resolve_list(analyse, base, exists, &mut exclusions.analyse, &mut errors);
        resolve_list(
            analyse_and_scan,
            base,
            exists,
            &mut exclusions.analyse_and_scan,
            &mut errors,
        );
        (exclusions, errors)
    }

    /// Whether `path`, absolute, is left out of the analysis by either list,
    /// counting only the directories below `below`, when it is given.
    pub(crate) fn excludes_analysis(
        &self,
        path: &Path,
        case: CaseRule,
        below: Option<&Path>,
    ) -> bool {
        matches_any(&self.analyse, path, case, below)
            || matches_any(&self.analyse_and_scan, path, case, below)
    }

    /// Whether `path`, absolute, is left out of the scan too, counting only the
    /// directories below `below`, when it is given.
    pub(crate) fn excludes_scan(&self, path: &Path, case: CaseRule, below: Option<&Path>) -> bool {
        matches_any(&self.analyse_and_scan, path, case, below)
    }

    /// Whether no entry is named.
    pub(crate) fn is_empty(&self) -> bool {
        self.analyse.is_empty() && self.analyse_and_scan.is_empty()
    }

    /// Whether an entry leaves paths out of the analysis alone, so that their
    /// files are still scanned.
    pub(crate) fn has_analyse_only(&self) -> bool {
        !self.analyse.is_empty()
    }
}

/// Resolves `entries` into `out`. An entry that names nothing, or that
/// names two files, is pushed to `errors` instead.
fn resolve_list(
    entries: &[Entry],
    base: &Path,
    exists: &dyn Fn(&Path) -> bool,
    out: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    for entry in entries {
        let (text, optional) = match entry {
            Entry::Plain(text) => (text.as_str(), false),
            Entry::Table { path, optional } => (path.as_str(), *optional),
        };
        if text.is_empty() {
            errors.push("an exclude_paths entry is empty".to_string());
            continue;
        }
        let joined = base.join(text);
        // A pattern names no file the disk can be asked about, so it reads the
        // separator way.
        if text.contains('*') {
            out.push(diagnostics::slash_spelling(&joined.to_string_lossy()).into_owned());
            continue;
        }
        let reading = match configured_path::single("exclude_paths entry", &joined, exists) {
            Ok(reading) => reading,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        if exists(&reading) {
            out.push(reading.to_string_lossy().into_owned());
        } else if !optional {
            errors.push(format!("exclude_paths entry `{text}` does not exist"));
        }
    }
}

/// Whether `path`, or one of its directories below `below`, matches one of
/// `patterns`. A directory whose name is not UTF-8 matches none.
fn matches_any(patterns: &[String], path: &Path, case: CaseRule, below: Option<&Path>) -> bool {
    if patterns.is_empty() {
        return false;
    }
    path.ancestors()
        .take_while(|dir| below.is_none_or(|below| *dir != below && dir.starts_with(below)))
        .any(|dir| {
            if dir.to_str().is_none() {
                return false;
            }
            // The patterns are written with `/`, which a Windows path spells
            // with `\`; a report path is the spelling both are compared in.
            let dir = diagnostics::report_path(dir);
            patterns
                .iter()
                .any(|pattern| path_glob_matches(pattern, &dir, case))
        })
}
