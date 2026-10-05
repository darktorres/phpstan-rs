//! The code the build calls without its source: each function the program
//! declares and does not define, and each record a library's headers
//! define, by the library the header is from. A system header names the
//! Debian package installing it (the export's `header-packages.tsv`);
//! php-src's own headers name code the catalog does not read (C++, the
//! assembly of the fiber switch, a platform's code); the compiler's own
//! headers name its builtins.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::ir::{FnName, GlobalName};
use crate::program::Program;

pub(crate) type LibId = u32;

/// The package installing the C library's headers.
pub(crate) const LIBC: &str = "libc6-dev";
/// php-src's own code the catalog does not read.
pub(crate) const UNREAD: &str = "php-src";
/// A header no package installs and php-src does not ship.
pub(crate) const UNKNOWN: &str = "unknown";
/// The code the program loads while it runs (`dlopen`).
pub(crate) const LOADED: &str = "loaded";

pub(crate) struct Libraries {
    /// Each library's name: the package, or `UNREAD`, `UNKNOWN` or
    /// `LOADED`.
    pub(crate) names: Vec<Arc<str>>,
    /// The libraries declaring each function.
    pub(crate) functions: FxHashMap<FnName, Vec<LibId>>,
    /// The libraries declaring each global.
    pub(crate) globals: FxHashMap<GlobalName, Vec<LibId>>,
    /// The library defining each record its headers define.
    pub(crate) records: FxHashMap<Arc<str>, LibId>,
    pub(crate) libc: LibId,
    pub(crate) unknown: LibId,
    pub(crate) loaded: LibId,
}

impl Libraries {
    /// The libraries of the program's declarations, from a header map's
    /// text: a header's path and its package on each line.
    pub(crate) fn find(program: &Program, header_packages: &str) -> Libraries {
        let packages: FxHashMap<&str, &str> = header_packages
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect();
        let mut libraries = Libraries {
            names: vec![
                Arc::from(LIBC),
                Arc::from(UNREAD),
                Arc::from(UNKNOWN),
                Arc::from(LOADED),
            ],
            functions: FxHashMap::default(),
            globals: FxHashMap::default(),
            records: FxHashMap::default(),
            libc: 0,
            unknown: 2,
            loaded: 3,
        };
        let mut ids: FxHashMap<Arc<str>, LibId> = libraries
            .names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), i as LibId))
            .collect();
        let mut id = |name: &str, names: &mut Vec<Arc<str>>| -> LibId {
            if let Some(&id) = ids.get(name) {
                return id;
            }
            let name: Arc<str> = Arc::from(name);
            let id = names.len() as LibId;
            names.push(name.clone());
            ids.insert(name, id);
            id
        };
        let mut of_files = |files: &[Arc<str>], names: &mut Vec<Arc<str>>| -> Vec<LibId> {
            // A system header's declaration names the library; php-src's
            // own declaration of it is the project's copy of that one.
            let system = files.iter().any(|file| file.starts_with('/'));
            let mut found: Vec<LibId> = Vec::new();
            for file in files {
                if system && !file.starts_with('/') {
                    continue;
                }
                if let Some(library) = library_of(file, &packages) {
                    let library = id(library, names);
                    if !found.contains(&library) {
                        found.push(library);
                    }
                }
            }
            found
        };
        for (function, files) in &program.declared {
            let found = of_files(files, &mut libraries.names);
            if !found.is_empty() {
                libraries.functions.insert(function.clone(), found);
            }
        }
        for (global, (files, _)) in &program.declared_globals {
            let found = of_files(files, &mut libraries.names);
            if !found.is_empty() {
                libraries.globals.insert(global.clone(), found);
            }
        }
        for (record, file) in &program.record_files {
            if file.starts_with('/')
                && let Some(library) = library_of(file, &packages)
            {
                let library = id(library, &mut libraries.names);
                libraries.records.insert(record.clone(), library);
            }
        }
        libraries
    }
}

/// The library a declaring file is from; none for the compiler's own
/// headers, whose declarations are its builtins.
fn library_of<'p>(file: &str, packages: &FxHashMap<&str, &'p str>) -> Option<&'p str> {
    if !file.starts_with('/') {
        return Some(UNREAD);
    }
    if let Some(package) = packages.get(file) {
        return Some(package);
    }
    if is_compiler_header(file) {
        return None;
    }
    Some(UNKNOWN)
}

/// A header of the compiler's own, beside its builtins: clang's resource
/// directory.
fn is_compiler_header(file: &str) -> bool {
    file.contains("/lib/clang/") || file.contains("/lib/llvm-")
}
