//! `Project::apply_settings` stores the PHP version range onto
//! `registry::Registry::php_versions`.
use super::super::*;

fn with_php_versions(php_versions: Option<types::PhpVersions>) -> ProjectSettings {
    ProjectSettings {
        php_versions,
        ..ProjectSettings::default()
    }
}

#[test]
fn php_versions_default_to_unset() {
    let project = Project::new();
    assert_eq!(project.registry().php_versions, None);
}

#[test]
fn php_versions_are_stored_onto_the_registry() {
    let mut project = Project::new();
    let range = types::PhpVersions::exactly(types::PhpVersion::new(8, 1));
    project.apply_settings(with_php_versions(Some(range)));
    assert_eq!(project.registry().php_versions, Some(range));
}

#[test]
fn php_versions_are_cleared_by_settings_without_them() {
    let mut project = Project::new();
    project.apply_settings(with_php_versions(Some(types::PhpVersions::exactly(
        types::PhpVersion::new(8, 1),
    ))));
    project.apply_settings(with_php_versions(None));
    assert_eq!(project.registry().php_versions, None);
}
