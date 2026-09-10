//! Phase 40.6: `find_composer_package_name`'s own upward-walk, used by
//! `checker::phpstan_api::PhpStanNamespaceIn3rdPartyPackageRule`.

use super::super::*;
use super::helpers::*;

#[test]
fn finds_the_name_in_the_starting_directory_itself() {
    let dir = TempDir::new("pkg_name_here");
    dir.write("composer.json", r#"{"name": "acme/widgets"}"#);
    assert_eq!(
        find_composer_package_name(dir.path()),
        Some("acme/widgets".to_string())
    );
}

#[test]
fn walks_upward_to_find_an_ancestor_composer_json() {
    let dir = TempDir::new("pkg_name_ancestor");
    dir.write("composer.json", r#"{"name": "acme/widgets"}"#);
    let nested = dir.path().join("src").join("Deep").join("Nested");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        find_composer_package_name(&nested),
        Some("acme/widgets".to_string())
    );
}

#[test]
fn a_composer_json_with_no_name_field_is_not_found() {
    let dir = TempDir::new("pkg_name_no_name_field");
    dir.write("composer.json", r#"{"require": {}}"#);
    assert_eq!(find_composer_package_name(dir.path()), None);
}

#[test]
fn a_malformed_composer_json_stops_the_search_rather_than_walking_further_up() {
    // Mirrors real PHPStan's own behavior exactly: a `composer.json` that
    // exists but fails to parse returns `null` from that one call - it
    // does not retry a parent directory, even if that parent directory
    // has its own, perfectly valid `composer.json`.
    let dir = TempDir::new("pkg_name_malformed_stops");
    dir.write("composer.json", r#"{"name": "acme/parent"}"#);
    let nested = dir.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("composer.json"), "not valid json").unwrap();
    assert_eq!(find_composer_package_name(&nested), None);
}

#[test]
fn no_composer_json_anywhere_up_to_the_root_is_not_found() {
    let dir = TempDir::new("pkg_name_none_anywhere");
    assert_eq!(find_composer_package_name(dir.path()), None);
}
