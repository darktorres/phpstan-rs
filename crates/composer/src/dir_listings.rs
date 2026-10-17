//! The directory listings of one load. The vendor discovery and the walk of a
//! project read the same directories, so each directory is read once per
//! load, by whichever asks for it first, and every later ask gets that
//! listing.

use hashbrown::{HashTable, hash_table};
use std::ffi::{OsStr, OsString};
use std::fs::FileType;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::hashed_path::HashedPath;

/// The directories a load has listed, by resolved path.
#[derive(Default)]
pub struct DirListings {
    listed: Mutex<HashTable<(HashedPath, Slot)>>,
}

/// One directory's listing, read by the first ask. `None` when the directory
/// cannot be read.
type Slot = Arc<OnceLock<Option<Arc<Listing>>>>;

/// A directory's entries, sorted by name bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Whether every entry could be read. An entry that could not be is not in
    /// `entries`.
    pub complete: bool,
}

/// One entry of a directory.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: OsString,
    /// The entry's type, not following a link. `None` when the system gives no
    /// type, even to a `symlink_metadata`.
    pub kind: Option<FileType>,
}

impl DirListings {
    /// The listing of `dir`, a resolved directory, or `None` when it cannot be
    /// read. It is read once: a later ask, from this thread or another, gets
    /// the same listing.
    pub fn listing(&self, dir: &HashedPath) -> Option<Arc<Listing>> {
        let slot = {
            let mut listed = self.listed.lock().unwrap_or_else(PoisonError::into_inner);
            match listed.entry(dir.hash(), |(path, _)| path == dir, |(path, _)| path.hash()) {
                hash_table::Entry::Occupied(entry) => Arc::clone(&entry.get().1),
                hash_table::Entry::Vacant(entry) => {
                    let slot = Slot::default();
                    entry.insert((dir.clone(), Arc::clone(&slot)));
                    slot
                }
            }
        };
        slot.get_or_init(|| read_listing(dir.as_path()).map(Arc::new))
            .clone()
    }

    /// Forgets every listing. A listing already handed out stays valid for its
    /// holder.
    pub fn clear(&self) {
        self.listed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

impl Listing {
    /// The index of the entry called `name` in `entries`, if the directory has
    /// one that could be read.
    pub fn position(&self, name: &OsStr) -> Option<usize> {
        self.entries
            .binary_search_by(|entry| entry.name.as_os_str().cmp(name))
            .ok()
    }

    /// The entry called `name`, if the directory has one that could be read.
    pub fn entry(&self, name: &OsStr) -> Option<&Entry> {
        self.entries.get(self.position(name)?)
    }
}

fn read_listing(dir: &Path) -> Option<Listing> {
    let read = std::fs::read_dir(dir).ok()?;
    let mut complete = true;
    let mut entries = Vec::new();
    for entry in read {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        // The directory's own type first; the system is asked again only when
        // the directory gives none.
        let kind = entry
            .file_type()
            .or_else(|_| std::fs::symlink_metadata(entry.path()).map(|m| m.file_type()))
            .ok();
        entries.push(Entry {
            name: entry.file_name(),
            kind,
        });
    }
    entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    Some(Listing { entries, complete })
}
