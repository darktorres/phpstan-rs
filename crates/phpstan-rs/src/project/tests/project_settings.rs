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

#[test]
fn patch_aware_target_reaches_registry_selection_without_platform_inference() {
    let mut project = Project::new();
    let release = types::PhpRelease::new(8, 5, 4);
    project.apply_settings(ProjectSettings {
        php_versions: Some(types::PhpVersions::exactly(types::PhpVersion::new(8, 5))),
        php_releases: Some(types::PhpReleaseRange::new(release, release)),
        ..ProjectSettings::default()
    });
    let target = project.registry().semantic_target_set();
    assert_eq!(target.scenarios.len(), 1);
    assert_eq!(
        target.scenarios[0].release,
        crate::semantics::VersionRange::exact(crate::semantics::ReleaseVersion {
            major: 8,
            minor: 5,
            patch: 4,
        })
    );
    assert!(target.scenarios[0].os.is_none());
    assert!(target.scenarios[0].sapi.is_none());
}

#[test]
fn registry_canonical_identity_includes_patch_aware_target_releases() {
    let make = |patch| {
        let mut project = Project::new();
        project.apply_settings(ProjectSettings {
            php_versions: Some(types::PhpVersions::exactly(types::PhpVersion::new(8, 5))),
            php_releases: Some(types::PhpReleaseRange::new(
                types::PhpRelease::new(8, 5, patch),
                types::PhpRelease::new(8, 5, patch),
            )),
            ..ProjectSettings::default()
        });
        syntax::canonical::print(project.registry())
    };
    assert_ne!(make(4), make(5));
}
