//! `Project::set_php_version` actually stores onto
//! `registry::Registry::php_version` - mirrors the existing
//! `set_strict_mixed_arrays`/`set_phpstan_api_check_enabled` precedent.
use super::super::*;

#[test]
fn defaults_to_unset() {
    let project = Project::new();
    assert_eq!(project.registry().php_version, None);
}

#[test]
fn set_php_version_stores_onto_the_registry() {
    let mut project = Project::new();
    project.set_php_version(Some(types::PhpVersion::new(8, 1)));
    assert_eq!(
        project.registry().php_version,
        Some(types::PhpVersion::new(8, 1))
    );
}

#[test]
fn set_php_version_can_be_cleared_back_to_none() {
    let mut project = Project::new();
    project.set_php_version(Some(types::PhpVersion::new(8, 1)));
    project.set_php_version(None);
    assert_eq!(project.registry().php_version, None);
}
