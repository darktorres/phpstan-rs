//! What `ComposerManifest::load` reads from `composer.json` and
//! `vendor/composer/installed.json`: repeated keys, members of the wrong
//! shape, and the documents that `json_decode` accepts or rejects.

use super::super::*;
use super::helpers::*;
use crate::canonical::canonicalize;
use crate::manifest::{Autoload, Located};

/// Loads a project whose `composer.json` is `composer` and whose
/// `vendor/composer/installed.json` is `installed`, when given.
fn load(label: &str, composer: &str, installed: Option<&str>) -> Option<ComposerManifest> {
    let dir = TempDir::new(label);
    dir.write("composer.json", composer);
    if let Some(installed) = installed {
        dir.write("vendor/composer/installed.json", installed);
    }
    let root = canonicalize(dir.path()).unwrap();
    ComposerManifest::load(&root, &root)
}

/// The package names of a list of requirements.
fn packages(entries: &[Located<String>]) -> Vec<&str> {
    entries.iter().map(|entry| entry.value.as_str()).collect()
}

/// The `(package, constraint)` pairs of the manifest.
fn constraints(manifest: &ComposerManifest) -> Vec<(&str, &str)> {
    manifest
        .constraints
        .iter()
        .map(|(name, constraint)| (name.as_str(), constraint.as_str()))
        .collect()
}

/// The `(type, url)` pairs of the manifest's repositories.
fn repositories(manifest: &ComposerManifest) -> Vec<(&str, &str)> {
    manifest
        .repositories
        .iter()
        .map(|repo| (repo.kind.as_str(), repo.url.as_str()))
        .collect()
}

/// The names of the installed packages, in the order they were read.
fn installed_names(manifest: &ComposerManifest) -> Vec<&str> {
    manifest.installed.iter().map(|p| p.name.as_str()).collect()
}

/// `depth` nested empty arrays.
fn nested(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn a_repeated_key_keeps_its_last_value() {
    let manifest = load(
        "repeated_key",
        r#"{"name": "a/first", "name": "a/second", "require": {"x/y": "1.0", "x/y": "2.0"}}"#,
        None,
    )
    .unwrap();
    assert_eq!(manifest.name.as_deref(), Some("a/second"));
    assert_eq!(packages(&manifest.require), vec!["x/y"]);
    assert_eq!(constraints(&manifest), vec![("x/y", "2.0")]);
}

#[test]
fn a_repeated_section_keeps_its_last_value() {
    let manifest = load(
        "repeated_section",
        r#"{"require": {"a/b": "1"}, "require": {"c/d": "2"}}"#,
        None,
    )
    .unwrap();
    assert_eq!(packages(&manifest.require), vec!["c/d"]);
    assert_eq!(constraints(&manifest), vec![("c/d", "2")]);
}

#[test]
fn a_member_of_the_wrong_shape_is_skipped_and_the_rest_is_read() {
    let manifest = load(
        "wrong_shapes",
        r#"{"name": 5, "require": "x", "autoload": ["x"], "provide": [1],
            "suggest": "x", "scripts": [1], "config": "x", "repositories": 5,
            "require-dev": {"y/z": "3"}}"#,
        None,
    )
    .unwrap();
    assert_eq!(manifest.name, None);
    assert!(manifest.require.is_empty());
    assert_eq!(packages(&manifest.require_dev), vec!["y/z"]);
    assert_eq!(manifest.autoload, Autoload::default());
    assert!(manifest.provide.is_empty());
    assert!(manifest.suggest.is_empty());
    assert!(manifest.scripts.is_empty());
    assert_eq!(manifest.platform_php, None);
    assert!(manifest.repositories.is_empty());
}

#[test]
fn a_top_level_value_that_is_not_an_object_gives_an_empty_manifest() {
    for text in ["[]", "5", "\"text\"", "null", "true"] {
        let manifest = load("top_level", text, None).unwrap();
        assert_eq!(manifest.name, None, "{text}");
        assert!(manifest.require.is_empty(), "{text}");
        assert!(manifest.installed.is_empty(), "{text}");
    }
}

#[test]
fn text_that_is_not_json_gives_no_manifest() {
    for text in [
        "{\"name\": }",
        "",
        "{\"name\": \"x\"",
        "{\"name\": \"x\"} trailing",
    ] {
        assert!(load("not_json", text, None).is_none(), "{text:?}");
    }
}

