//! Packages that a custom installer put outside the vendor directory
//! (`extra.installer-paths`, as Drupal's `web/core` is): their install
//! directories are not the project's own code, and their autoload entries are
//! vendor context.

use super::helpers::*;
use crate::PackageContext;
use crate::canonical::canonicalize;
use crate::dir_listings::DirListings;
use crate::layout::no_variables;
use crate::manifest::ComposerManifest;
use std::sync::Arc;

/// A project whose `vendor/composer/` lists `drupal/core`, installed at
/// `web/core` as Composer writes it from the vendor directory, beside
/// `acme/lib`, which is installed in the vendor directory. Its generated map
/// lists `web/core`'s classes and the project's own `src/`.
fn drupal_project(dir: &TempDir) {
    dir.write(
        "composer.json",
        r#"{"name": "acme/site", "extra": {"installer-paths": {"web/core": ["drupal/core"]}}}"#,
    );
    dir.write(
        "vendor/composer/installed.json",
        r#"{"packages": [
            {"name": "drupal/core", "install-path": "../../web/core"},
            {"name": "acme/lib", "install-path": "../acme/lib"}
        ]}"#,
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Drupal\\Core\\' => array($baseDir . '/web/core/lib/Drupal/Core'),
    'Project\\' => array($baseDir . '/src'),
);
"#,
    );
    dir.write("web/core/lib/Drupal/Core/Foo.php", "<?php class Foo {}");
    dir.write("src/Bar.php", "<?php class Bar {}");
    dir.write("vendor/acme/lib/src/Baz.php", "<?php class Baz {}");
}

/// The project at `dir`, loaded with no environment.
fn load(dir: &TempDir) -> PackageContext {
    let root = canonicalize(dir.path()).unwrap();
    PackageContext::load(
        &root,
        &root,
        &Arc::new(DirListings::default()),
        &no_variables,
        None,
    )
}

#[test]
fn the_directory_of_a_package_installed_outside_the_vendor_directory_is_context() {
    let dir = TempDir::new("installer_paths_context");
    drupal_project(&dir);
    let package = load(&dir);
    let core = canonicalize(&dir.path().join("web/core/lib/Drupal/Core")).unwrap();
    let found = &package.vendor.get().dirs;
    assert!(
        found.iter().any(|d| d.as_path() == core.as_path()),
        "{found:?}"
    );
}

#[test]
fn the_project_own_directories_are_not_context_when_a_package_is_installed_elsewhere() {
    let dir = TempDir::new("installer_paths_own_code");
    drupal_project(&dir);
    let package = load(&dir);
    let own = canonicalize(&dir.path().join("src")).unwrap();
    let found = &package.vendor.get().dirs;
    assert!(
        !found.iter().any(|d| d.as_path() == own.as_path()),
        "{found:?}"
    );
}

#[test]
fn installed_elsewhere_lists_the_install_directories_outside_the_vendor_directory() {
    let dir = TempDir::new("installed_elsewhere");
    dir.write(
        "composer.json",
        r#"{"name": "acme/site", "repositories": [{"type": "path", "url": "packages/*"}]}"#,
    );
    dir.write(
        "vendor/composer/installed.json",
        r#"{"packages": [
            {"name": "drupal/core", "install-path": "../../web/core"},
            {"name": "acme/meta", "install-path": null},
            {"name": "acme/lib", "install-path": "../acme/lib"},
            {"name": "acme/shared", "install-path": "../../packages/shared"}
        ]}"#,
    );
    dir.write("web/core/lib.php", "<?php");
    dir.write("packages/shared/composer.json", "{}");
    let root = canonicalize(dir.path()).unwrap();
    let vendor = root.join("vendor");
    let manifest = ComposerManifest::load(
        &root.join("composer.json"),
        &root,
        &root,
        Some(vendor.as_path()),
        &DirListings::default(),
    )
    .unwrap();
    assert_eq!(
        manifest.installed_elsewhere(&vendor),
        vec![canonicalize(&dir.path().join("web/core")).unwrap()]
    );
}
