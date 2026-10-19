//! Where each installed package's directories are, as `installed.json` and the
//! vendor directory on disk give them: a package in place, a linked one, a
//! path that goes through a link, and a directory that is not there.

use super::helpers::*;
use crate::canonical::canonicalize;
use crate::dir_listings::DirListings;
use crate::manifest::{ComposerManifest, InstalledPackage};
use crate::test_links::link_dir;
use std::path::PathBuf;

/// The packages `composer.json` and `vendor/composer/installed.json` of the
/// project at `dir` describe, with `vendor` as its vendor directory.
fn installed(dir: &TempDir) -> Vec<InstalledPackage> {
    let root = canonicalize(dir.path()).unwrap();
    ComposerManifest::load(
        &root.join("composer.json"),
        &root,
        &root,
        Some(&root.join("vendor")),
        &DirListings::default(),
    )
    .unwrap()
    .installed
}

/// The canonical form of `rel` under the project at `dir`, as the test
/// expects a directory to be read.
fn expected(dir: &TempDir, rel: &str) -> PathBuf {
    canonicalize(&dir.path().join(rel)).unwrap()
}

/// The package called `name`.
fn package<'a>(packages: &'a [InstalledPackage], name: &str) -> &'a InstalledPackage {
    packages.iter().find(|p| p.name == name).unwrap()
}

/// A project whose `vendor/composer/installed.json` lists `packages`.
fn project(dir: &TempDir, packages: &str) {
    dir.write("composer.json", "{}");
    dir.write(
        "vendor/composer/installed.json",
        &format!(r#"{{"packages": [{packages}]}}"#),
    );
}

#[test]
fn a_package_in_place_is_read_as_its_directory() {
    let dir = TempDir::new("installed_in_place");
    project(
        &dir,
        r#"{"name": "acme/lib", "install-path": "../acme/lib"}"#,
    );
    dir.write("vendor/acme/lib/src/A.php", "<?php");
    let packages = installed(&dir);
    let lib = package(&packages, "acme/lib");
    assert_eq!(lib.vendor_dir, expected(&dir, "vendor/acme/lib"));
    assert_eq!(lib.install_dir, expected(&dir, "vendor/acme/lib"));
}

#[test]
fn a_dot_prefixed_install_path_in_place_is_the_package_directory() {
    let dir = TempDir::new("installed_dot_prefixed");
    project(
        &dir,
        r#"{"name": "acme/lib", "install-path": "./../acme/lib"}"#,
    );
    dir.write("vendor/acme/lib/src/A.php", "<?php");
    let packages = installed(&dir);
    let lib = package(&packages, "acme/lib");
    assert_eq!(lib.install_dir, expected(&dir, "vendor/acme/lib"));
}

#[test]
fn a_linked_package_is_read_as_the_directory_it_links_to() {
    let dir = TempDir::new("installed_linked_package");
    project(
        &dir,
        r#"{"name": "acme/lib", "install-path": "../acme/lib"}"#,
    );
    dir.write("elsewhere/lib/src/A.php", "<?php");
    std::fs::create_dir_all(dir.path().join("vendor/acme")).unwrap();
    link_dir(
        dir.path().join("elsewhere/lib"),
        dir.path().join("vendor/acme/lib"),
    );
    let packages = installed(&dir);
    let lib = package(&packages, "acme/lib");
    assert_eq!(lib.vendor_dir, expected(&dir, "elsewhere/lib"));
    assert_eq!(lib.install_dir, expected(&dir, "elsewhere/lib"));
}

#[test]
fn a_linked_vendor_name_resolves_the_package_through_it() {
    let dir = TempDir::new("installed_linked_vendor_name");
    project(
        &dir,
        r#"{"name": "acme/lib", "install-path": "../acme/lib"}"#,
    );
    dir.write("elsewhere/acme/lib/src/A.php", "<?php");
    std::fs::create_dir_all(dir.path().join("vendor")).unwrap();
    link_dir(
        dir.path().join("elsewhere/acme"),
        dir.path().join("vendor/acme"),
    );
    let packages = installed(&dir);
    let lib = package(&packages, "acme/lib");
    assert_eq!(lib.vendor_dir, expected(&dir, "elsewhere/acme/lib"));
    assert_eq!(lib.install_dir, expected(&dir, "elsewhere/acme/lib"));
}

#[test]
fn a_linked_composer_directory_resolves_the_install_path_as_it_did() {
    let dir = TempDir::new("installed_linked_composer");
    dir.write("composer.json", "{}");
    dir.write(
        "elsewhere/composer/installed.json",
        r#"{"packages": [{"name": "acme/lib", "install-path": "../acme/lib"}]}"#,
    );
    dir.write("elsewhere/acme/lib/src/A.php", "<?php");
    std::fs::create_dir_all(dir.path().join("vendor")).unwrap();
    link_dir(
        dir.path().join("elsewhere/composer"),
        dir.path().join("vendor/composer"),
    );
    let packages = installed(&dir);
    let lib = package(&packages, "acme/lib");
    assert_eq!(lib.install_dir, expected(&dir, "elsewhere/acme/lib"));
}

#[test]
fn an_interior_parent_step_is_resolved_through_the_link_it_follows() {
    let dir = TempDir::new("installed_interior_parent");
    project(
        &dir,
        r#"{"name": "acme/lib", "install-path": "../acme/q/../lib"}"#,
    );
    dir.write("elsewhere/z/keep.php", "<?php");
    dir.write("elsewhere/lib/src/A.php", "<?php");
    std::fs::create_dir_all(dir.path().join("vendor/acme")).unwrap();
    link_dir(
        dir.path().join("elsewhere/z"),
        dir.path().join("vendor/acme/q"),
    );
    let packages = installed(&dir);
    let lib = package(&packages, "acme/lib");
    assert_eq!(lib.install_dir, expected(&dir, "elsewhere/lib"));
}

#[test]
fn a_package_whose_directory_is_missing_keeps_the_lexical_path() {
    let dir = TempDir::new("installed_missing_package");
    project(
        &dir,
        r#"{"name": "acme/gone", "install-path": "../acme/gone"}"#,
    );
    let packages = installed(&dir);
    let gone = package(&packages, "acme/gone");
    let vendor = canonicalize(&dir.path().join("vendor")).unwrap();
    assert_eq!(gone.vendor_dir, vendor.join("acme/gone"));
    assert_eq!(gone.install_dir, vendor.join("acme/gone"));
}

#[test]
fn a_null_install_path_is_the_vendor_directory() {
    let dir = TempDir::new("installed_null_install_path");
    project(&dir, r#"{"name": "acme/meta", "install-path": null}"#);
    dir.write("vendor/acme/meta/composer.json", "{}");
    let packages = installed(&dir);
    let meta = package(&packages, "acme/meta");
    assert_eq!(meta.install_dir, meta.vendor_dir);
    assert_eq!(meta.vendor_dir, expected(&dir, "vendor/acme/meta"));
}

#[test]
fn an_install_path_outside_the_vendor_directory_is_resolved() {
    let dir = TempDir::new("installed_outside_vendor");
    project(
        &dir,
        r#"{"name": "acme/web", "install-path": "../../web/core"}"#,
    );
    dir.write("web/core/lib.php", "<?php");
    let packages = installed(&dir);
    let web = package(&packages, "acme/web");
    assert_eq!(web.install_dir, expected(&dir, "web/core"));
}
