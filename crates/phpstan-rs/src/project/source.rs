//! Where a load reads a file's bytes from: the disk, except for the files
//! an overlay holds, which are read from it and exist whether or not the
//! disk has them. The language server's open documents are an overlay, and
//! so are a test's files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_hash::FxHashMap;

/// What a file's bytes are, to compare two reads of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash(u128);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> ContentHash {
        ContentHash(twox_hash::XxHash3_128::oneshot(bytes))
    }

    pub fn to_le_bytes(self) -> [u8; 16] {
        self.0.to_le_bytes()
    }

    /// The hash of a source, as the bytes it is.
    pub fn of_source(source: syntax::Source<'_>) -> ContentHash {
        match source {
            syntax::Source::Text(text) => ContentHash::of(text.as_bytes()),
            syntax::Source::Bytes(bytes) => ContentHash::of(bytes),
        }
    }
}

/// The files a read finds before it goes to the disk, by absolute
/// lexically normalized path.
#[derive(Clone, Default)]
pub struct Overlay {
    files: FxHashMap<PathBuf, Arc<[u8]>>,
}

impl Overlay {
    /// Holds `bytes` as the file at `path`, in place of what the disk has.
    pub fn set(&mut self, path: PathBuf, bytes: impl Into<Arc<[u8]>>) {
        self.files.insert(path, bytes.into());
    }

    /// Gives the file at `path` back to the disk.
    pub fn remove(&mut self, path: &Path) {
        self.files.remove(path);
    }

    pub fn get(&self, path: &Path) -> Option<&Arc<[u8]>> {
        self.files.get(path)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The files held that are `dir` or below it.
    pub fn under<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = &'a PathBuf> {
        self.files.keys().filter(move |path| path.starts_with(dir))
    }
}
