//! Which package owns a file: the root package, or the installed package
//! whose directory holds it.

use super::super::*;
use super::helpers::*;

#[test]
fn a_file_under_the_root_belongs_to_the_root_package() {
    let dir = TempDir::new("owner_root");
    dir.write("composer.json", r#"{"name": "acme/widgets"}"#);
    let dirs = PackageContext::load(dir.path(), dir.path()).package_dirs();
    let file = dir.path().join("src/Deep/Widget.php");
    assert_eq!(owning_package(&dirs, &file), Some("acme/widgets"));
}

#[test]
fn a_file_of_an_installed_package_belongs_to_it() {
    let dir = TempDir::new("owner_installed");
    dir.write(
        "composer.json",
        r#"{"name": "acme/app", "require": {"phpstan/phpstan": "^2.0"}}"#,
    );
    dir.write(
        "vendor/composer/installed.json",
        r#"{"packages": [{"name": "phpstan/phpstan", "type": "library", "install-path": "../phpstan/phpstan"}]}"#,
    );
    let file = dir.write("vendor/phpstan/phpstan/src/Rules/Rule.php", "<?php");
    let dirs = PackageContext::load(dir.path(), dir.path()).package_dirs();
    assert_eq!(owning_package(&dirs, &file), Some("phpstan/phpstan"));
}

#[test]
fn a_root_package_with_no_name_owns_nothing() {
    let dir = TempDir::new("owner_no_name");
    dir.write("composer.json", r#"{"require": {}}"#);
    let dirs = PackageContext::load(dir.path(), dir.path()).package_dirs();
    assert_eq!(owning_package(&dirs, &dir.path().join("src/A.php")), None);
}
