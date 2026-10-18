//! Which package owns a directory, and so the files in it: the root package,
//! or the installed package whose directory holds it.

use super::super::manifest::{lexically_normalized, resolve};
use super::super::*;
use super::helpers::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The package index of the project at `dir`, read as a session reads it.
fn index(dir: &TempDir) -> PackageIndex {
    let root = std::fs::canonicalize(dir.path()).unwrap();
    PackageIndex::new(
        PackageContext::load(&root, &root, &Arc::default(), None)
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
    let owner = index.owner_of_dir(file.parent().unwrap());
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
    let owner = index.owner_of_dir(file.parent().unwrap()).unwrap();
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
    let owner = index.owner_of_dir(file.parent().unwrap());
    assert_eq!(owner, Some(PackageOwner::Root(0)));
    assert_eq!(index.name(owner.unwrap()), None);
}

#[cfg(unix)]
#[test]
fn a_package_linked_into_the_vendor_directory_belongs_to_its_target() {
    let dir = TempDir::new("owner_linked");
    dir.write("composer.json", r#"{"name": "acme/app"}"#);
    dir.write("packages/b/src/B.php", "<?php");
    dir.write(
        "vendor/composer/installed.json",
        r#"{"packages": [{"name": "acme/b", "type": "library", "install-path": "../acme/b"}]}"#,
    );
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("vendor/acme")).unwrap();
    std::os::unix::fs::symlink(root.join("packages/b"), root.join("vendor/acme/b")).unwrap();
    let index = index(&dir);
    // A path is looked up resolved, so the link is followed to its target.
    let owner = index.owner_of_dir(&resolve(&root.join("vendor/acme/b/src")));
    assert_eq!(owner.and_then(|o| index.name(o)), Some("acme/b"));
    assert_eq!(index.owner_of_dir(&root.join("packages/b/src")), owner);
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

/// The directories `index` owns, in listing order: each root's own, then
/// each installed package's `vendor/` directory and its install directory.
fn owned_dirs(index: &PackageIndex) -> Vec<(PathBuf, PackageOwner)> {
    let mut dirs = Vec::new();
    for (i, manifest) in index.manifests().iter().enumerate() {
        dirs.push((manifest.absolute_root.clone(), PackageOwner::Root(i)));
        for (j, package) in manifest.installed.iter().enumerate() {
            let owner = PackageOwner::Installed(i, j);
            dirs.push((package.vendor_dir.clone(), owner));
            dirs.push((package.install_dir.clone(), owner));
        }
    }
    dirs
}

/// What `owner_of_dir` answers, by its definition: of the owned directories
/// that contain `dir` once normalized, the deepest, the last listed among
/// equals.
fn owner_by_definition(dirs: &[(PathBuf, PackageOwner)], dir: &Path) -> Option<PackageOwner> {
    let normalized = lexically_normalized(dir);
    dirs.iter()
        .enumerate()
        .filter(|(_, (owned, _))| normalized.starts_with(owned))
        .max_by_key(|(i, (owned, _))| (owned.components().count(), *i))
        .map(|(_, (_, owner))| *owner)
}

/// A project whose installed packages nest: `acme/lib-extra` is installed
/// inside `acme/lib`'s directory.
fn nested_packages(name: &str) -> TempDir {
    let dir = TempDir::new(name);
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
    dir
}

#[test]
fn the_owner_is_what_its_definition_gives_for_any_path() {
    let dir = nested_packages("owner_definition");
    let index = index(&dir);
    let dirs = owned_dirs(&index);
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
        let expected = owner_by_definition(&dirs, &file);
        let normalized = lexically_normalized(&file);
        assert_eq!(
            index.owner_of_dir(&normalized),
            expected,
            "{}",
            file.display()
        );
        checked += 1;
        owned += usize::from(expected.is_some());
    }
    // The generated paths reach owned directories and miss them, and some
    // belong to a package nested in another's directory.
    assert!(owned > 500 && owned < checked, "{owned} of {checked}");
}

#[test]
fn a_subdirectory_stands_where_its_directory_does_with_its_name() {
    let dir = nested_packages("owner_at");
    let index = index(&dir);
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let names = [
        "vendor", "phpstan", "acme", "lib", "extra", "src", "composer", "other",
    ];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut owned = 0;
    for _ in 0..2000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut path = match (state >> 40) % 3 {
            0 => root.parent().unwrap().to_path_buf(),
            1 => PathBuf::from("/"),
            _ => root.clone(),
        };
        let mut at = index.owner_at(&path);
        assert_eq!(at.owner(), index.owner_of_dir(&path), "{}", path.display());
        for _ in 0..(state >> 60) % 8 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let name = names[((state >> 33) % names.len() as u64) as usize];
            path.push(name);
            at = at.child(std::ffi::OsStr::new(name));
            assert_eq!(at.owner(), index.owner_of_dir(&path), "{}", path.display());
            owned += usize::from(at.owner().is_some());
        }
    }
    assert!(owned > 500, "the walks reach owned directories: {owned}");
}
