//! A Composer project's package context: its `composer.json` and
//! installed packages (`manifest`), the vendor files its autoloader loads
//! for context (`vendor`), and the answers read from them, computed once
//! per project root.

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
    /// Reads the project at `root`. `spelled_root` is the root as analysed
    /// paths spell it, which the manifest's own paths follow.
    pub fn load(root: &Path, spelled_root: &Path) -> PackageContext {
        PackageContext {
            manifest: ComposerManifest::load(spelled_root),
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

    /// Each directory whose files belong to a package, with that package's
    /// name: every installed package's (as linked under `vendor/` and as
    /// installed) and the root package's own.
    pub fn package_dirs(&self) -> Vec<(PathBuf, String)> {
        let Some(manifest) = &self.manifest else {
            return Vec::new();
        };
        let mut dirs: Vec<(PathBuf, String)> = manifest
            .installed
            .iter()
            .flat_map(|p| {
                [
                    (p.vendor_dir.clone(), p.name.clone()),
                    (p.install_dir.clone(), p.name.clone()),
                ]
            })
            .collect();
        if let Some(name) = &manifest.name {
            dirs.push((manifest.root.clone(), name.clone()));
        }
        dirs
    }
}

/// The package owning `file` among `package_dirs`: the one with the
/// deepest directory containing it.
pub fn owning_package<'a>(package_dirs: &'a [(PathBuf, String)], file: &Path) -> Option<&'a str> {
    package_dirs
        .iter()
        .filter(|(dir, _)| path_starts_with(file, dir))
        .max_by_key(|(dir, _)| dir.components().count())
        .map(|(_, name)| name.as_str())
}

#[cfg(test)]
mod tests;
