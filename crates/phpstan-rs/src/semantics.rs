//! Adapter from the checker's current language-version setting to shared
//! catalog targets. Native platform facts remain unknown until configured.

pub use php_semantics::*;

/// Convert the existing major/minor language range into correlated full-minor
/// scenarios. The current checker setting has no patch/platform fields, so
/// this adapter deliberately leaves those target properties unknown.
pub fn target_set_from_php_versions(versions: Option<types::PhpVersions>) -> TargetSet {
    let known = types::KNOWN_PHP_VERSIONS;
    let min = versions.map_or_else(
        || *known.first().expect("PHP versions are defined"),
        |v| v.min,
    );
    let max = versions.map_or_else(
        || *known.last().expect("PHP versions are defined"),
        |v| v.max,
    );
    let requested = VersionRange {
        min: release(min),
        max: VersionRange::minor(max.major as u16, max.minor as u16).max,
    };
    let mut scenarios = Vec::new();
    let mut cursor = Some(requested.min);
    for version in known
        .iter()
        .copied()
        .filter(|version| min <= *version && *version <= max)
    {
        let supported = VersionRange::minor(version.major as u16, version.minor as u16);
        if let Some(start) = cursor.filter(|start| *start < supported.min) {
            if let Some(end) = previous_release(supported.min) {
                scenarios.push(unknown_scenario(VersionRange {
                    min: start,
                    max: end,
                }));
            }
        }
        scenarios.push(TargetScenario {
            release: supported,
            os: None,
            arch: None,
            sapi: None,
            zts: None,
            debug: None,
            extensions: None,
            ini: None,
        });
        cursor = next_minor(supported.max);
    }
    if let Some(start) = cursor.filter(|start| *start <= requested.max) {
        scenarios.push(unknown_scenario(VersionRange {
            min: start,
            max: requested.max,
        }));
    }
    TargetSet::new(scenarios).expect("known PHP versions form valid minor scenarios")
}

fn release(version: types::PhpVersion) -> ReleaseVersion {
    ReleaseVersion {
        major: version.major as u16,
        minor: version.minor as u16,
        patch: 0,
    }
}

fn unknown_scenario(release: VersionRange) -> TargetScenario {
    TargetScenario {
        release,
        os: None,
        arch: None,
        sapi: None,
        zts: None,
        debug: None,
        extensions: None,
        ini: None,
    }
}

fn previous_release(version: ReleaseVersion) -> Option<ReleaseVersion> {
    if version.patch > 0 {
        return Some(ReleaseVersion {
            patch: version.patch - 1,
            ..version
        });
    }
    if version.minor > 0 {
        return Some(ReleaseVersion {
            major: version.major,
            minor: version.minor - 1,
            patch: u16::MAX,
        });
    }
    (version.major > 0).then_some(ReleaseVersion {
        major: version.major - 1,
        minor: u16::MAX,
        patch: u16::MAX,
    })
}

fn next_minor(version: ReleaseVersion) -> Option<ReleaseVersion> {
    if version.minor < u16::MAX {
        return Some(ReleaseVersion {
            major: version.major,
            minor: version.minor + 1,
            patch: 0,
        });
    }
    (version.major < u16::MAX).then_some(ReleaseVersion {
        major: version.major + 1,
        minor: 0,
        patch: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{PhpVersion, PhpVersions};

    #[test]
    fn a_configured_minor_preserves_unknown_patch_and_platform() {
        let targets =
            target_set_from_php_versions(Some(PhpVersions::exactly(PhpVersion::new(8, 5))));
        assert_eq!(targets.scenarios.len(), 1);
        assert_eq!(targets.scenarios[0].release, VersionRange::minor(8, 5));
        assert_eq!(targets.scenarios[0].os, None);
        assert_eq!(targets.scenarios[0].extensions, None);
    }

    #[test]
    fn absent_version_setting_preserves_all_known_language_versions() {
        let targets = target_set_from_php_versions(None);
        assert_eq!(
            targets.scenarios.first().unwrap().release,
            VersionRange::minor(5, 3)
        );
        assert_eq!(
            targets.scenarios.last().unwrap().release,
            VersionRange::minor(8, 5)
        );
        assert!(
            targets
                .scenarios
                .iter()
                .all(|scenario| scenario.os.is_none())
        );
    }

    #[test]
    fn configured_unmodeled_minor_is_preserved_as_an_unknown_scenario() {
        let targets =
            target_set_from_php_versions(Some(PhpVersions::exactly(PhpVersion::new(8, 6))));
        assert_eq!(targets.scenarios.len(), 1);
        assert_eq!(targets.scenarios[0].release, VersionRange::minor(8, 6));
        assert!(targets.scenarios[0].extensions.is_none());
    }

    #[test]
    fn configured_range_keeps_unmodeled_tail_after_known_minors() {
        let targets = target_set_from_php_versions(Some(PhpVersions::new(
            PhpVersion::new(8, 4),
            PhpVersion::new(8, 6),
        )));
        assert_eq!(targets.scenarios.len(), 3);
        assert_eq!(targets.scenarios[0].release, VersionRange::minor(8, 4));
        assert_eq!(targets.scenarios[1].release, VersionRange::minor(8, 5));
        assert_eq!(targets.scenarios[2].release, VersionRange::minor(8, 6));

        let only_85 = TargetPredicate {
            release: Some(VersionRange::minor(8, 5)),
            ..TargetPredicate::any()
        };
        let selection = targets.select(&only_85);
        assert_eq!(selection.applicable.len(), 1);
        assert_eq!(selection.applicable[0].0.release, VersionRange::minor(8, 5));
        assert_eq!(selection.uncovered.len(), 2);
        assert_eq!(selection.uncovered[1].release, VersionRange::minor(8, 6));
    }
}
