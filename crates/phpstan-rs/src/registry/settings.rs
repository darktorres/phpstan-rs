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
    /// finds them, and not every file of the autoloader's directories. On
    /// unless set to `false`. `"declarations"` follows a vendor file's
    /// names only where its declarations hold them (signatures, hierarchy,
    /// PHPDoc, constant values), not those in its function and method
    /// bodies: fewer classes load, and a call in the body of a vendor
    /// function to a class that does not load is one the passes over the
    /// bodies (whose properties a function may write) cannot follow, which
    /// they read as writing any.
    pub lazy_vendor_context: Option<LazyVendorContext>,
}

/// `ReflectionSettings::lazy_vendor_context`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum LazyVendorContext {
    Enabled(bool),
    Mode(LazyVendorMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LazyVendorMode {
    Declarations,
}

impl config::Section for ReflectionSettings {
    const PATH: &'static [&'static str] = &[];
}
