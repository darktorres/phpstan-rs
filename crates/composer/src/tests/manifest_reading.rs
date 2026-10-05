//! What `composer.json` and `installed.json` are read as, in the shapes
//! Composer writes and in the malformed ones it does not.

use super::super::manifest::*;
use super::helpers::*;
use std::path::PathBuf;

fn load(dir: &TempDir) -> ComposerManifest {
    let root = std::fs::canonicalize(dir.path()).unwrap();
    ComposerManifest::load(&root, &root).unwrap()
}

fn installed(dir: &TempDir, json: &str) -> Vec<InstalledPackage> {
    dir.write("composer.json", "{}");
    dir.write("vendor/composer/installed.json", json);
    load(dir).installed
}

#[test]
fn installed_packages_come_from_the_packages_list_or_a_bare_list() {
    let dir = TempDir::new("manifest_installed_shapes");
    let wrapped = installed(
        &dir,
        r#"{"packages": [{"name": "a/b", "type": "project", "install-path": "../a/b",
            "bin": ["bin/x"], "extra": {"laravel": {"providers": []}},
            "provide": {"z/z": "1", "c/c": "1"}, "replace": {"r/r": "self.version"}}],
            "dev": true}"#,
    );
    assert_eq!(wrapped.len(), 1);
    let package = &wrapped[0];
    assert_eq!(package.name, "a/b");
    assert_eq!(package.kind, "project");
    assert!(package.has_bin);
    assert!(package.auto_discovered);
    assert_eq!(package.provide, ["c/c", "z/z"]);
    assert_eq!(package.replace, ["r/r"]);
    assert!(package.vendor_dir.ends_with("vendor/a/b"));
    assert!(package.install_dir.ends_with("composer/../a/b"));

    let bare = installed(&dir, r#"[{"name": "a/b"}]"#);
    assert_eq!(bare.len(), 1);
    assert_eq!(bare[0].kind, "library");
    assert!(!bare[0].has_bin);
    assert!(!bare[0].auto_discovered);
    assert!(bare[0].install_dir.ends_with("vendor/a/b"));
}

#[test]
fn installed_json_that_is_not_a_package_list_has_no_packages() {
    let dir = TempDir::new("manifest_installed_not_list");
    for json in [
        "",
        "not json",
        "42",
        "\"packages\"",
        "{}",
        r#"{"packages": 3}"#,
        r#"{"packages": {"a": {"name": "a/b"}}}"#,
    ] {
        assert!(installed(&dir, json).is_empty(), "{json}");
    }
}

#[test]
fn a_package_that_cannot_be_read_is_skipped_and_the_others_kept() {
    let dir = TempDir::new("manifest_installed_skips");
    let packages = installed(
        &dir,
        r#"{"packages": [
            "text", 7, null, [1, 2], {"type": "library"}, {"name": 5},
            {"name": "ok/one", "type": 3, "install-path": 4, "bin": "x",
             "extra": [1], "provide": ["a"], "replace": 9},
            {"name": "ok/two", "extra": {"psalm": 1}, "bin": []}
        ]}"#,
    );
    let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["ok/one", "ok/two"]);
    assert_eq!(packages[0].kind, "library");
    assert!(!packages[0].has_bin);
    assert!(!packages[0].auto_discovered);
    assert!(packages[0].provide.is_empty());
    assert!(packages[0].replace.is_empty());
    assert!(packages[0].install_dir.ends_with("vendor/ok/one"));
    assert!(packages[1].auto_discovered);
    assert!(!packages[1].has_bin);
}

#[test]
fn a_repeated_key_takes_its_last_value_and_an_escaped_one_is_its_text() {
    let dir = TempDir::new("manifest_installed_keys");
    let packages = installed(
        &dir,
        r#"{"packages": [], "packages": [
            {"name": "first/one", "name": "last/one", "type": "metapackage"}]}"#,
    );
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].name, "last/one");
    assert_eq!(packages[0].kind, "metapackage");
}

