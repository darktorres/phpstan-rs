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
        let source_id = source_record.id().expect("source evidence encodes");
        let layout_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| record.rule_version.0 == b"php-8.5.4-zend-string-layout-v1")
            .expect("strlen Return has Zend layout evidence");
        let layout_id = layout_record.id().expect("layout evidence encodes");
        let parser_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| record.rule_version.0 == b"php-8.5.4-zend-string-parser-source-v1")
            .expect("strlen Return has Zend parser source evidence");
        let parser_id = parser_record.id().expect("parser evidence encodes");
        let diagnostic_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| {
                record.rule_version.0 == b"php-8.5.4-strlen-null-deprecation-observation-v1"
            })
            .expect("strlen Diagnostic has paired runtime evidence");
        let diagnostic_id = diagnostic_record
            .id()
            .expect("diagnostic observation evidence encodes");
        assert_eq!(diagnostic_record.kind, EvidenceKind::Observation);
        assert!(diagnostic_record.dependencies.contains(&parser_id));
        let cast_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| record.rule_version.0 == b"php-8.5.4-zend-stringable-cast-invocation-v1")
            .expect("Stringable invocation cites the Zend handler source");
        assert_eq!(cast_record.kind, EvidenceKind::SourceTransfer);
        assert!(cast_record.dependencies.contains(&parser_id));
        let cast_id = cast_record.id().expect("cast evidence encodes");
        let callback_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| {
                record.rule_version.0 == b"php-8.5.4-strlen-stringable-callback-observation-v2"
            })
            .expect("Stringable invocation has paired runtime observation evidence");
        assert_eq!(callback_record.kind, EvidenceKind::Observation);
        assert!(callback_record.dependencies.contains(&cast_id));
        let callback_id = callback_record.id().expect("callback evidence encodes");
        let inventory_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| {
                record.rule_version.0 == b"php-8.5.4-clang-21.1.8-strlen-opcode-inventory-v1"
            })
            .expect("strlen Return has complete LLVM operation-inventory evidence");
        assert_eq!(inventory_record.kind, EvidenceKind::SourceTransfer);
        assert_eq!(
            inventory_record.source_digest,
            Digest([
                0xe3, 0x42, 0x7c, 0xe8, 0x1f, 0x98, 0xf9, 0xa0, 0x4d, 0xb3, 0xe0, 0x4c, 0xe8, 0x22,
                0xc3, 0xe0, 0xf1, 0xc7, 0x31, 0x13, 0x7f, 0x0d, 0x63, 0xe0, 0x44, 0x0f, 0x86, 0x2b,
                0x1e, 0x61, 0xc9, 0x3e,
            ])
        );
        let inventory_id = inventory_record.id().expect("inventory evidence encodes");
        let native_demand_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| {
                record.rule_version.0 == b"php-8.5.4-clang-21.1.8-strlen-native-return-demand-v1"
            })
            .expect("strlen Return has a source-linked native demand record");
        assert_eq!(native_demand_record.kind, EvidenceKind::SourceTransfer);
        assert_eq!(
            native_demand_record.source_digest,
            inventory_record.source_digest
        );
        assert!(native_demand_record.dependencies.contains(&source_id));
        assert!(native_demand_record.dependencies.contains(&layout_id));
        assert!(native_demand_record.dependencies.contains(&parser_id));
        assert!(native_demand_record.dependencies.contains(&inventory_id));
        let native_demand_id = native_demand_record
            .id()
            .expect("native demand evidence encodes");
        let llvm_record = manifest
            .evidence
            .iter()
            .map(|(_, record)| record)
            .find(|record| {
                record.rule_version.0 == b"php-8.5.4-clang-21.1.8-strlen-return-v2-inventoried"
            })
            .expect("strlen Return has linked-module evidence");
        assert_eq!(llvm_record.kind, EvidenceKind::SourceTransfer);
        assert!(llvm_record.dependencies.contains(&source_id));
        assert!(llvm_record.dependencies.contains(&layout_id));
        assert!(llvm_record.dependencies.contains(&parser_id));
        assert!(llvm_record.dependencies.contains(&inventory_id));
        assert!(llvm_record.dependencies.contains(&native_demand_id));
        assert_eq!(
            parser_record.source_digest,
            Digest([
                0xba, 0x6d, 0x16, 0x3b, 0x75, 0x90, 0x5a, 0x3f, 0xc1, 0x22, 0x18, 0x64, 0x52, 0xa4,
                0xcc, 0x08, 0xab, 0x06, 0xb2, 0x5e, 0x7f, 0xc5, 0x1b, 0x22, 0xd8, 0x30, 0xb3, 0x8d,
                0x5d, 0xac, 0xa2, 0x3b,
            ])
        );
        assert_eq!(
            layout_record.source_digest,
            Digest([
                0x60, 0x1e, 0xd3, 0x37, 0x20, 0x5b, 0x22, 0x58, 0xff, 0x0d, 0xe2, 0x17, 0xd4, 0x0a,
                0xaf, 0x38, 0xaf, 0xd3, 0xa5, 0x2f, 0x67, 0xa8, 0xed, 0xf0, 0xf9, 0x11, 0x82, 0x41,
                0x4a, 0x6b, 0xcc, 0xcd,
            ])
        );
        assert_eq!(
            llvm_record.source_digest,
            Digest([
                0xe3, 0x42, 0x7c, 0xe8, 0x1f, 0x98, 0xf9, 0xa0, 0x4d, 0xb3, 0xe0, 0x4c, 0xe8, 0x22,
                0xc3, 0xe0, 0xf1, 0xc7, 0x31, 0x13, 0x7f, 0x0d, 0x63, 0xe0, 0x44, 0x0f, 0x86, 0x2b,
                0x1e, 0x61, 0xc9, 0x3e,
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
            coverage.domain == Domain::Return
                && matches!(coverage.coverage, Coverage::Closed)
                && coverage.evidence.contains(&source_id)
                && coverage.evidence.contains(&layout_id)
                && coverage.evidence.contains(&parser_id)
                && coverage.evidence.contains(&inventory_id)
                && coverage.evidence.contains(&native_demand_id)
                && coverage.evidence.contains(&llvm_record.id().unwrap())
        }));
        assert!(selection.variants[0].graph.coverage.iter().any(|coverage| {
            coverage.domain == Domain::Diagnostic
                && matches!(coverage.coverage, Coverage::Closed)
                && coverage.evidence.contains(&parser_id)
                && coverage.evidence.contains(&diagnostic_id)
        }));
        assert!(selection.variants[0].graph.coverage.iter().any(|coverage| {
            coverage.domain == Domain::Invocation
                && matches!(coverage.coverage, Coverage::Open { .. })
                && coverage.evidence.contains(&parser_id)
                && coverage.evidence.contains(&cast_id)
                && coverage.evidence.contains(&callback_id)
        }));
        assert!(selection.variants[0].graph.blocks.iter().any(|block| {
            block.events.iter().any(|event| {
                matches!(
                    event,
                    Event::Invoke {
                        callable: Operand::Literal(method),
                        arguments,
                        ..
                    } if method.0 == b"__toString"
                        && arguments == &[Operand::Argument(0)]
                )
            })
        }));
        assert!(
            selection.variants[0]
                .graph
                .coverage
                .iter()
                .filter(|coverage| {
                    coverage.domain != Domain::Return && coverage.domain != Domain::Diagnostic
                })
                .all(|coverage| {
                    matches!(coverage.coverage, Coverage::Open { .. })
                        && coverage.evidence.contains(&source_id)
                        && coverage.evidence.contains(&parser_id)
                        && coverage.evidence.contains(&llvm_record.id().unwrap())
                })
        );
        let native_unwind = selection.variants[0]
            .graph
            .coverage
            .iter()
            .find(|coverage| coverage.domain == Domain::NativeUnwind)
            .expect("schema records native unwind coverage independently");
        assert!(matches!(native_unwind.coverage, Coverage::Open { .. }));
        assert!(selection.variants[0].graph.blocks.iter().any(|block| {
            block.events.iter().any(|event| {
                matches!(event,
                Event::Unknown { domains, reason }
                    if domains.contains(&Domain::NativeUnwind)
                        && reason.0.windows(b"parser failure exits".len())
                            .any(|window| window == b"parser failure exits"))
            })
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
