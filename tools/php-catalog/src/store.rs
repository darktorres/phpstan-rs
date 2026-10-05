//! The catalog's store: each function's lowered code under the hash of its
//! content, so that a function another read leaves unchanged is stored
//! once, and each program read under a name, as the functions it links by
//! their hashes. The analyses load a function when they reach it, so the
//! program never sits whole in memory.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use twox_hash::XxHash3_128;

/// A record's key: the hash of its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Hash(pub(crate) u128);

impl Hash {
    pub(crate) fn of(bytes: &[u8]) -> Hash {
        Hash(XxHash3_128::oneshot(bytes))
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

pub(crate) struct Store {
    root: PathBuf,
}

/// The temporaries written so far, to name the next one apart.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

impl Store {
    pub(crate) fn open(root: &Path) -> Result<Store, String> {
        for dir in ["objects", "programs"] {
            let dir = root.join(dir);
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        Ok(Store {
            root: root.to_owned(),
        })
    }

    fn object_path(&self, hash: Hash) -> PathBuf {
        let hex = hash.to_string();
        self.root.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    /// Stores a record, once for its content, and gives its key.
    pub(crate) fn put<T: Serialize>(&self, value: &T) -> Result<Hash, String> {
        let bytes = postcard::to_allocvec(value).map_err(|e| e.to_string())?;
        let hash = Hash::of(&bytes);
        let path = self.object_path(hash);
        if !path.exists() {
            write_whole(&path, &bytes)?;
        }
        Ok(hash)
    }

    /// The record stored under a key, checked against it.
    pub(crate) fn get<T: DeserializeOwned>(&self, hash: Hash) -> Result<T, String> {
        let path = self.object_path(hash);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if Hash::of(&bytes) != hash {
            return Err(format!("{}: its content has another hash", path.display()));
        }
        postcard::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The size in bytes of the record stored under a key.
    pub(crate) fn size(&self, hash: Hash) -> u64 {
        std::fs::metadata(self.object_path(hash)).map_or(0, |m| m.len())
    }

    fn program_path(&self, name: &str) -> PathBuf {
        self.root.join("programs").join(name)
    }

    /// Stores a value under a name, replacing what the name held.
    pub(crate) fn save_named<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        let path = self.program_path(name);
        let bytes = postcard::to_allocvec(value).map_err(|e| format!("{}: {e}", path.display()))?;
        write_whole(&path, &bytes)
    }

    pub(crate) fn load_named<T: DeserializeOwned>(&self, name: &str) -> Result<T, String> {
        let path = self.program_path(name);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        postcard::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Writes a file whole or not at all: to a temporary beside it, then
/// renamed over it.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let temporary = dir.join(format!(
        ".{}-{}",
        std::process::id(),
        TEMPORARIES.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&temporary, bytes).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))
}