#[test]
fn composer_json_is_read_into_its_sections() {
    let dir = TempDir::new("manifest_composer_json");
    dir.write(
        "composer.json",
        r#"{
    "name": "acme/app",
    "require": {"php": "^8.1", "b/b": "^1", "A/A": 3},
    "require-dev": {"d/d": "*"},
    "autoload": {
        "psr-4": {"App\\": "src/", "Lib\\": ["lib/", "more/", 7]},
        "classmap": "classes",
        "files": ["a.php", 9],
        "exclude-from-classmap": ["/gen/*"]
    },
    "autoload-dev": {"psr-4": {"T\\": "tests"}},
    "provide": {"p/p": "1"},
    "suggest": {"s/s": "why", "a/a": "x"},
    "scripts": {"test": "phpunit", "ci": ["a", "b", 1], "n": 5},
    "config": {"platform": {"php": "8.2.1"}},
    "repositories": [{"type": "path", "url": "pkgs/*"}, 4, {"type": "vcs"}]
}"#,
    );
    let manifest = load(&dir);
    let root = manifest.root.clone();
    assert_eq!(manifest.name.as_deref(), Some("acme/app"));
    let required: Vec<(&str, u32)> = manifest
        .require
        .iter()
        .map(|r| (r.value.as_str(), r.line))
        .collect();
    assert_eq!(required, [("A/A", 3), ("b/b", 3), ("php", 3)]);
    assert_eq!(manifest.require_dev[0].value, "d/d");
    assert_eq!(manifest.constraints["b/b"], "^1");
    assert_eq!(manifest.constraints["php"], "^8.1");
    assert!(!manifest.constraints.contains_key("a/a"));
    assert_eq!(
        manifest.autoload.psr4,
        vec![
            ("App\\".to_string(), vec![root.join("src")]),
            (
                "Lib\\".to_string(),
                vec![root.join("lib"), root.join("more")]
            ),
        ]
    );
    assert_eq!(manifest.autoload.classmap, vec![root.join("classes")]);
    assert_eq!(manifest.autoload.files, vec![root.join("a.php")]);
    assert_eq!(manifest.autoload.excluded, vec![root.join("gen")]);
    assert_eq!(
        manifest.autoload_dev.psr4,
        vec![("T\\".to_string(), vec![root.join("tests")])]
    );
    assert_eq!(manifest.provide[0].value, "p/p");
    assert_eq!(manifest.suggest, ["a/a", "s/s"]);
    let scripts: Vec<(&str, &str, u32)> = manifest
        .scripts
        .iter()
        .map(|s| (s.value.0.as_str(), s.value.1.as_str(), s.line))
        .collect();
    assert_eq!(
        scripts,
        [("ci", "a", 14), ("ci", "b", 14), ("test", "phpunit", 14)]
    );
    let platform = manifest.platform_php.as_ref().unwrap();
    assert_eq!((platform.value.as_str(), platform.line), ("8.2.1", 15));
    let repositories: Vec<(&str, &str)> = manifest
        .repositories
        .iter()
        .map(|r| (r.kind.as_str(), r.url.as_str()))
        .collect();
    assert_eq!(repositories, [("path", "pkgs/*")]);
}

#[test]
fn repositories_keyed_by_name_are_read_by_key_order() {
    let dir = TempDir::new("manifest_repositories_object");
    dir.write(
        "composer.json",
        r#"{"repositories": {"z": {"type": "path", "url": "z"}, "a": {"type": "vcs", "url": "a"},
            "packagist.org": false}}"#,
    );
    let urls: Vec<String> = load(&dir).repositories.into_iter().map(|r| r.url).collect();
    assert_eq!(urls, ["a", "z"]);
}

#[test]
fn a_composer_json_that_is_not_an_object_is_still_a_manifest_of_nothing() {
    let dir = TempDir::new("manifest_composer_not_object");
    for json in ["[]", "3", "\"x\"", "null"] {
        dir.write("composer.json", json);
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let manifest = ComposerManifest::load(&root, &root).unwrap();
        assert_eq!(manifest.name, None, "{json}");
        assert!(manifest.require.is_empty(), "{json}");
    }
    dir.write("composer.json", "{");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    assert!(ComposerManifest::load(&root, &root).is_none());
}

#[test]
fn an_unpathable_install_directory_falls_back_to_the_vendor_one() {
    let dir = TempDir::new("manifest_installed_missing_dir");
    let packages = installed(&dir, r#"[{"name": "a/b", "install-path": "../missing/b"}]"#);
    let expected: PathBuf = std::fs::canonicalize(dir.path())
        .unwrap()
        .join("vendor/composer/../missing/b");
    assert_eq!(packages[0].install_dir, expected);
}