#[test]
fn every_requirement_is_listed_and_only_string_constraints_are_kept() {
    let manifest = load(
        "requirements",
        r#"{"require": {"Vendor/Pkg": "^1.0", "x/y": 5, "z/w": null},
            "require-dev": {"Vendor/Pkg": "2.0"}}"#,
        None,
    )
    .unwrap();
    assert_eq!(
        packages(&manifest.require),
        vec!["Vendor/Pkg", "x/y", "z/w"]
    );
    assert_eq!(packages(&manifest.require_dev), vec!["Vendor/Pkg"]);
    assert_eq!(constraints(&manifest), vec![("vendor/pkg", "2.0")]);
}

#[test]
fn a_script_has_an_entry_for_each_string_command() {
    let manifest = load(
        "scripts",
        r#"{"scripts": {"a": "echo 1", "b": ["x", "y", 3], "c": 5, "d": {"x": 1}, "e": []}}"#,
        None,
    )
    .unwrap();
    let scripts: Vec<(&str, &str)> = manifest
        .scripts
        .iter()
        .map(|script| (script.value.0.as_str(), script.value.1.as_str()))
        .collect();
    assert_eq!(scripts, vec![("a", "echo 1"), ("b", "x"), ("b", "y")]);
}

#[test]
fn repositories_are_read_from_a_list_or_from_an_object_of_entries() {
    let list = load(
        "repositories_list",
        r#"{"repositories": [{"type": "path", "url": "packages/*"}, {"type": "vcs"}, "x", {"type": 1, "url": "y"}]}"#,
        None,
    )
    .unwrap();
    assert_eq!(repositories(&list), vec![("path", "packages/*")]);

    let object = load(
        "repositories_object",
        r#"{"repositories": {"a": {"type": "path", "url": "p"}, "b": 5}}"#,
        None,
    )
    .unwrap();
    assert_eq!(repositories(&object), vec![("path", "p")]);

    let last_is_object = load(
        "repositories_last",
        r#"{"repositories": [{"type": "path", "url": "a"}], "repositories": {"b": {"type": "path", "url": "b"}}}"#,
        None,
    )
    .unwrap();
    assert_eq!(repositories(&last_is_object), vec![("path", "b")]);
}

