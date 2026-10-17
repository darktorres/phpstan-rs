//! Which package owns a file: the root package, or the installed package
//! whose directory holds it.

use super::super::*;
use super::helpers::*;
use std::sync::Arc;

/// The package index of the project at `dir`, read as a session reads it.
fn index(dir: &TempDir) -> PackageIndex {
    let root = std::fs::canonicalize(dir.path()).unwrap();
    PackageIndex::new(
        PackageContext::load(&root, &root, &Arc::default())
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

/// What `owner` answers, by its definition: of the owned directories that
/// are an ancestor of `file` as written or of it normalized, the deepest,
/// the last listed among equals.
fn owner_by_definition(index: &PackageIndex, file: &Path) -> Option<PackageOwner> {
    let normalized = lexically_normalized(file);
    index
        .dirs
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            file.ancestors().any(|a| a == d.dir.as_path())
                || normalized.ancestors().any(|a| a == d.normalized.as_path())
        })
        .max_by_key(|(i, d)| (d.depth, *i))
        .map(|(_, d)| d.owner)
}

#[test]
fn the_owner_is_what_its_definition_gives_for_any_path() {
    let dir = TempDir::new("owner_definition");
    dir.write(
        "composer.json",
        r#"{"name": "acme/app", "require": {"phpstan/phpstan": "^2.0", "acme/lib": "*"}}"#,
    );
    dir.write(
        "vendor/composer/installed.json",
        r#"{"packages": [
            {"name": "phpstan/phpstan", "type": "library", "install-path": "../phpstan/phpstan"},
            {"name": "acme/lib", "type": "library", "install-path": "../acme/lib/./"},
            {"name": "acme/lib-extra", "type": "library", "install-path": "../acme/lib/extra"}
        ]}"#,
    );
    let index = index(&dir);
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let parts = [
        "vendor", "phpstan", "acme", "lib", "extra", "src", "..", ".", "x.php", "composer",
        "other", "",
    ];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut checked = 0;
    let mut owned = 0;
    for _ in 0..4000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut file = match (state >> 40) % 4 {
            0 => root.parent().unwrap().to_path_buf(),
            1 => PathBuf::from("/"),
            2 => PathBuf::from("relative"),
            _ => root.clone(),
        };
        for _ in 0..(state >> 60) % 9 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            file.push(parts[((state >> 33) % parts.len() as u64) as usize]);
        }
        let expected = owner_by_definition(&index, &file);
        assert_eq!(index.owner(&file), expected, "{}", file.display());
        checked += 1;
        owned += usize::from(expected.is_some());
    }
    // The generated paths reach owned directories and miss them, and some
    // belong to a package nested in another's directory.
    assert!(owned > 500 && owned < checked, "{owned} of {checked}");
}
