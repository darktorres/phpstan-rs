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
    /// Directories loaded for type resolution only, never analysed.
    pub scan_paths: Vec<PathBuf>,
    /// Directories or files whose class-like declarations are designated
    /// overrides (a vendored `Imagick.stub.php`) rather than accidental
    /// duplicates; loaded for context like `scan_paths`.
    pub stub_paths: Vec<PathBuf>,
    /// Turns off loading `vendor/` through Composer's autoloader.
    pub no_composer: Option<bool>,
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
    /// The versions it names.
    pub fn versions(&self) -> Result<types::PhpVersions, String> {
        match self {
            PhpVersionSetting::Exactly(version) => Ok(types::PhpVersions::exactly(
                types::parse_php_version(version)?,
            )),
            PhpVersionSetting::Range { min, max } => {
                let (low, high) = (
                    types::parse_php_version(min)?,
                    types::parse_php_version(max)?,
                );
                if low > high {
                    return Err(format!("php_version: min {min} is above max {max}"));
                }
                Ok(types::PhpVersions::new(low, high))
            }
        }
    }
}
