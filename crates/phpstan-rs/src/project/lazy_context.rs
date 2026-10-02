//! The vendor classes a run loads when something names them, and not all
//! of them up front.
//!
//! Composer's autoloader finds a class by its name: in the class map, or
//! under the directory of a PSR-4 prefix its name begins with. PHPStan
//! reads a vendored class the same way, when a name asks for it. A run
//! that parses every file of `vendor/` instead pays for the half of them
//! nothing refers to.
//!
//! The files a run loads are the closure of the names they hold: the
//! analysed files' names, and each loaded vendor file's, and the files the
//! autoloader finds those in. A file the autoloader would not find a name
//! in is loaded when it is one of the autoloader's `files` entries, or was
//! asked for as a scan directory, and not otherwise.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use composer::vendor::NamedAutoload;
use rustc_hash::FxHashMap;

/// The names of vendor classes and the files they are found in.
#[derive(Debug, Default, Clone)]
pub(crate) struct LazyContext {
    /// Each class map entry: the class name in lowercase, and its file.
    class_map: FxHashMap<String, Vec<PathBuf>>,
    /// Each PSR-4 prefix in lowercase, and one of its directories, by the
    /// first part of the prefix.
    psr4: FxHashMap<String, Vec<(String, PathBuf)>>,
}

impl LazyContext {
    /// Adds what `autoload` finds by name.
    pub(crate) fn extend(&mut self, autoload: &NamedAutoload) {
        for (class, file) in &autoload.class_map {
            let files = self
                .class_map
                .entry(class.trim_start_matches('\\').to_ascii_lowercase())
                .or_default();
            if !files.contains(file) {
                files.push(file.clone());
            }
        }
        for (prefix, dir) in &autoload.psr4 {
            let prefix = prefix.trim_start_matches('\\').to_ascii_lowercase();
            let first = prefix.split('\\').next().unwrap_or("").to_string();
            self.psr4
                .entry(first)
                .or_default()
                .push((prefix, dir.clone()));
        }
    }

    /// Keeps the files and directories `keep` holds for.
    pub(crate) fn retain(&mut self, keep: impl Fn(&Path) -> bool) {
        for files in self.class_map.values_mut() {
            files.retain(|f| keep(f));
        }
        self.class_map.retain(|_, files| !files.is_empty());
        for prefixes in self.psr4.values_mut() {
            prefixes.retain(|(_, dir)| keep(dir));
        }
        self.psr4.retain(|_, prefixes| !prefixes.is_empty());
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.class_map.is_empty() && self.psr4.is_empty()
    }

    /// Every file that holds the class `key`, a `dependency_key` (the name
    /// without its leading backslash, in lowercase): the class map's, and
    /// each PSR-4 directory whose prefix the name begins with, the name
    /// below it read as a path in any case, as a registry reads names.
    pub(crate) fn find(&self, key: &str, listings: &mut Listings) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = self.class_map.get(key).cloned().unwrap_or_default();
        let first = key.split('\\').next().unwrap_or("");
        // A prefix is a run of whole parts of the name, so it is among
        // those that begin with the name's first part, or has no part.
        for prefixes in [self.psr4.get(first), self.psr4.get("")]
            .into_iter()
            .flatten()
        {
            for (prefix, dir) in prefixes {
                let Some(rest) = key.strip_prefix(prefix.as_str()) else {
                    continue;
                };
                if rest.is_empty() {
                    continue;
                }
                if let Some(file) = listings.resolve(dir, rest)
                    && !found.contains(&file)
                {
                    found.push(file);
                }
            }
        }
        found
    }
}

/// The directories read while resolving PSR-4 names, each read once: its
/// entries by their lowercase names.
#[derive(Default)]
pub(crate) struct Listings {
    dirs: FxHashMap<PathBuf, Vec<(String, OsString, bool)>>,
}

impl Listings {
    /// The file `name` (a class name below a prefix, lowercase, with
    /// backslashes) is under `dir`, `Foo\Bar` being `dir/Foo/Bar.php`.
    fn resolve(&mut self, dir: &Path, name: &str) -> Option<PathBuf> {
        let mut at = dir.to_path_buf();
        let mut parts = name.split('\\').peekable();
        while let Some(part) = parts.next() {
            if part.is_empty() {
                return None;
            }
            let last = parts.peek().is_none();
            let wanted = if last {
                format!("{part}.php")
            } else {
                part.to_string()
            };
            let entries = self.dirs.entry(at.clone()).or_insert_with(|| list(&at));
            let (_, entry, _) = entries
                .iter()
                .find(|(lower, _, is_dir)| *lower == wanted && *is_dir != last)?;
            at.push(entry);
        }
        Some(at)
    }
}

fn list(dir: &Path) -> Vec<(String, OsString, bool)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let lower = name.to_str()?.to_ascii_lowercase();
            // A link is followed, as the walk that found every file did.
            let is_dir = std::fs::metadata(entry.path()).ok()?.is_dir();
            Some((lower, name, is_dir))
        })
        .collect()
}
