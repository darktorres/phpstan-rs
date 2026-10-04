//! A Composer project's package context: its `composer.json` and
//! installed packages (`manifest`), the vendor files its autoloader loads
//! for context (`vendor`), and the answers read from them, computed once
//! per project root.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

pub mod constraint;
pub mod manifest;
pub mod path_order;
pub mod vendor;

use rustc_hash::FxHashMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use manifest::{ComposerManifest, lexically_normalized};
use vendor::VendorContext;

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
    /// Starts discovering the vendor context of the project at `root`.
    pub fn spawn(root: PathBuf) -> PendingVendor {
        let thread = std::thread::spawn(move || vendor::discover_vendor_context(&root));
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
    /// follow.
    pub fn load(root: &Path, spelled_root: &Path) -> PackageContext {
        PackageContext {
            manifest: ComposerManifest::load(spelled_root, root),
            vendor: PendingVendor::spawn(root.to_path_buf()),
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
    /// Every owned directory, absolute: each root's own, and each
    /// installed package's `vendor/` link and install directory.
    dirs: Vec<OwnedDir>,
    // Indices into `dirs` by directory as written, and by its normalized
    // form: a file's owners are among its ancestors.
    by_dir: DirTrie,
    by_normalized: DirTrie,
}

/// Directories by their path components, so the owners among a path's
/// ancestors are found by one walk down its components, hashing each
/// component once, and not by hashing every ancestor path in full.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct DirTrie {
    root: TrieNode,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TrieNode {
    /// The last of the directories that end here, in the order listed.
    last: Option<usize>,
    children: FxHashMap<OsString, TrieNode>,
}

impl DirTrie {
    fn insert(&mut self, dir: &Path, index: usize) {
        let mut node = &mut self.root;
        for component in dir.components() {
            node = node
                .children
                .entry(component.as_os_str().to_os_string())
                .or_default();
        }
        // Directories are inserted in listing order.
        node.last = Some(index);
    }

    /// Calls `found` with the directory that ends at each prefix of
    /// `components` the trie has one for, shortest prefix first.
    fn for_each_prefix<'a>(
        &self,
        components: impl Iterator<Item = Component<'a>>,
        mut found: impl FnMut(usize),
    ) {
        let mut node = &self.root;
        for component in components {
            let Some(next) = node.children.get(component.as_os_str()) else {
                return;
            };
            node = next;
            if let Some(index) = node.last {
                found(index);
            }
        }
    }
}

/// A directory some package owns, with what `PackageIndex::owner`
/// compares a file against computed once.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnedDir {
    dir: PathBuf,
    // `dir` with `.` and `..` resolved.
    normalized: PathBuf,
    depth: usize,
    owner: PackageOwner,
}

impl OwnedDir {
    fn new(dir: PathBuf, owner: PackageOwner) -> OwnedDir {
        let normalized = lexically_normalized(&dir);
        OwnedDir {
            depth: dir.components().count(),
            dir,
            normalized,
            owner,
        }
    }
}

impl PackageIndex {
    pub fn new(manifests: Vec<ComposerManifest>) -> PackageIndex {
        let mut dirs = Vec::new();
        for (i, manifest) in manifests.iter().enumerate() {
            dirs.push(OwnedDir::new(
                manifest.absolute_root.clone(),
                PackageOwner::Root(i),
            ));
            for (j, package) in manifest.installed.iter().enumerate() {
                let owner = PackageOwner::Installed(i, j);
                dirs.push(OwnedDir::new(manifest.absolute(&package.vendor_dir), owner));
                dirs.push(OwnedDir::new(package.install_dir.clone(), owner));
            }
        }
        let mut by_dir = DirTrie::default();
        let mut by_normalized = DirTrie::default();
        for (index, dir) in dirs.iter().enumerate() {
            by_dir.insert(&dir.dir, index);
            by_normalized.insert(&dir.normalized, index);
        }
        PackageIndex {
            manifests,
            dirs,
            by_dir,
            by_normalized,
        }
    }

    pub fn manifests(&self) -> &[ComposerManifest] {
        &self.manifests
    }

    /// The package owning the absolute `file`: the one whose directory
    /// containing it is deepest.
    pub fn owner(&self, file: &Path) -> Option<PackageOwner> {
        // The directories `path_starts_with(file, dir)` holds for: an
        // ancestor of `file` as written, or of it normalized. Of equally
        // deep ones the last listed wins.
        let mut best: Option<usize> = None;
        let mut consider = |index: usize| {
            let key = |i: usize| (self.dirs[i].depth, i);
            if best.is_none_or(|b| key(index) > key(b)) {
                best = Some(index);
            }
        };
        self.by_dir
            .for_each_prefix(file.components(), &mut consider);
        // `lexically_normalized`, as the components it would have: a `.` is
        // dropped, and a `..` takes the last one off unless it is all that
        // is left of a root, in which case it stays.
        let mut normalized: Vec<Component<'_>> = Vec::new();
        for component in file.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    let can_pop = !matches!(normalized.as_slice(), [] | [Component::RootDir]);
                    if can_pop {
                        normalized.pop();
                    } else {
                        normalized.push(Component::ParentDir);
                    }
                }
                other => normalized.push(other),
            }
        }
        self.by_normalized
            .for_each_prefix(normalized.into_iter(), &mut consider);
        best.map(|index| self.dirs[index].owner)
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

syntax::canonical_enum!(PackageOwner {
    Root(index),
    Installed(package, index),
});

#[cfg(test)]
mod tests;
