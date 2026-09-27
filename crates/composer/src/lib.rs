//! A Composer project's package context: its `composer.json` and
//! installed packages (`manifest`), the vendor files its autoloader loads
//! for context (`vendor`), and the answers read from them, computed once
//! per project root.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

pub mod constraint;
pub mod manifest;
pub mod vendor;

use std::path::{Path, PathBuf};

use manifest::{ComposerManifest, path_starts_with};
use vendor::VendorContext;

/// Everything a Composer project root provides, read once.
#[derive(Debug, Default, Clone)]
pub struct PackageContext {
    /// `composer.json` and `installed.json`; `None` without a readable,
    /// parseable `composer.json`.
    pub manifest: Option<ComposerManifest>,
    /// The vendor files and directories loaded for context.
    pub vendor: VendorContext,
}

impl PackageContext {
    /// Reads the project at `root`, which is canonical. `spelled_root` is
    /// the root as analysed paths spell it, which the manifest's own paths
    /// follow.
    pub fn load(root: &Path, spelled_root: &Path) -> PackageContext {
        PackageContext {
            manifest: ComposerManifest::load(spelled_root, root),
            vendor: vendor::discover_vendor_context(root),
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
    dirs: Vec<(PathBuf, PackageOwner)>,
}

impl PackageIndex {
    pub fn new(manifests: Vec<ComposerManifest>) -> PackageIndex {
        let mut dirs = Vec::new();
        for (i, manifest) in manifests.iter().enumerate() {
            dirs.push((manifest.absolute_root.clone(), PackageOwner::Root(i)));
            for (j, package) in manifest.installed.iter().enumerate() {
                let owner = PackageOwner::Installed(i, j);
                dirs.push((manifest.absolute(&package.vendor_dir), owner));
                dirs.push((package.install_dir.clone(), owner));
            }
        }
        PackageIndex { manifests, dirs }
    }

    pub fn manifests(&self) -> &[ComposerManifest] {
        &self.manifests
    }

    /// The package owning the absolute `file`: the one whose directory
    /// containing it is deepest.
    pub fn owner(&self, file: &Path) -> Option<PackageOwner> {
        self.dirs
            .iter()
            .filter(|(dir, _)| path_starts_with(file, dir))
            .max_by_key(|(dir, _)| dir.components().count())
            .map(|(_, owner)| *owner)
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
