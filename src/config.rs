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
//! check_throw_without_message = false
//! check_throw_base_class = false
//! check_exception_hygiene = false
//! check_useless_casts = false
//! check_strict_array_filter = false
//! check_mixed_type_operands = false
//! check_comparable_types = false
//! check_arithmetic_operands = false
//! check_identity_vs_equals = false
//! check_loose_haystack_comparison = false
//! check_strict_function_calls = false
//! check_switch_case_types = false
//! check_loop_variable_overwrite = false
//! check_dynamic_static_calls = false
//! check_illegal_constructor_calls = false
//! check_enum_exhaustiveness = false
//! check_null_coercion = false
//! check_new_exception_discarded = false
//! check_risky_tests = false
//! check_dead_enum_cases = false
//! check_dead_code = false
//! check_circular_dependencies = false
//! check_dead_abstract_classes = false
//! check_dead_members = false
//! check_renamed_named_arguments = false
//! report_unmatched_ignored_errors = false
//! check_composer_hygiene = false
//! check_closed_classes = false
//! check_mock_hygiene = false
//! default_parent_calls = true
//! no_composer = false
//! php_version = "8.1"
//!
//! [[ignore_errors]]
//! identifier = "missingType.iterableValue"
//! path = "_analysis/phpstan-latte/*"
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `Config::dynamic_constant_names`'s two accepted shapes.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DynamicConstantNames {
    Names(Vec<String>),
    Typed(std::collections::BTreeMap<String, String>),
}

