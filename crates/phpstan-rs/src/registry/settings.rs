//! The reflection's keys: which PHP versions the project runs on, and
//! what's loaded beside the analysed files for context.

use std::path::PathBuf;

use serde::Deserialize;

/// Top-level keys.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReflectionSettings {
    /// The PHP version, as `"major.minor"`.
    pub php_version: Option<String>,
    /// Directories loaded for type resolution only, never analysed.
    pub scan_paths: Vec<PathBuf>,
    /// Directories or files whose class-like declarations are designated
    /// overrides (a vendored `Imagick.stub.php`) rather than accidental
    /// duplicates; loaded for context like `scan_paths`.
    pub stub_paths: Vec<PathBuf>,
    /// Turns off loading `vendor/` through Composer's autoloader.
    pub no_composer: Option<bool>,
    /// Loads the classes of `vendor/` by the names the analysed code and
    /// the vendor code it reaches mention, found as Composer's autoloader
    /// finds them, and not every file of the autoloader's directories.
    /// On unless set to `false`.
    pub lazy_vendor_context: Option<bool>,
}

impl config::Section for ReflectionSettings {
    const PATH: &'static [&'static str] = &[];
}
