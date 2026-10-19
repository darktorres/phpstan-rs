//! The vendor context of a package that bundles a vendor directory of its
//! own, which its autoloader loads the classes it names from.

use super::super::*;
use super::helpers::*;
use crate::canonical::canonicalize;
use crate::hashed_path::Resolved;
use crate::vendor::VendorPath;
use std::sync::Arc;

/// The vendor context of the project at `dir`, read as a session reads it.
fn context(dir: &TempDir) -> VendorContext {
    let root = canonicalize(dir.path()).unwrap();
    PackageContext::load(
        &root,
        &root,
        &Arc::default(),
        &crate::layout::no_variables,
        None,
    )
    .vendor
    .get()
    .clone()
}

/// A project that installs `rector/rector` and `acme/plain`, with the
/// package list Composer writes.
fn project(dir: &TempDir) {
    dir.write(
        "composer.json",
        r#"{"name": "acme/app", "require": {"rector/rector": "^1.0", "acme/plain": "^1.0"}}"#,
    );
    dir.write(
        "vendor/composer/installed.json",
        r#"{"packages": [
            {"name": "rector/rector", "type": "library", "install-path": "../rector/rector"},
            {"name": "acme/plain", "type": "library", "install-path": "../acme/plain"}
        ]}"#,
    );
    dir.write("vendor/acme/plain/src/Plain.php", "<?php class Plain {}");
}

#[test]
fn a_bundled_vendor_directory_of_an_installed_package_is_context() {
    let dir = TempDir::new("bundled_vendor");
    project(&dir);
    dir.write("vendor/rector/rector/composer.json", "{}");
    dir.write(
        "vendor/rector/rector/vendor/composer/autoload_real.php",
        "<?php",
    );
    dir.write(
        "vendor/rector/rector/vendor/composer/autoload_classmap.php",
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Bundled\\Thing' => $vendorDir . '/bundled/lib/Thing.php',
);
"#,
    );
    dir.write(
        "vendor/rector/rector/vendor/bundled/lib/Thing.php",
        "<?php class Thing {}",
    );

    let thing = canonicalize(
        &dir.path()
            .join("vendor/rector/rector/vendor/bundled/lib/Thing.php"),
    )
    .unwrap();
    assert_eq!(
        context(&dir).files,
        vec![VendorPath::Resolved(Resolved::resolve(&thing))]
    );
}

#[test]
fn a_package_without_a_bundled_vendor_directory_adds_no_context() {
    let dir = TempDir::new("unbundled_vendor");
    project(&dir);
    assert_eq!(context(&dir), VendorContext::default());
}