impl DynamicConstantNames {
    /// Every entry as `(name, explicit type)`.
    pub fn entries(&self) -> Vec<(String, Option<String>)> {
        match self {
            DynamicConstantNames::Names(names) => names.iter().map(|n| (n.clone(), None)).collect(),
            DynamicConstantNames::Typed(map) => map
                .iter()
                .map(|(n, t)| (n.clone(), Some(t.clone())))
                .collect(),
        }
    }
}

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Config {
    pub paths: Option<Vec<PathBuf>>,
    pub level: Option<u8>,
    pub baseline: Option<PathBuf>,
    /// Match baseline entries by identifier when their message text no
    /// longer matches (`baseline::MatchMode::Identifier`).
    pub baseline_match_identifier: Option<bool>,
    /// Write `--generate-baseline` as one file per identifier plus an
    /// index (`baseline::generate_per_identifier`).
    pub baseline_per_identifier: Option<bool>,
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
    /// The by-ref-parameter analogue - see
    /// `registry::Registry::check_too_wide_by_ref_type_in_protected_and_public_methods`.
    pub check_too_wide_by_ref_type_in_protected_and_public_methods: Option<bool>,
    /// Opt-in `MethodThrowTypeCovarianceRule` - see
    /// `registry::Registry::check_method_throw_type_covariance`.
    pub check_method_throw_type_covariance: Option<bool>,
    /// Mirrors real PHPStan's own standalone `checkBenevolentUnionTypes`
    /// parameter - see `registry::Registry::check_benevolent_union_types`.
    pub check_benevolent_union_types: Option<bool>,
    /// Mirrors real PHPStan's own `checkMissingCallableSignature`
    /// parameter - see `registry::Registry::check_missing_callable_signature`.
    pub check_missing_callable_signature: Option<bool>,
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
    /// `[[architecture]]` rules - see `architecture`'s own doc comment.
    pub architecture: Option<Vec<crate::architecture::ArchitectureRule>>,
    /// Language constructs to report wherever they're used - see
    /// `banned::BannedConstruct::ALL` for the names.
    pub banned_constructs: Option<Vec<String>>,
    /// Native types (`float`, ...) or class names to report in
    /// signatures, properties and constants.
    pub banned_types: Option<Vec<String>>,
    /// Built-in `restricted_symbols` sets to add (`multibyte-strings`).
    pub restricted_symbol_presets: Option<Vec<String>>,
    /// Opt-in "closed by default" class shape - see
    /// `project::class_shape`'s own doc comment.
    pub check_closed_classes: Option<bool>,
    /// `[[parent_calls]]` requirements - see `parent_calls`.
    pub parent_calls: Option<Vec<crate::parent_calls::ParentCallRule>>,
    /// Whether the built-in PHPUnit/Codeception hook requirements
    /// (`parent_calls::default_rules`) apply; on unless set to false.
    pub default_parent_calls: Option<bool>,
    /// `[[assertions]]` - see `assertion_tables`.
    pub assertions: Option<Vec<crate::assertion_tables::AssertionEntry>>,
    /// `[[class_argument_returns]]` - see `assertion_tables`.
    pub class_argument_returns: Option<Vec<crate::assertion_tables::ClassArgumentReturn>>,
    /// Opt-in PHPUnit mock-builder misuse checks - see
    /// `project::mock_hygiene`.
    pub check_mock_hygiene: Option<bool>,
    /// Classes tests must not mock (`mock.bannedClass`).
    pub mock_banned_classes: Option<Vec<String>>,
    /// Mirrors real PHPStan's own `dynamicConstantNames`: global
    /// (`NAME`) or class (`Class::NAME`) constants whose declared value
    /// differs between environments, so their literal value is never
    /// trusted. A plain list reads each one as its value's general type;
    /// a table gives the type explicitly, and a declared value it doesn't
    /// accept is reported. Config-file-only.
    ///
    /// ```toml
    /// dynamic_constant_names = ["DEBUG", "App\\Settings::LEVEL"]
    /// # or
    /// [dynamic_constant_names]
    /// DEBUG = "bool"
    /// ```
    pub dynamic_constant_names: Option<DynamicConstantNames>,
    /// A project's own custom DQL function registrations - see
    /// `doctrine_functions::DoctrineCustomFunctionEntry`. Config-file-only,
    /// no CLI flag, the same reasoning as `restricted_symbols`.
    ///
    /// ```toml
    /// [[doctrine_custom_functions]]
    /// name = "DATE_FORMAT_YEAR_MONTH"
    /// kind = "string"
    /// ```
    pub doctrine_custom_functions:
        Option<Vec<crate::doctrine_functions::DoctrineCustomFunctionEntry>>,
    /// A project's own live-SQL-reflection connection/schema-source
    /// config - see `sql_reflection_config::SqlReflectionConfig`.
    /// Config-file-only, no CLI flag, the same reasoning as
    /// `restricted_symbols`. Absent: no connection is ever attempted and
    /// every raw-SQL call site keeps today's fallback, unchanged.
    ///
    /// ```toml
    /// [sql_reflection]
    /// driver = "mysql"
    /// host = "127.0.0.1"
    /// password_env = "PIWIGO_DB_PASSWORD"
    /// database = "piwigo_test"
    /// ```
    pub sql_reflection: Option<crate::sql_reflection_config::SqlReflectionConfig>,
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
    /// Opt-in missing-exception-message detection - see
    /// `registry::Registry::check_throw_without_message`'s own doc
    /// comment for why.
    pub check_throw_without_message: Option<bool>,
    /// Opt-in base-Exception/Error-class detection - see
    /// `registry::Registry::check_throw_base_class`'s own doc comment
    /// for why.
    pub check_throw_base_class: Option<bool>,
    /// Opt-in: a `catch` must chain the caught exception into one it
    /// throws, and a catch of a base exception type must rethrow.
    pub check_exception_hygiene: Option<bool>,
    /// Opt-in (phpstan-strict-rules): a cast to a type the expression already has (`cast.useless`).
    pub check_useless_casts: Option<bool>,
    /// Opt-in (phpstan-strict-rules): `array_filter()` without a callback over values whose truthiness is ambiguous, or with a null callback (`arrayFilter.strict`).
    pub check_strict_array_filter: Option<bool>,
    /// Opt-in (voku/phpstan-rules): a string operand combined with a non-string one, or an array with a non-array, by a binary operator or compound assignment (`binaryOp.nonStringOperand`/`binaryOp.nonArrayOperand`/`assignOp.*`).
    pub check_mixed_type_operands: Option<bool>,
    /// Opt-in (shipmonk/phpstan-rules): `<`/`>`/`<=`/`>=`/`<=>` only between int|float, string, DateTimeInterface, or tuples of those (`comparison.nonComparable`/`comparison.differentTypes`).
    pub check_comparable_types: Option<bool>,
    /// Opt-in (phpstan-strict-rules): arithmetic, unary `+`/`-` and increment/decrement operands that are not numeric (`plus.leftNonNumeric`, `unaryMinus.nonNumeric`, `preInc.nonNumeric`, ...).
    pub check_arithmetic_operands: Option<bool>,
    /// Opt-in: `===`/`!==` between objects whose class defines an `equals()` method accepting the other side (`identical.equalsMethod`).
    pub check_identity_vs_equals: Option<bool>,
    /// Opt-in: `in_array()`/`array_search()`/`array_keys()` compared loosely where the needle and haystack value types can be `==` without being `===` (`function.looseComparison`).
    pub check_loose_haystack_comparison: Option<bool>,
    /// Opt-in (phpstan-strict-rules): `in_array()`/`array_search()`/`array_keys()`/`base64_decode()` without their strict argument set to `true`, and `iterator_to_array()` without `$preserve_keys` (`function.strict`).
    pub check_strict_function_calls: Option<bool>,
    /// Opt-in (phpstan-strict-rules): a `switch` case whose type can never match the subject's (`switch.type`).
    pub check_switch_case_types: Option<bool>,
    /// Opt-in (phpstan-strict-rules): a `foreach` key/value variable or `for` initial assignment overwriting an already-defined variable (`foreach.keyOverwrite`/`foreach.valueOverwrite`/`for.variableOverwrite`).
    pub check_loop_variable_overwrite: Option<bool>,
    /// Opt-in (phpstan-strict-rules): a static method called through an instance, `$obj->staticMethod()` (`staticMethod.dynamicCall`).
    pub check_dynamic_static_calls: Option<bool>,
    /// Opt-in (phpstan-strict-rules): `__construct()` called on an existing object, or statically outside a parent-constructor call (`constructor.call`).
    pub check_illegal_constructor_calls: Option<bool>,
    /// Opt-in: enum branching that defeats match exhaustiveness (a default arm, switch, or if-chains over enum cases).
    pub check_enum_exhaustiveness: Option<bool>,
    /// Opt-in: isset()/empty() on objects and possibly-null values silently coerced by strings, operators and compound assignments.
    pub check_null_coercion: Option<bool>,
    /// Opt-in discarded-`new`-exception detection - see
    /// `registry::Registry::check_new_exception_discarded`'s own doc
    /// comment for why.
    pub check_new_exception_discarded: Option<bool>,
    /// Opt-in assertion-less-test ("risky test") detection - see
    /// `registry::Registry::check_risky_tests`'s own doc comment for
    /// why.
    pub check_risky_tests: Option<bool>,
    /// Opt-in dead-enum-case detection - see
    /// `registry::Registry::check_dead_enum_cases`'s own doc comment for
    /// why.
    pub check_dead_enum_cases: Option<bool>,
    /// Opt-in dead-function/dead-class detection - see
    /// `registry::Registry::check_dead_code`'s own doc comment for why.
    pub check_dead_code: Option<bool>,
    /// Opt-in namespace-level circular-dependency detection - see
    /// `registry::Registry::check_circular_dependencies`'s own doc
    /// comment for why.
    pub check_circular_dependencies: Option<bool>,
    /// Opt-in dead-abstract-class detection - see `registry::Registry::
    /// check_dead_abstract_classes`'s own doc comment for why.
    pub check_dead_abstract_classes: Option<bool>,
    /// Opt-in dead-protected/public-member detection - see `registry::
    /// Registry::check_dead_members`'s own doc comment for why.
    pub check_dead_members: Option<bool>,
    /// Opt-in named-argument-rename-collision detection - see
    /// `registry::Registry::check_renamed_named_arguments`'s own doc
    /// comment for why.
    pub check_renamed_named_arguments: Option<bool>,
    /// Opt-in reporting of inline `@phpstan-ignore*` comments that
    /// suppress nothing (`ignore.unmatchedLine`/
    /// `ignore.unmatchedIdentifier`). Off by default: an ignore naming a
    /// rule this project doesn't implement would always be unmatched.
    pub report_unmatched_ignored_errors: Option<bool>,
    /// Opt-in checks of the analysed code against the root
    /// `composer.json` (declared, unused and misplaced dependencies,
    /// PSR-4 locations, required extensions, `provide`, `scripts`,
    /// `config.platform.php`). Off by default: dependency usage through
    /// framework configuration the analysis can't see reads as unused.
    pub check_composer_hygiene: Option<bool>,
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
