//! Which package owns a file: the root package, or the installed package
//! whose directory holds it.

use super::super::*;
use super::helpers::*;

/// The package index of the project at `dir`, read as a session reads it.
fn index(dir: &TempDir) -> PackageIndex {
    let root = std::fs::canonicalize(dir.path()).unwrap();
    PackageIndex::new(
        PackageContext::load(&root, &root)
            .manifest
            .into_iter()
            .collect(),
    )
}

#[test]
fn a_file_under_the_root_belongs_to_the_root_package() {
    let dir = TempDir::new("owner_root");
    dir.write("composer.json", r#"{"name": "acme/widgets"}"#);
    let index = index(&dir);
    let file = std::fs::canonicalize(dir.path())
        .unwrap()
        .join("src/Deep/Widget.php");
    let owner = index.owner(&file);
    assert_eq!(owner, Some(PackageOwner::Root(0)));
    assert_eq!(index.name(owner.unwrap()), Some("acme/widgets"));
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
    dir.write("vendor/phpstan/phpstan/src/Rules/Rule.php", "<?php");
    let index = index(&dir);
    let file = std::fs::canonicalize(dir.path())
        .unwrap()
        .join("vendor/phpstan/phpstan/src/Rules/Rule.php");
    let owner = index.owner(&file).unwrap();
    assert_eq!(index.name(owner), Some("phpstan/phpstan"));
    assert_eq!(
        index.installed(owner).map(|p| p.name.as_str()),
        Some("phpstan/phpstan")
    );
}

#[test]
fn a_root_package_with_no_name_owns_its_files_but_has_no_name() {
    let dir = TempDir::new("owner_no_name");
    dir.write("composer.json", r#"{"require": {}}"#);
    let index = index(&dir);
    let file = std::fs::canonicalize(dir.path()).unwrap().join("src/A.php");
    let owner = index.owner(&file);
    assert_eq!(owner, Some(PackageOwner::Root(0)));
    assert_eq!(index.name(owner.unwrap()), None);
}

#[test]
fn a_path_spelled_from_the_root_resolves_against_the_canonical_root() {
    let manifest = ComposerManifest {
        root: PathBuf::from("."),
        absolute_root: PathBuf::from("/project"),
        ..ComposerManifest::default()
    };
    assert_eq!(
        manifest.absolute(Path::new("./src/../lib/A.php")),
        PathBuf::from("/project/lib/A.php")
    );
    assert_eq!(
        manifest.absolute(Path::new("/elsewhere/./B.php")),
        PathBuf::from("/elsewhere/B.php")
    );
}
