//! A Composer project's package context: its `composer.json` and
//! installed packages (`manifest`), the vendor files its autoloader loads
//! for context (`vendor`), and the answers read from them, computed once
//! per project root.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

pub mod canonical;
pub mod constraint;
pub mod dir_listings;
pub mod hashed_path;
pub mod manifest;
pub mod path_bytes;
pub mod path_order;
pub mod vendor;

use rustc_hash::FxHashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use dir_listings::DirListings;
use manifest::ComposerManifest;
use vendor::{GeneratedCache, VendorContext};

/// Everything a Composer project root provides, read once.
#[derive(Debug, Default, Clone)]
pub struct PackageContext {
    /// `composer.json` and `installed.json`; `None` without a readable,
    /// parseable `composer.json`.
    pub manifest: Option<ComposerManifest>,
    /// The vendor files and directories loaded for context, found while
    /// the caller reads on (`vendor()` waits for them).
    pub vendor: PendingVendor,
}

/// The vendor context of a project, discovered on a thread of its own: the
/// generated autoload files are parsed while the caller goes on to what
/// does not depend on them, and the first to ask for them waits.
#[derive(Clone, Default)]
pub struct PendingVendor(Arc<PendingVendorInner>);

#[derive(Default)]
struct PendingVendorInner {
    found: OnceLock<VendorContext>,
    thread: Mutex<Option<std::thread::JoinHandle<VendorContext>>>,
}

impl PendingVendor {
    /// Starts discovering the vendor context of the project at `root`, reading
    /// its directories through `listings` and keeping what the generated
    /// files list in `cache`.
    pub fn spawn(
        root: PathBuf,
        listings: Arc<DirListings>,
        cache: Option<&'static dyn GeneratedCache>,
    ) -> PendingVendor {
        let thread =
            std::thread::spawn(move || vendor::discover_vendor_context(&root, &listings, cache));
        PendingVendor(Arc::new(PendingVendorInner {
            found: OnceLock::new(),
            thread: Mutex::new(Some(thread)),
        }))
    }

    /// The vendor context, once discovery has ended: empty when it
    /// failed.
    pub fn get(&self) -> &VendorContext {
        self.0.found.get_or_init(|| {
            let thread = self
                .0
                .thread
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            thread
                .and_then(|thread| thread.join().ok())
                .unwrap_or_default()
        })
    }
}

impl From<VendorContext> for PendingVendor {
    fn from(context: VendorContext) -> PendingVendor {
        let found = OnceLock::new();
        let _ = found.set(context);
        PendingVendor(Arc::new(PendingVendorInner {
            found,
            thread: Mutex::new(None),
        }))
    }
}

impl std::fmt::Debug for PendingVendor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.found.get() {
            Some(found) => found.fmt(f),
            None => f.write_str("PendingVendor(..)"),
        }
    }
}

impl PackageContext {
    /// Reads the project at `root`, which is canonical. `spelled_root` is
    /// the root as analysed paths spell it, which the manifest's own paths
    /// follow. The vendor context is discovered through `listings`, with what
    /// the generated files list kept in `cache`.
    pub fn load(
        root: &Path,
        spelled_root: &Path,
        listings: &Arc<DirListings>,
        cache: Option<&'static dyn GeneratedCache>,
    ) -> PackageContext {
        PackageContext {
            manifest: ComposerManifest::load(spelled_root, root),
            vendor: PendingVendor::spawn(root.to_path_buf(), Arc::clone(listings), cache),
        }
    }

    /// Whether the project requires `phpstan/phpstan` or
    /// `phpstan/extension-installer` (either section): a consumer of
    /// PHPStan as a library, which the PHPStan-API rules apply to.
    pub fn requires_phpstan(&self) -> bool {
        self.manifest.as_ref().is_some_and(|m| {
            ["phpstan/phpstan", "phpstan/extension-installer"]
                .iter()
                .any(|p| m.requires(p) || m.requires_dev(p))
        })
    }

    /// The PHP versions the project declares it runs on: its
    /// `config.platform.php`, else its `require.php` constraint.
    pub fn php_constraint(&self) -> Option<&str> {
        let manifest = self.manifest.as_ref()?;
        manifest
            .platform_php
            .as_ref()
            .map(|p| p.value.as_str())
            .or_else(|| manifest.constraints.get("php").map(String::as_str))
    }
}

/// A package that owns files: a composer root's own package, or one of the
/// packages installed under it (indices into `PackageIndex::manifests` and
/// that manifest's `installed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageOwner {
    Root(usize),
    Installed(usize, usize),
}

