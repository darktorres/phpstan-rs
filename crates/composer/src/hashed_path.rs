//! A path with the hash of its components, made once, where the path is made,
//! and carried with it, so that a table keyed by such paths finds one without
//! hashing it again. The hash is folded over the components, so paths equal
//! as `Path`s hash alike, and a directory's hash extends to each of its
//! entries by the entry's name. A `Resolved` path is one that resolving gives
//! back as it is, so a walk takes it without resolving it again.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::hash::BuildHasher;
use std::path::{Component, Path, PathBuf};

use rustc_hash::FxBuildHasher;

use crate::path_bytes;

/// The hash of a path, as `path_hash` gives it.
pub type PathHash = u64;

/// A path with its hash and its parent's, as `path_hash` gives them. `new`
/// folds both over the path's components, and `child` extends a directory's
/// by an entry's name; nothing else makes one, so the hashes are the path's.
#[derive(Clone, Debug)]
pub struct HashedPath {
    path: PathBuf,
    hash: PathHash,
    /// The hash of `path.parent()`, or 0 when it has none.
    parent_hash: PathHash,
}

impl HashedPath {
    /// `path`, with its hashes.
    pub fn new(path: PathBuf) -> Self {
        let (hash, parent_hash) = path.components().fold((0, 0), |(hash, _), component| {
            (child_hash(hash, component.as_os_str()), hash)
        });
        HashedPath {
            path,
            hash,
            parent_hash,
        }
    }

    /// The entry `name` of this directory. `name` is one name, as a listing
    /// gives it.
    pub fn child(&self, name: &OsStr) -> Self {
        // Made in one allocation, where `join` copies the directory and then
        // grows the copy.
        let mut path = PathBuf::with_capacity(self.path.as_os_str().len() + 1 + name.len());
        path.push(&self.path);
        path.push(name);
        let child = HashedPath {
            path,
            hash: child_hash(self.hash, name),
            parent_hash: self.hash,
        };
        debug_assert_eq!(
            child.hash,
            path_hash(&child.path),
            "`name` must be one name"
        );
        child
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    pub fn hash(&self) -> PathHash {
        self.hash
    }

    /// The directory that holds the path, with its hash, when it has one.
    pub fn parent(&self) -> Option<(&Path, PathHash)> {
        path_bytes::parent(&self.path).map(|parent| (parent, self.parent_hash))
    }

    /// `parent` and the path's last component, as `Path::file_name` gives it,
    /// when the path has both, from one reading of that component.
    pub fn split(&self) -> Option<(&Path, PathHash, &OsStr)> {
        path_bytes::split(&self.path).map(|(dir, name)| (dir, self.parent_hash, name))
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.path
    }
}

impl AsRef<Path> for HashedPath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl PartialEq for HashedPath {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.path == other.path
    }
}

impl Eq for HashedPath {}

/// In the order of the paths.
impl Ord for HashedPath {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path.cmp(&other.path)
    }
}

impl PartialOrd for HashedPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A path that `manifest::resolve` gives back as it is, with its hashes: its
/// links followed and its `.` and `..` taken out. `resolve` makes one, and the
/// vendor discovery makes one where a directory's listing shows that a path is
/// already resolved; nothing else does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved(HashedPath);

impl Resolved {
    /// `path`, resolved.
    pub fn resolve(path: &Path) -> Self {
        Resolved(HashedPath::new(crate::manifest::resolve(path)))
    }

    /// `path`, which the caller knows to be resolved.
    pub(crate) fn known(path: HashedPath) -> Self {
        debug_assert_eq!(
            crate::manifest::resolve(path.as_path()),
            path.as_path(),
            "the path must be resolved"
        );
        Resolved(path)
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    pub fn into_hashed(self) -> HashedPath {
        self.0
    }
}

/// The hash of `path`, to look up a path that is not held as a `HashedPath`.
pub fn path_hash(path: &Path) -> PathHash {
    path.components()
        .fold(0, |hash, component| child_hash(hash, component.as_os_str()))
}

/// Each ancestor of `path`, from `path` itself up to the top, with its hash
/// and its name, as `Path::file_name` gives it; from the top, reversed. All
/// come from one pass over the components.
pub fn hashed_ancestors(
    path: &Path,
) -> impl DoubleEndedIterator<Item = (&Path, PathHash, Option<&OsStr>)> {
    // From the top. A relative path's top ancestor is the empty path, of
    // hash 0.
    let mut ancestors = Vec::with_capacity(16);
    if let Some(components) = path_bytes::components(path) {
        if !path.has_root() {
            ancestors.push((Path::new(""), 0, None));
        }
        let mut hash = 0;
        for (component, ancestor) in components {
            hash = child_hash(hash, component.as_os_str());
            let name = match component {
                Component::Normal(name) => Some(name),
                _ => None,
            };
            ancestors.push((ancestor, hash, name));
        }
    } else {
        let mut hashes = Vec::with_capacity(16);
        hashes.push(0);
        let mut hash = 0;
        for component in path.components() {
            hash = child_hash(hash, component.as_os_str());
            hashes.push(hash);
        }
        ancestors.extend(
            path.ancestors()
                .zip(hashes.into_iter().rev())
                .map(|(ancestor, hash)| (ancestor, hash, ancestor.file_name())),
        );
        ancestors.reverse();
    }
    ancestors.into_iter().rev()
}

/// The hash of the path that `name` makes under a directory of hash `parent`.
fn child_hash(parent: PathHash, name: &OsStr) -> PathHash {
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
    (parent.rotate_left(5) ^ FxBuildHasher.hash_one(name)).wrapping_mul(SEED)
}
