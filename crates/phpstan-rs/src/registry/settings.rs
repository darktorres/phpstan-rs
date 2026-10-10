//! The reflection's keys: which PHP versions the project runs on, and
//! what's loaded beside the analysed files for context.

use std::path::PathBuf;

use serde::Deserialize;

/// Top-level keys.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReflectionSettings {
    /// The PHP versions the project runs on.
    pub php_version: Option<PhpVersionSetting>,
    /// Runtime facts required by profile-bound semantic catalog entries.
    /// Unspecified facts remain unknown and prevent those entries from
    /// applying.
    pub php_target_profile: Option<PhpTargetProfileSetting>,
    /// Directories loaded for type resolution only, never analysed.
    pub scan_paths: Vec<PathBuf>,
    /// Directories or files whose class-like declarations are designated
    /// overrides (a vendored `Imagick.stub.php`) rather than accidental
    /// duplicates; loaded for context like `scan_paths`.
    pub stub_paths: Vec<PathBuf>,
    /// Turns off loading `vendor/` through Composer's autoloader.
    pub no_composer: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct PhpTargetProfileSetting {
    pub os: Option<String>,
    pub arch: Option<String>,
    pub sapi: Option<String>,
    pub zts: Option<bool>,
    pub debug: Option<bool>,
}

impl Default for PhpTargetProfileSetting {
    fn default() -> Self {
        Self {
            os: None,
            arch: None,
            sapi: None,
            zts: None,
            debug: None,
        }
    }
}

impl PhpTargetProfileSetting {
    pub fn target_profile(&self) -> Result<php_semantics::TargetProfile, String> {
        fn normalized(
            value: &Option<String>,
            field: &str,
        ) -> Result<Option<php_semantics::Bytes>, String> {
            value.as_ref().map_or(Ok(None), |value| {
                if value.trim().is_empty() {
                    return Err(format!("php_target_profile.{field} must not be empty"));
                }
                Ok(Some(php_semantics::Bytes(
                    value.trim().to_ascii_lowercase().into_bytes(),
                )))
            })
        }
        Ok(php_semantics::TargetProfile {
            os: normalized(&self.os, "os")?,
            arch: normalized(&self.arch, "arch")?,
            sapi: normalized(&self.sapi, "sapi")?,
            zts: self.zts,
            debug: self.debug,
            extensions: None,
            ini: None,
        })
    }
}

impl config::Section for ReflectionSettings {
    const PATH: &'static [&'static str] = &[];
}

/// The PHP versions the project runs on, as the config writes them: one
/// version, `"8.1"`, or every version from `min` to `max`,
/// `{ min = "7.4", max = "8.4" }`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum PhpVersionSetting {
    Exactly(String),
    Range { min: String, max: String },
}

impl PhpVersionSetting {
    /// Patch-aware target interval for catalog applicability. The existing
    /// `PhpVersions` projection below remains the language-feature view.
    pub fn releases(&self) -> Result<types::PhpReleaseRange, String> {
        match self {
            PhpVersionSetting::Exactly(version) => types::parse_php_release_range(version),
            PhpVersionSetting::Range { min, max } => {
                let low = types::parse_php_release_range(min)?.min;
                let high = types::parse_php_release_range(max)?.max;
                if low > high {
                    return Err(format!("php_version: min {min} is above max {max}"));
                }
                Ok(types::PhpReleaseRange::new(low, high))
            }
        }
    }

    /// The versions it names.
    pub fn versions(&self) -> Result<types::PhpVersions, String> {
        self.releases()?.language_versions().ok_or_else(|| {
            "php_version: release range cannot be projected to language versions".to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_patch_release_is_preserved_alongside_minor_feature_projection() {
        let setting = PhpVersionSetting::Exactly("8.5.4".into());
        assert_eq!(
            setting.releases().unwrap(),
            types::PhpReleaseRange::new(
                types::PhpRelease::new(8, 5, 4),
                types::PhpRelease::new(8, 5, 4)
            )
        );
        assert_eq!(
            setting.versions().unwrap(),
            types::PhpVersions::exactly(types::PhpVersion::new(8, 5))
        );
    }

    #[test]
    fn minor_range_expands_patch_endpoints_without_dropping_the_series() {
        let setting = PhpVersionSetting::Range {
            min: "8.4".into(),
            max: "8.5".into(),
        };
        assert_eq!(
            setting.releases().unwrap(),
            types::PhpReleaseRange::new(
                types::PhpRelease::new(8, 4, 0),
                types::PhpRelease::new(8, 5, u16::MAX)
            )
        );
    }

    #[test]
    fn target_profile_configuration_is_normalized_and_keeps_unknown_fields_unknown() {
        let settings: ReflectionSettings = toml::from_str(
            "php_target_profile = { os = \"Linux\", arch = \"X86_64\", sapi = \"CLI\", zts = false, debug = false }\n",
        )
        .unwrap();
        let profile = settings
            .php_target_profile
            .unwrap()
            .target_profile()
            .unwrap();
        assert_eq!(profile.os, Some(php_semantics::Bytes(b"linux".to_vec())));
        assert_eq!(profile.arch, Some(php_semantics::Bytes(b"x86_64".to_vec())));
        assert_eq!(profile.sapi, Some(php_semantics::Bytes(b"cli".to_vec())));
        assert_eq!(profile.zts, Some(false));
        assert_eq!(profile.debug, Some(false));
        assert_eq!(profile.extensions, None);
        assert_eq!(profile.ini, None);
    }

    #[test]
    fn empty_target_profile_facts_are_rejected() {
        let setting = PhpTargetProfileSetting {
            os: Some("  ".into()),
            ..PhpTargetProfileSetting::default()
        };
        assert!(setting.target_profile().is_err());
    }
}
