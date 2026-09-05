//! Phase 11 Stage 6's TOML config file - a minimal, optional standing
//! project setting (paths/level/baseline/strict-mixed-arrays), read by
//! `check-project`'s `--config` flag. Every field is optional and CLI
//! flags always take precedence over the matching config value (see
//! `main.rs::run_check_project`'s own resolution logic) - a config file
//! only fills in what the command line didn't already say.
//!
//! Example:
//!
//! ```toml
//! paths = ["src", "tests"]
//! level = 5
//! baseline = "phpstan-baseline.json"
//! strict_mixed_arrays = false
//! check_uninitialized_properties = false
//! check_internal_usage = false
//! check_deprecated_usage = false
//! scan_paths = ["vendor/some-manual-extra"]
//! no_composer = false
//! php_version = "8.1"
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Config {
    pub paths: Option<Vec<PathBuf>>,
    pub level: Option<u8>,
    pub baseline: Option<PathBuf>,
    pub strict_mixed_arrays: Option<bool>,
    /// Phase 62: mirrors real PHPStan's own opt-in `checkUninitializedProperties`
    /// parameter - see `registry::Registry::check_uninitialized_properties`'s
    /// own doc comment for why this defaults to `false` (unset) rather
    /// than running unconditionally like this project's other checks.
    pub check_uninitialized_properties: Option<bool>,
    /// Phase 67: mirrors real PHPStan's own opt-in `featureToggles.
    /// internalTag` parameter - see `registry::Registry::
    /// check_internal_usage`'s own doc comment for why this defaults to
    /// `false` (unset) too.
    pub check_internal_usage: Option<bool>,
    /// Phase 39: mirrors real PHPStan's own separate, opt-in
    /// `phpstan/phpstan-deprecation-rules` package - see
    /// `registry::Registry::check_deprecated_usage`'s own doc comment
    /// for why this defaults to `false` (unset) too.
    pub check_deprecated_usage: Option<bool>,
    /// Extra directories loaded for type-resolution context only - never
    /// analyzed or reported on. See `composer::discover_vendor_context`
    /// for the automatic `vendor/` counterpart to this.
    pub scan_paths: Option<Vec<PathBuf>>,
    /// Disables the automatic Composer-autoloader-based `vendor/`
    /// context load (see `composer::discover_vendor_context`).
    pub no_composer: Option<bool>,
    /// Phase 38: the project's target PHP version, as a `"major.minor"`
    /// string (e.g. `"8.1"`) - see `php_version::parse_php_version` for
    /// the exact accepted format. A plain `Option<String>` here rather
    /// than a structured `(u8, u8)`, since TOML has no tuple literal and
    /// `"8.1"` is what a user actually types; parsing (and erroring on a
    /// malformed value) happens once, at the CLI/config merge point in
    /// `main.rs`, the same place `baseline`/`scan_paths` are resolved
    /// against the config file's own directory.
    pub php_version: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests;
