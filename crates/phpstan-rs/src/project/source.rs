//! Where a load reads a file's bytes from: an in-memory overlay (the
//! language server's open documents, the files a test or a benchmark
//! loads), and the disk for every other path.

use rustc_hash::FxHashMap;
use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The bytes a load reads, by path: the overlay's where it has a path, the
/// disk's elsewhere.
#[derive(Clone, Default)]
pub struct FileSource {
    overlay: FxHashMap<PathBuf, Arc<[u8]>>,
}

impl FileSource {
    /// Gives `path` the bytes `bytes`, in place of what the disk holds.
    pub fn set(&mut self, path: &Path, bytes: &[u8]) {
        self.overlay.insert(path.to_path_buf(), Arc::from(bytes));
    }

    /// Gives `path` back to the disk.
    pub fn unset(&mut self, path: &Path) {
        self.overlay.remove(path);
    }

    /// The bytes of `path`: the overlay's, else the disk's.
    pub fn read(&self, path: &Path) -> io::Result<Cow<'_, [u8]>> {
        match self.overlay.get(path) {
            Some(bytes) => Ok(Cow::Borrowed(&bytes[..])),
            None => std::fs::read(path).map(Cow::Owned),
        }
    }

    /// The bytes of `path` on the disk, whatever the overlay holds.
    pub fn read_disk(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

/// The digest that stands for a file's bytes: what a file's state keeps, and
/// what an edit compares against to find a document unchanged.
pub fn content_hash(bytes: &[u8]) -> u128 {
    let mut hasher = twox_hash::XxHash3_128::new();
    hasher.write(bytes);
    hasher.finish_128()
}
