//! Directory listings, each made once: the vendor discovery reads the
//! directories its autoloader's entries are in, and the session's walk
//! reads the directories it lists files from, and many of them are the
//! same. A table lists a directory the first time either asks, and gives
//! the same listing to the other, whichever thread that is.

use rustc_hash::FxHashMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// What a directory entry is, as its directory's listing says it: without
/// following a link, which only a `stat` of the link itself settles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    /// Anything that is neither a directory nor a link.
    File,
    /// A link, or an entry whose type the system did not give.
    Link,
}

/// One entry of a listed directory.
#[derive(Debug)]
pub struct Entry {
    pub name: OsString,
    pub kind: EntryKind,
}

/// A directory's entries, sorted by the bytes of their names: a name is
/// before the longer ones it begins, and what is in a directory is before
/// what comes after the directory's own name, the order paths compare in.
#[derive(Debug, Default)]
pub struct Listing {
    entries: Vec<Entry>,
}

impl Listing {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The kind of the entry named `name`, if the directory has one.
    pub fn kind_of(&self, name: &std::ffi::OsStr) -> Option<EntryKind> {
        let bytes = name.as_encoded_bytes();
        self.entries
            .binary_search_by(|entry| entry.name.as_encoded_bytes().cmp(bytes))
            .ok()
            .map(|at| self.entries[at].kind)
    }
}

/// The listings made so far, by the directory's text as written, which
/// hashes as bytes where a `Path` hashes component by component. A
/// directory that cannot be read is recorded as such.
#[derive(Debug, Default)]
pub struct DirTable {
    listings: Mutex<FxHashMap<Vec<u8>, Option<Arc<Listing>>>>,
}

impl DirTable {
    /// The listing of `dir`, made now if no one has made it, or none for
    /// a directory that cannot be read. An entry the system fails to give
    /// is left out.
    pub fn list(&self, dir: &Path) -> Option<Arc<Listing>> {
        let key = dir.as_os_str().as_encoded_bytes();
        if let Some(found) = self.lock().get(key) {
            return found.clone();
        }
        // Two threads may list the same directory at once; the second's
        // listing is dropped for the first's.
        let listed = read(dir).map(Arc::new);
        self.lock().entry(key.to_vec()).or_insert(listed).clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FxHashMap<Vec<u8>, Option<Arc<Listing>>>> {
        self.listings.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn read(dir: &Path) -> Option<Listing> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| Entry {
            kind: match entry.file_type() {
                Ok(kind) if kind.is_dir() => EntryKind::Dir,
                Ok(kind) if kind.is_symlink() => EntryKind::Link,
                // A type the system does not give is a link until a
                // `stat` says otherwise.
                Err(_) => EntryKind::Link,
                Ok(_) => EntryKind::File,
            },
            name: entry.file_name(),
        })
        .collect();
    entries.sort_unstable_by(|a, b| a.name.as_encoded_bytes().cmp(b.name.as_encoded_bytes()));
    Some(Listing { entries })
}