#[test]
fn installed_packages_are_read_from_a_list_or_from_an_object_with_packages() {
    let list = load(
        "installed_list",
        "{}",
        Some(r#"[{"name": "acme/b", "type": "library", "install-path": "../acme/b"}]"#),
    )
    .unwrap();
    assert_eq!(installed_names(&list), vec!["acme/b"]);

    let object = load(
        "installed_object",
        "{}",
        Some(r#"{"dev": true, "packages": [{"name": "acme/b"}]}"#),
    )
    .unwrap();
    assert_eq!(installed_names(&object), vec!["acme/b"]);

    for text in [
        "[]",
        "{}",
        "5",
        "null",
        r#"{"packages": {"a": {}}}"#,
        r#"{"packages": null}"#,
    ] {
        let manifest = load("installed_other", "{}", Some(text)).unwrap();
        assert!(manifest.installed.is_empty(), "{text}");
    }
}

#[test]
fn a_repeated_packages_key_keeps_its_last_value() {
    let last_is_text = load(
        "packages_last_text",
        "{}",
        Some(r#"{"packages": [{"name": "a/a"}], "packages": "x"}"#),
    )
    .unwrap();
    assert!(last_is_text.installed.is_empty());

    let last_is_list = load(
        "packages_last_list",
        "{}",
        Some(r#"{"packages": "x", "packages": [{"name": "a/a"}]}"#),
    )
    .unwrap();
    assert_eq!(installed_names(&last_is_list), vec!["a/a"]);
}

#[test]
fn an_installed_package_needs_a_name_and_defaults_its_kind_and_directory() {
    let manifest = load(
        "installed_defaults",
        "{}",
        Some(
            r#"[{"type": "library"}, {"name": 5}, {"name": null},
                {"name": "ok/one", "install-path": 5},
                {"name": "ok/two", "type": 1}, {"name": "ok/three", "type": "metapackage"}]"#,
        ),
    )
    .unwrap();
    let kinds: Vec<(&str, &str)> = manifest
        .installed
        .iter()
        .map(|p| (p.name.as_str(), p.kind.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("ok/one", "library"),
            ("ok/two", "library"),
            ("ok/three", "metapackage"),
        ]
    );
    let one = manifest.installed_package("ok/one").unwrap();
    assert_eq!(one.install_dir, manifest.root.join("vendor/ok/one"));
}

#[test]
fn installed_package_flags_and_lists_are_read_by_shape() {
    let manifest = load(
        "installed_flags",
        "{}",
        Some(
            r#"[
                {"name": "a/a", "extra": {"pest": {}}, "bin": ["a"],
                    "provide": {"z/z": 1, "b/b": 2}, "replace": {"y/y": "*", "c/c": "*"}},
                {"name": "b/b", "extra": {"laravel": {}}, "bin": [null],
                    "provide": 3, "replace": ["x"]},
                {"name": "c/c", "extra": {"phpstan": null}, "bin": []},
                {"name": "d/d", "extra": {"psalm": 1}, "bin": "x"},
                {"name": "e/e", "extra": []},
                {"name": "f/f", "extra": {"laravelx": {}}, "bin": {"a": 1}}
            ]"#,
        ),
    )
    .unwrap();
    let flags: Vec<(&str, bool, bool)> = manifest
        .installed
        .iter()
        .map(|p| (p.name.as_str(), p.auto_discovered, p.has_bin))
        .collect();
    assert_eq!(
        flags,
        vec![
            ("a/a", true, true),
            ("b/b", true, true),
            ("c/c", true, false),
            ("d/d", true, false),
            ("e/e", false, false),
            ("f/f", false, false),
        ]
    );
    let a = manifest.installed_package("a/a").unwrap();
    assert_eq!(a.provide, vec!["b/b", "z/z"]);
    assert_eq!(a.replace, vec!["c/c", "y/y"]);
    let b = manifest.installed_package("b/b").unwrap();
    assert!(b.provide.is_empty());
    assert!(b.replace.is_empty());
}

#[test]
fn composer_json_reads_up_to_511_levels_of_nesting() {
    // The object is one level; the arrays of `extra` are the rest.
    let read = load(
        "nesting_composer",
        &format!(r#"{{"name": "a/a", "extra": {}}}"#, nested(510)),
        None,
    )
    .unwrap();
    assert_eq!(read.name.as_deref(), Some("a/a"));

    let too_deep = load(
        "nesting_composer_deep",
        &format!(r#"{{"name": "a/a", "extra": {}}}"#, nested(511)),
        None,
    );
    assert!(too_deep.is_none());
}

#[test]
fn installed_json_has_the_same_nesting_limit() {
    let read = load(
        "nesting_installed",
        "{}",
        Some(format!(r#"[{{"name": "a/a", "unused": {}}}]"#, nested(509)).as_str()),
    )
    .unwrap();
    assert_eq!(installed_names(&read), vec!["a/a"]);

    let too_deep = load(
        "nesting_installed_deep",
        "{}",
        Some(format!(r#"[{{"name": "a/a", "unused": {}}}]"#, nested(510)).as_str()),
    )
    .unwrap();
    assert!(too_deep.installed.is_empty());
}

#[test]
fn brackets_inside_a_string_do_not_count_toward_the_nesting() {
    let composer = format!(r#"{{"name": "a/a", "description": "{}"}}"#, "[".repeat(600));
    let manifest = load("nesting_in_string", &composer, None).unwrap();
    assert_eq!(manifest.name.as_deref(), Some("a/a"));
}

#[test]
fn an_out_of_range_number_in_an_unread_member_does_not_hide_the_rest() {
    // `json_decode` reads `1e400` as INF, so the documents are read.
    let manifest = load(
        "out_of_range",
        r#"{"name": "a/a", "unused": 1e400, "require": {"x/y": "1.0"}}"#,
        Some(r#"[{"name": "b/b", "unused": 1e400}]"#),
    )
    .unwrap();
    assert_eq!(manifest.name.as_deref(), Some("a/a"));
    assert_eq!(packages(&manifest.require), vec!["x/y"]);
    assert_eq!(installed_names(&manifest), vec!["b/b"]);
}
