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
    let scenarios = known
        .iter()
        .copied()
        .filter(|version| min <= *version && *version <= max)
        .map(|version| TargetScenario {
            release: VersionRange::minor(version.major as u16, version.minor as u16),
            os: None,
            arch: None,
            sapi: None,
            zts: None,
            debug: None,
            extensions: None,
            ini: None,
        })
        .collect();
    TargetSet::new(scenarios).expect("known PHP versions form valid minor scenarios")
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
}
