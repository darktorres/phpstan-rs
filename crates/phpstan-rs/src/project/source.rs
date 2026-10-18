//! Where a load reads a file's bytes from: an in-memory overlay (the
//! language server's open documents, the files a test or a benchmark
//! loads), and the disk for every other path.

use rustc_hash::FxHashMap;
use std::borrow::Cow;
use std::io::{self, Read};
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

    /// Reads the bytes of `path` into `buf`, which is cleared first: the
    /// overlay's, else the disk's. A reader that fills one buffer for every
    /// file allocates only when a file is longer than those before it.
    pub fn read_into(&self, path: &Path, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.clear();
        match self.overlay.get(path) {
            Some(bytes) => buf.extend_from_slice(&bytes[..]),
            None => {
                std::fs::File::open(path)?.read_to_end(buf)?;
            }
        }
        Ok(())
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

/// A document's bytes with their digest (`content_hash`), taken once for
/// every project the document is given to.
#[derive(Clone, Copy)]
pub struct Digested<'a> {
    bytes: &'a [u8],
    digest: u128,
}

impl<'a> Digested<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Digested {
            bytes,
            digest: content_hash(bytes),
        }
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn digest(&self) -> u128 {
        self.digest
    }
}

/// A file's bytes as text, when they are UTF-8.
pub fn utf8_text(bytes: &[u8]) -> Option<&str> {
    simdutf8::basic::from_utf8(bytes).ok()
}

/// A file's bytes as text: the bytes themselves when they are UTF-8, and a
/// lossy reading of them, each invalid sequence replaced, when they are not.
pub fn decode_text(bytes: &[u8]) -> Cow<'_, str> {
    match utf8_text(bytes) {
        Some(text) => Cow::Borrowed(text),
        None => String::from_utf8_lossy(bytes),
    }
}
