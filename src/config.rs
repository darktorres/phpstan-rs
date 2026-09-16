//! A minimal, optional standing TOML config file
//! (paths/level/baseline/strict-mixed-arrays), read by
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
//! check_duplicate_function_declarations = false
//! check_duplicate_class_declarations = false
//! check_boolean_trap = false
//! check_array_key_cast = false
//! check_parent_constructor_call = false
//! check_promoted_property_suggestions = false
//! check_boolean_conditions = false
//! check_dynamic_properties = false
//! check_swapped_arguments = false
//! check_never_return_suggestions = false
//! check_switch_missing_default = false
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
    /// Mirrors real PHPStan's own opt-in `checkUninitializedProperties`
    /// - see `registry::Registry::check_uninitialized_properties`.
    pub check_uninitialized_properties: Option<bool>,
    /// Mirrors real PHPStan's own opt-in `featureToggles.internalTag` -
    /// see `registry::Registry::check_internal_usage`.
    pub check_internal_usage: Option<bool>,
    /// Mirrors real PHPStan's own opt-in `checkFunctionNameCase` - see
    /// `registry::Registry::check_function_name_case`.
    pub check_function_name_case: Option<bool>,
    /// Mirrors real PHPStan's own separate, opt-in
    /// `phpstan/phpstan-deprecation-rules` package - see
    /// `registry::Registry::check_deprecated_usage`.
    pub check_deprecated_usage: Option<bool>,
    /// Extra directories loaded for type-resolution context only - never
    /// analyzed or reported on. See `composer::discover_vendor_context`
    /// for the automatic `vendor/` counterpart to this.
    pub scan_paths: Option<Vec<PathBuf>>,
    /// Disables the automatic Composer-autoloader-based `vendor/`
    /// context load (see `composer::discover_vendor_context`).
    pub no_composer: Option<bool>,
    /// The project's target PHP version, as a `"major.minor"` string
    /// (see `php_version::parse_php_version`) - a plain `String` since
    /// TOML has no tuple literal and that's what a user actually types;
    /// parsed once at the CLI/config merge point in `main.rs`.
    pub php_version: Option<String>,
    /// The genuinely-empty-concrete-method-body completeness check
    /// (`return.missing`/`return.never`), opt-in - see
    /// `registry::Registry::check_empty_method_bodies`.
    pub check_empty_method_bodies: Option<bool>,
    /// Opt-in "missing checked exception" family - see
    /// `registry::Registry::check_missing_checked_exceptions`.
    pub check_missing_checked_exceptions: Option<bool>,
    /// Opt-in strict (vs. the default, castable) printf
    /// placeholder-type matching mode - see
    /// `registry::Registry::check_strict_printf_placeholder_types`.
    pub check_strict_printf_placeholder_types: Option<bool>,
    /// Mirrors real PHPStan's own opt-in
    /// `checkTooWideThrowTypesInProtectedAndPublicMethods` - see
    /// `registry::Registry::check_too_wide_throw_type_in_protected_and_public_methods`.
    pub check_too_wide_throw_type_in_protected_and_public_methods: Option<bool>,
    /// The return-type analogue - see
    /// `registry::Registry::check_too_wide_return_type_in_protected_and_public_methods`.
    pub check_too_wide_return_type_in_protected_and_public_methods: Option<bool>,
    /// Opt-in `MethodThrowTypeCovarianceRule` - see
    /// `registry::Registry::check_method_throw_type_covariance`.
    pub check_method_throw_type_covariance: Option<bool>,
    /// A hand-mirrored subset of real PHPStan's own `ignoreErrors:` -
    /// this project has no NEON parser (see `baseline.rs`), so a
    /// project's real `phpstan.neon` config is manually re-typed here.
    /// See `ignore_errors::IgnoreErrorEntry` for field semantics.
    pub ignore_errors: Option<Vec<crate::ignore_errors::IgnoreErrorEntry>>,
    /// A declarative, config-driven ban list - see
    /// `restricted_symbols::RestrictedSymbolEntry`. Config-file-only, no
    /// CLI flag - a structured list doesn't fit a boolean/string one.
    ///
    /// ```toml
    /// [[restricted_symbols]]
    /// kind = "staticMethod"
    /// class = "Legacy\\Db"
    /// name = "query"
    /// message = "use the Db\\Connection service instead"
    /// ```
    pub restricted_symbols: Option<Vec<crate::restricted_symbols::RestrictedSymbolEntry>>,
    /// Extra directories/files whose class-like declarations are
    /// designated overrides (e.g. a vendored `Imagick.stub.php`) rather
    /// than a genuine accidental duplicate - mirrors `scan_paths`'s
    /// shape, unioned into it at load time. See
    /// `project::duplicate_class_sites::ClassDeclSite::is_stub`.
    pub stub_paths: Option<Vec<PathBuf>>,
    /// Opt-in whole-project duplicate function-declaration detection -
    /// see `registry::Registry::check_duplicate_function_declarations`'s
    /// own doc comment for why.
    pub check_duplicate_function_declarations: Option<bool>,
    /// Opt-in whole-project duplicate class-like-declaration detection.
    /// See `registry::Registry::check_duplicate_class_declarations` for
    /// why this needs `stub_paths` correctly populated first.
    pub check_duplicate_class_declarations: Option<bool>,
    /// Opt-in "boolean trap" detection - see
    /// `registry::Registry::check_boolean_trap`'s own doc comment for why.
    pub check_boolean_trap: Option<bool>,
    /// Opt-in array-key cast warnings - see
    /// `registry::Registry::check_array_key_cast`'s own doc comment for
    /// why.
    pub check_array_key_cast: Option<bool>,
    /// Opt-in `RequireParentConstructCallRule` - see
    /// `registry::Registry::check_parent_constructor_call`'s own doc
    /// comment for why.
    pub check_parent_constructor_call: Option<bool>,
    /// Opt-in constructor-property-promotion suggestions - see
    /// `registry::Registry::check_promoted_property_suggestions`'s own
    /// doc comment for why.
    pub check_promoted_property_suggestions: Option<bool>,
    /// Opt-in "only booleans in a condition" family - see
    /// `registry::Registry::check_boolean_conditions`'s own doc comment
    /// for why.
    pub check_boolean_conditions: Option<bool>,
    /// Opt-in strict `checkDynamicProperties` behavior inside
    /// `isset()`/`empty()`/`??`/`??=` - see `registry::Registry::
    /// check_dynamic_properties`'s own doc comment for why.
    pub check_dynamic_properties: Option<bool>,
    /// Opt-in swapped-adjacent-argument-name detection - see
    /// `registry::Registry::check_swapped_arguments`'s own doc comment
    /// for why.
    pub check_swapped_arguments: Option<bool>,
    /// Opt-in `never`-return-type suggestion - see
    /// `registry::Registry::check_never_return_suggestions`'s own doc
    /// comment for why.
    pub check_never_return_suggestions: Option<bool>,
    /// Opt-in missing-`default:`-arm detection - see
    /// `registry::Registry::check_switch_missing_default`'s own doc
    /// comment for why.
    pub check_switch_missing_default: Option<bool>,
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
