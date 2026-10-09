//! Adapter from the checker's current language-version setting to shared
//! catalog targets. Native platform facts remain unknown until configured.

pub use php_semantics::*;

/// The first reviewed/source-linked behavior artifact shipped with the
/// checker. Runtime ingestion still passes through `Catalog::from_json`,
/// which validates its schema and evidence graph before any call can use it.
pub(crate) fn embedded_catalog() -> &'static Catalog {
    static CATALOG: std::sync::LazyLock<Catalog> = std::sync::LazyLock::new(|| {
        Catalog::from_json(include_str!(
            "../../php-semantics/data/php-8.5.4-strlen.json"
        ))
        .expect("embedded PHP semantic catalog must validate")
    });
    &CATALOG
}

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

/// Preserve configured patch endpoints for catalog selection while keeping
/// native platform/profile facts unknown unless separately configured.
pub fn target_set_from_php_releases(releases: Option<types::PhpReleaseRange>) -> TargetSet {
    let Some(releases) = releases else {
        return target_set_from_php_versions(None);
    };
    let release = |version: types::PhpRelease| ReleaseVersion {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
    };
    TargetSet::new(vec![TargetScenario {
        release: VersionRange {
            min: release(releases.min),
            max: release(releases.max),
        },
        os: None,
        arch: None,
        sapi: None,
        zts: None,
        debug: None,
        extensions: None,
        ini: None,
    }])
    .expect("configured PHP release range is ordered")
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
mod embedded_catalog_tests {
    use super::*;
    use types::{PhpRelease, PhpReleaseRange};

    fn key() -> EntryKey {
        EntryKey::Function {
            extension: Bytes(b"standard".to_vec()),
            name: Bytes(b"strlen".to_vec()),
        }
    }

    #[test]
    fn embedded_strlen_artifact_is_patch_exact_and_leaves_other_targets_uncovered() {
        let catalog = embedded_catalog();
        let manifest = catalog.manifest();
        let source_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| {
                record.site.as_ref().is_some_and(|site| {
                    site.0 == b"Zend/zend_builtin_functions.c:ZEND_FUNCTION(strlen):ZSTR_LEN(s)"
                })
            })
            .expect("strlen entry retains its source transfer evidence");
        assert_eq!(
            source_record.source_digest,
            Digest([
                0x0d, 0x44, 0x77, 0x17, 0x0a, 0x78, 0x54, 0x01, 0x31, 0x89, 0x3a, 0x8c, 0xfc, 0xa2,
                0xb9, 0xd1, 0x03, 0x3c, 0x79, 0xea, 0xb7, 0x54, 0xf9, 0xca, 0x3d, 0x85, 0xe0, 0x5a,
                0xe6, 0x91, 0x95, 0x05,
            ])
        );
        let exact = target_set_from_php_releases(Some(PhpReleaseRange::new(
            PhpRelease::new(8, 5, 4),
            PhpRelease::new(8, 5, 4),
        )));
        let selection = catalog.select_for_targets(&key(), &exact);
        assert_eq!(selection.variants.len(), 1);
        assert!(selection.uncovered.is_empty());
        assert_eq!(selection.variants[0].applicability, Applicability::Yes);
        assert!(selection.variants[0].graph.coverage.iter().any(|coverage| {
            coverage.domain == Domain::Return && matches!(coverage.coverage, Coverage::Closed)
        }));

        let minor = target_set_from_php_releases(Some(PhpReleaseRange::new(
            PhpRelease::new(8, 5, 0),
            PhpRelease::new(8, 5, u16::MAX),
        )));
        let selection = catalog.select_for_targets(&key(), &minor);
        assert_eq!(selection.variants.len(), 1);
        assert_eq!(selection.variants[0].applicability, Applicability::Maybe);
        assert_eq!(selection.uncovered.len(), 1);

        let earlier = target_set_from_php_releases(Some(PhpReleaseRange::new(
            PhpRelease::new(8, 5, 0),
            PhpRelease::new(8, 5, 3),
        )));
        let selection = catalog.select_for_targets(&key(), &earlier);
        assert!(selection.variants.is_empty());
        assert_eq!(selection.uncovered, earlier.scenarios);
    }
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

    #[test]
    fn configured_patch_release_remains_exact_for_catalog_selection() {
        let targets = target_set_from_php_releases(Some(types::PhpReleaseRange::new(
            types::PhpRelease::new(8, 5, 4),
            types::PhpRelease::new(8, 5, 4),
        )));
        assert_eq!(targets.scenarios.len(), 1);
        assert_eq!(
            targets.scenarios[0].release,
            VersionRange::exact(ReleaseVersion {
                major: 8,
                minor: 5,
                patch: 4,
            })
        );
        assert!(targets.scenarios[0].os.is_none());
    }
}
