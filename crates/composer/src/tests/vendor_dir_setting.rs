//! Where a project's vendor context is read when Composer's `vendor-dir`,
//! `COMPOSER_VENDOR_DIR` and `COMPOSER` settings name another directory or
//! file, and when a setting names a directory Composer has not built.

use super::helpers::*;
use crate::PackageContext;
use crate::canonical::canonicalize;
use crate::dir_listings::DirListings;
use crate::hashed_path::Resolved;
use crate::layout::{Variables, no_variables};
use crate::vendor::VendorPath;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

/// An environment whose only variable is `name`, set to `value`.
fn setting(name: &'static str, value: &'static str) -> impl Fn(&OsStr) -> Option<OsString> + Sync {
    move |asked| (asked == OsStr::new(name)).then(|| OsString::from(value))
}

/// Writes the vendor directory Composer built at `vendor_dir`: its generated
/// PSR-4 map lists the source directory of one class, and its loader is the
/// file every dump writes, which is how Composer is seen to have built there.
fn built_at(dir: &TempDir, vendor_dir: &str) {
    dir.write(
        &format!("{vendor_dir}/acme/lib/src/Bar.php"),
        "<?php class Bar {}",
    );
    dir.write(
        &format!("{vendor_dir}/composer/autoload_real.php"),
        "<?php\nclass ComposerAutoloaderInit {}\n",
    );
    dir.write(
        &format!("{vendor_dir}/composer/autoload_classmap.php"),
        "<?php\nreturn array();\n",
    );
    dir.write(
        &format!("{vendor_dir}/composer/autoload_psr4.php"),
        r#"<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'Acme\\' => array($vendorDir . '/acme/lib/src'),
);
"#,
    );
}

/// The project at `dir`, loaded with the environment `variables`.
fn load(dir: &TempDir, variables: Variables<'_>) -> PackageContext {
    let root = canonicalize(dir.path()).unwrap();
    PackageContext::load(
        &root,
        &root,
        &Arc::new(DirListings::default()),
        variables,
        None,
    )
}

/// The one source directory the generated map of `vendor_dir` lists.
fn src_dir(dir: &TempDir, vendor_dir: &str) -> VendorPath {
    let src = canonicalize(&dir.path().join(vendor_dir).join("acme/lib/src")).unwrap();
    VendorPath::Resolved(Resolved::resolve(&src))
}

#[test]
fn the_vendor_dir_setting_names_the_directory_whose_context_is_read() {
    let dir = TempDir::new("vendor_dir_setting");
    dir.write("composer.json", r#"{"config": {"vendor-dir": "libs"}}"#);
    built_at(&dir, "libs");
    let package = load(&dir, &no_variables);
    assert_eq!(package.vendor.get().dirs, vec![src_dir(&dir, "libs")]);
}

#[test]
fn composer_vendor_dir_is_read_before_the_manifest_setting() {
    let dir = TempDir::new("composer_vendor_dir");
    dir.write("composer.json", r#"{"config": {"vendor-dir": "nowhere"}}"#);
    built_at(&dir, "libs");
    let variables = setting("COMPOSER_VENDOR_DIR", "libs");
    let package = load(&dir, &variables);
    assert_eq!(package.vendor.get().dirs, vec![src_dir(&dir, "libs")]);
}

#[test]
fn a_variable_naming_an_unbuilt_directory_keeps_the_built_vendor_directory() {
    let dir = TempDir::new("unbuilt_variable");
    dir.write("composer.json", "{}");
    built_at(&dir, "vendor");
    let variables = setting("COMPOSER_VENDOR_DIR", "not-built");
    let package = load(&dir, &variables);
    assert_eq!(package.vendor.get().dirs, vec![src_dir(&dir, "vendor")]);
}

#[test]
fn the_composer_variable_names_the_manifest_that_holds_the_setting() {
    let dir = TempDir::new("composer_file");
    dir.write("project.json", r#"{"config": {"vendor-dir": "libs"}}"#);
    built_at(&dir, "libs");
    let variables = setting("COMPOSER", "project.json");
    let package = load(&dir, &variables);
    assert!(package.manifest.is_some());
    assert_eq!(package.vendor.get().dirs, vec![src_dir(&dir, "libs")]);
}
