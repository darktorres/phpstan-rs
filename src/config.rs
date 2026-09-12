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
//! check_missing_checked_exceptions = false
//! scan_paths = ["vendor/some-manual-extra"]
//! stub_paths = ["phpstan-stubs/Imagick.stub.php"]
//! check_duplicate_class_declarations = false
//! no_composer = false
//! php_version = "8.1"
//!
//! [[ignore_errors]]
//! identifier = "missingType.iterableValue"
//! path = "_analysis/phpstan-latte/*"
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
    /// ROADMAP "Known but not yet scheduled": mirrors real PHPStan's own
    /// opt-in `checkFunctionNameCase` parameter - see `registry::
    /// Registry::check_function_name_case`'s own doc comment for why
    /// this defaults to `false` (unset) too.
    pub check_function_name_case: Option<bool>,
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
    /// Phase 61: reinstates the genuinely-empty-concrete-method-body
    /// completeness check (`return.missing`/`return.never`) as opt-in -
    /// see `registry::Registry::check_empty_method_bodies`'s own doc
    /// comment for why this defaults to `false` (unset), the same reason
    /// `check_uninitialized_properties`/`check_internal_usage`/
    /// `check_deprecated_usage` above all do.
    pub check_empty_method_bodies: Option<bool>,
    /// Phase 42 bonus item: opt-in "missing checked exception" family -
    /// see `registry::Registry::check_missing_checked_exceptions`'s own
    /// doc comment for why this defaults to `false` (unset), the same
    /// reason every other opt-in check above does.
    pub check_missing_checked_exceptions: Option<bool>,
    /// Phase 69: opt-in strict (vs. the default, castable) printf
    /// placeholder-type matching mode - see `registry::Registry::
    /// check_strict_printf_placeholder_types`'s own doc comment for why
    /// this defaults to `false` (unset), the same reason every other
    /// opt-in check above does.
    pub check_strict_printf_placeholder_types: Option<bool>,
    /// Phase 42 gating fix: mirrors real PHPStan's own opt-in
    /// `checkTooWideThrowTypesInProtectedAndPublicMethods` parameter - see
    /// `registry::Registry::check_too_wide_throw_type_in_protected_and_public_methods`'s
    /// own doc comment for why this defaults to `false` (unset), the same
    /// reason every other opt-in check above does.
    pub check_too_wide_throw_type_in_protected_and_public_methods: Option<bool>,
    /// Phase 45 (item 11, 45A): the return-type analogue - see
    /// `registry::Registry::check_too_wide_return_type_in_protected_and_public_methods`'s
    /// own doc comment for why this defaults to `false` (unset).
    pub check_too_wide_return_type_in_protected_and_public_methods: Option<bool>,
    /// Phase 42: opt-in `MethodThrowTypeCovarianceRule` - see
    /// `registry::Registry::check_method_throw_type_covariance`'s own
    /// doc comment for why this defaults to `false` (unset), the same
    /// reason every other opt-in check above does.
    pub check_method_throw_type_covariance: Option<bool>,
    /// A hand-mirrored subset of real PHPStan's own `ignoreErrors:` -
    /// this project has no NEON parser and one isn't otherwise justified
    /// (see `baseline.rs`'s own identical judgment for a related
    /// mechanism), so a project's real `phpstan.neon` config is manually
    /// re-typed here, the same way `paths`/`scan_paths` already are.
    /// See `ignore_errors::IgnoreErrorEntry`'s own doc comment for the
    /// exact field semantics.
    pub ignore_errors: Option<Vec<crate::ignore_errors::IgnoreErrorEntry>>,
    /// Phase 67: a declarative, config-driven ban list -
    /// `restricted_symbols::RestrictedSymbolEntry`'s own doc comment has
    /// the full shape. Config-file-only, no CLI flag - a structured list
    /// doesn't fit a boolean/string flag.
    ///
    /// ```toml
    /// [[restricted_symbols]]
    /// kind = "staticMethod"
    /// class = "Legacy\\Db"
    /// name = "query"
    /// message = "use the Db\\Connection service instead"
    /// ```
    pub restricted_symbols: Option<Vec<crate::restricted_symbols::RestrictedSymbolEntry>>,
    /// Phase 83: extra directories/files whose class-like declarations are
    /// designated overrides (e.g. a vendored `Imagick.stub.php`) rather
    /// than a genuine accidental duplicate - mirrors `scan_paths`'s own
    /// shape exactly (a directory, walked, or a single file listed
    /// directly), and is unioned into it at load time so a `stub_paths`
    /// entry still loads for type-resolution context exactly as before.
    /// The only consumer is `check_duplicate_class_declarations` below -
    /// see `project::duplicate_class_sites::ClassDeclSite`'s own
    /// `is_stub` field and `checker::duplicate_classes`'s own module doc
    /// comment for why this distinction exists at all.
    pub stub_paths: Option<Vec<PathBuf>>,
    /// Phase 83: opt-in whole-project duplicate class-like-declaration
    /// detection - see `registry::Registry::check_duplicate_class_
    /// declarations`'s own doc comment for why this defaults to `false`
    /// (unset), the same reason every other opt-in check above does, and
    /// for why this one specifically needs `stub_paths` correctly
    /// populated before it's safe to turn on for a real project.
    pub check_duplicate_class_declarations: Option<bool>,
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
