//! `Project::set_php_versions` stores onto
//! `registry::Registry::php_versions`.
use super::super::*;

#[test]
fn defaults_to_unset() {
    let project = Project::new();
    assert_eq!(project.registry().php_versions, None);
}

#[test]
fn set_php_versions_stores_onto_the_registry() {
    let mut project = Project::new();
    let range = types::PhpVersions::exactly(types::PhpVersion::new(8, 1));
    project.set_php_versions(Some(range));
    assert_eq!(project.registry().php_versions, Some(range));
}

#[test]
fn set_php_versions_can_be_cleared_back_to_none() {
    let mut project = Project::new();
    project.set_php_versions(Some(types::PhpVersions::exactly(types::PhpVersion::new(
        8, 1,
    ))));
    project.set_php_versions(None);
    assert_eq!(project.registry().php_versions, None);
}