/// Package ownership across a project's composer roots.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackageIndex {
    manifests: Vec<ComposerManifest>,
    /// The owned directories: each root's own, and each installed package's
    /// `vendor/` directory and install directory.
    by_dir: DirTrie,
}

/// Directories by their path components, so the directory that owns a path
/// is found by one walk down its components, hashing each component once. A
/// component is keyed by its text as `canonical::component_text` gives it,
/// so the spellings of a prefix that `Path` calls equal find one directory.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct DirTrie {
    root: TrieNode,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TrieNode {
    /// The owner of the directory that ends here: the last one listed.
    owner: Option<PackageOwner>,
    children: FxHashMap<OsString, TrieNode>,
}

impl DirTrie {
    fn insert(&mut self, dir: &Path, owner: PackageOwner) {
        let mut node = &mut self.root;
        for component in dir.components() {
            node = node
                .children
                .entry(canonical::component_text(component).into_owned())
                .or_default();
        }
        // Directories are inserted in listing order.
        node.owner = Some(owner);
    }

    /// Where `dir` stands: the owner of the deepest owned directory that
    /// contains it, `dir` itself included.
    fn at(&self, dir: &Path) -> OwnerAt<'_> {
        let mut at = OwnerAt {
            owner: None,
            node: Some(&self.root),
        };
        for component in dir.components() {
            at = at.child(&canonical::component_text(component));
            if at.node.is_none() {
                break;
            }
        }
        at
    }
}

/// Where a directory stands among the owned directories: the owner of the
/// deepest one that contains it, and its own node while the trie has one, so
/// that a subdirectory's is found from its name alone.
#[derive(Clone, Copy, Debug)]
pub struct OwnerAt<'i> {
    owner: Option<PackageOwner>,
    node: Option<&'i TrieNode>,
}

impl<'i> OwnerAt<'i> {
    /// The package owning the directory, as `PackageIndex::owner_of_dir`
    /// gives it.
    pub fn owner(&self) -> Option<PackageOwner> {
        self.owner
    }

    /// Where the directory's entry `name` stands. `name` is one name, as a
    /// listing gives it.
    pub fn child(&self, name: &OsStr) -> OwnerAt<'i> {
        match self.node.and_then(|node| node.children.get(name)) {
            Some(node) => OwnerAt {
                owner: node.owner.or(self.owner),
                node: Some(node),
            },
            None => OwnerAt {
                owner: self.owner,
                node: None,
            },
        }
    }
}

impl PackageIndex {
    pub fn new(manifests: Vec<ComposerManifest>) -> PackageIndex {
        let mut by_dir = DirTrie::default();
        for (i, manifest) in manifests.iter().enumerate() {
            by_dir.insert(&manifest.absolute_root, PackageOwner::Root(i));
            for (j, package) in manifest.installed.iter().enumerate() {
                let owner = PackageOwner::Installed(i, j);
                by_dir.insert(&package.vendor_dir, owner);
                by_dir.insert(&package.install_dir, owner);
            }
        }
        PackageIndex { manifests, by_dir }
    }

    pub fn manifests(&self) -> &[ComposerManifest] {
        &self.manifests
    }

    /// The package owning the directory `dir`: the one whose owned directory
    /// containing it is deepest, the last listed among equals. `dir` is an
    /// absolute path with `.` and `..` taken out and its links followed, as
    /// `manifest::resolve` gives it. A file belongs to its directory's package.
    pub fn owner_of_dir(&self, dir: &Path) -> Option<PackageOwner> {
        self.by_dir.at(dir).owner()
    }

    /// Where the directory `dir` stands, `dir` as `owner_of_dir` takes it:
    /// its owner, and where each of its subdirectories stands.
    pub fn owner_at(&self, dir: &Path) -> OwnerAt<'_> {
        self.by_dir.at(dir)
    }

    /// `owner`'s package name; a root package may have none.
    pub fn name(&self, owner: PackageOwner) -> Option<&str> {
        match owner {
            PackageOwner::Root(i) => self.manifests[i].name.as_deref(),
            PackageOwner::Installed(i, j) => Some(&self.manifests[i].installed[j].name),
        }
    }

    /// The installed package `owner` names, if it is one.
    pub fn installed(&self, owner: PackageOwner) -> Option<&manifest::InstalledPackage> {
        match owner {
            PackageOwner::Root(_) => None,
            PackageOwner::Installed(i, j) => Some(&self.manifests[i].installed[j]),
        }
    }
}

#[cfg(test)]
mod tests;
