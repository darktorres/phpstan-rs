//! The LSP-mode equivalent of `main/check_project.rs::run_check_project_command`'s
//! own CLI+config merge, minus anything CLI-only (the LSP has no flags
//! of its own - a workspace's `phpstan-rs.toml`, auto-discovered at its
//! root, is the only source for everything here).

use std::path::{Path, PathBuf};

use crate::composer::ComposerContext;

/// Everything a workspace's `phpstan-rs.toml` (or its absence) resolves
/// into, applied once per `Server::configure_from_workspace` call.
/// `PartialEq` backs `Server::reload_configuration`'s no-op fast path -
/// comparing the real resolved value (not a `Debug`-formatted fingerprint)
/// so a change to a field a hand-written `Debug` impl deliberately
/// redacts (`SqlReflectionConfig`'s `password`) is never silently missed.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedWorkspaceConfig {
    pub(crate) configured_level: Option<u8>,
    pub(crate) strict_mixed_arrays: bool,
    pub(crate) check_uninitialized_properties: bool,
    pub(crate) check_internal_usage: bool,
    pub(crate) check_function_name_case: bool,
    pub(crate) check_deprecated_usage: bool,
    pub(crate) check_empty_method_bodies: bool,
    pub(crate) check_missing_checked_exceptions: bool,
    pub(crate) check_strict_printf_placeholder_types: bool,
    pub(crate) check_too_wide_throw_type_in_protected_and_public_methods: bool,
    pub(crate) check_too_wide_return_type_in_protected_and_public_methods: bool,
    pub(crate) check_too_wide_by_ref_type_in_protected_and_public_methods: bool,
    pub(crate) check_method_throw_type_covariance: bool,
    pub(crate) check_benevolent_union_types: bool,
    pub(crate) check_missing_callable_signature: bool,
    pub(crate) check_duplicate_function_declarations: bool,
    pub(crate) check_duplicate_class_declarations: bool,
    pub(crate) check_boolean_trap: bool,
    pub(crate) check_array_key_cast: bool,
    pub(crate) check_parent_constructor_call: bool,
    pub(crate) check_promoted_property_suggestions: bool,
    pub(crate) check_boolean_conditions: bool,
    pub(crate) check_dynamic_properties: bool,
    pub(crate) check_swapped_arguments: bool,
    pub(crate) check_never_return_suggestions: bool,
    pub(crate) check_switch_missing_default: bool,
    pub(crate) check_throw_without_message: bool,
    pub(crate) check_throw_base_class: bool,
    pub(crate) check_exception_hygiene: bool,
    pub(crate) check_identity_vs_equals: bool,
    pub(crate) check_loose_haystack_comparison: bool,
    pub(crate) check_strict_function_calls: bool,
    pub(crate) check_switch_case_types: bool,
    pub(crate) check_loop_variable_overwrite: bool,
    pub(crate) check_dynamic_static_calls: bool,
    pub(crate) check_illegal_constructor_calls: bool,
    pub(crate) check_enum_exhaustiveness: bool,
    pub(crate) check_null_coercion: bool,
    pub(crate) check_new_exception_discarded: bool,
    pub(crate) check_risky_tests: bool,
    pub(crate) php_version: Option<crate::php_version::PhpVersion>,
    pub(crate) assertion_entries: Vec<crate::assertion_tables::AssertionEntry>,
    pub(crate) class_argument_returns: Vec<crate::assertion_tables::ClassArgumentReturn>,
    pub(crate) restricted_symbol_entries: Vec<crate::restricted_symbols::RestrictedSymbolEntry>,
    pub(crate) dynamic_constant_names: Vec<(String, Option<String>)>,
    pub(crate) doctrine_custom_function_entries:
        Vec<crate::doctrine_functions::DoctrineCustomFunctionEntry>,
    pub(crate) sql_reflection_config: Option<crate::sql_reflection_config::SqlReflectionConfig>,
    pub(crate) sql_reflection_schema: Option<crate::sql_reflection_schema::SchemaCatalog>,
    pub(crate) phpstan_api_check_enabled: bool,
    pub(crate) vendor_context: ComposerContext,
    pub(crate) extra_context_dirs: Vec<PathBuf>,
    pub(crate) baseline: Option<crate::baseline::Baseline>,
    pub(crate) ignore_error_entries: Vec<crate::ignore_errors::IgnoreErrorEntry>,
    pub(crate) analyze_paths: Vec<PathBuf>,
}

/// The two kinds of paths `Server::load_workspace` needs beyond `root`
/// itself - loaded first, as context (`analyzed: false`), so `root`'s
/// own walk can dedupe against them by canonicalized path. See
/// `Server::load_workspace`'s own doc comment for why load order matters
/// beyond just the dedup. `analyze_paths` is the *analyzed* (`analyzed:
/// true`) scope's own restriction - empty means "no `paths` key
/// configured, walk all of `root`" (`Server::load_workspace_files`'s own
/// fallback), matching `run_check_project`'s identical "config.paths
/// empty -> analyze everything under the given dirs" default, not "empty
/// -> analyze nothing".
pub(crate) struct WorkspaceContextPaths {
    pub(crate) vendor: ComposerContext,
    pub(crate) extra_dirs: Vec<PathBuf>,
    pub(crate) analyze_paths: Vec<PathBuf>,
}

/// Resolves `root`'s own `phpstan-rs.toml` (or `config_path_override`,
/// for a future explicit-path setting) into everything
/// `Server::configure_from_workspace` needs to apply. Every failure mode
/// (a malformed config file, an invalid `php_version`, an unreadable
/// `sql_reflection.schema_file`) degrades to "as if unset" plus one
/// `eprintln!`, never `std::process::exit` - unlike `check-project`,
/// there is no process invocation here to fail out of, and a malformed
/// config must not kill the editor session.
pub(crate) fn resolve(root: &Path, config_path_override: Option<&Path>) -> ResolvedWorkspaceConfig {
    let config_path = config_path_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("phpstan-rs.toml"));
    let mut config = if config_path.is_file() {
        match crate::config::Config::load(&config_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("phpstan-rs lsp: {e}");
                crate::config::Config::default()
            }
        }
    } else {
        crate::config::Config::default()
    };

    let resolve_against_root = |p: PathBuf| root.join(p);

    let php_version = match config.php_version {
        Some(s) => match crate::php_version::parse_php_version(&s) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("phpstan-rs lsp: {e}");
                None
            }
        },
        None => None,
    };

    let sql_reflection_config = config.sql_reflection;
    let sql_reflection_schema = match sql_reflection_config
        .as_ref()
        .and_then(|c| c.schema_file.as_ref().map(|f| (f.clone(), c.driver)))
    {
        Some((path, driver)) => {
            let path = resolve_against_root(path);
            match crate::sql_reflection_schema::parse_ddl_file(&path, driver) {
                Ok(catalog) => Some(catalog),
                Err(e) => {
                    eprintln!("phpstan-rs lsp: {e}");
                    None
                }
            }
        }
        None => None,
    };

    let mut restricted_symbol_entries = config.restricted_symbols.take().unwrap_or_default();
    for preset in config.restricted_symbol_presets.take().unwrap_or_default() {
        match crate::banned::restricted_symbol_preset(&preset) {
            Ok(entries) => restricted_symbol_entries.extend(entries),
            Err(e) => eprintln!("phpstan-rs lsp: {e}"),
        }
    }

    let baseline = config.baseline.map(resolve_against_root).and_then(|path| {
        match crate::baseline::Baseline::load(&path) {
            Ok(b) => Some(b.with_match_mode(
                if config.baseline_match_identifier.unwrap_or(false) {
                    crate::baseline::MatchMode::Identifier
                } else {
                    crate::baseline::MatchMode::Message
                },
            )),
            Err(e) => {
                eprintln!(
                    "phpstan-rs lsp: failed to read baseline {}: {e}",
                    path.display()
                );
                None
            }
        }
    });

    let no_composer = config.no_composer.unwrap_or(false);
    let (vendor_context, phpstan_api_check_enabled) = if no_composer {
        (ComposerContext::default(), false)
    } else {
        (
            crate::composer::discover_vendor_context(root),
            crate::composer::detect_phpstan_dependency(root),
        )
    };

    let analyze_paths: Vec<PathBuf> = config
        .paths
        .unwrap_or_default()
        .into_iter()
        .map(resolve_against_root)
        .collect();

    let mut extra_context_dirs: Vec<PathBuf> = config
        .scan_paths
        .unwrap_or_default()
        .into_iter()
        .map(resolve_against_root)
        .collect();
    extra_context_dirs.extend(
        config
            .stub_paths
            .unwrap_or_default()
            .into_iter()
            .map(resolve_against_root),
    );

    ResolvedWorkspaceConfig {
        configured_level: config.level,
        strict_mixed_arrays: config.strict_mixed_arrays.unwrap_or(false),
        check_uninitialized_properties: config.check_uninitialized_properties.unwrap_or(false),
        check_internal_usage: config.check_internal_usage.unwrap_or(false),
        check_function_name_case: config.check_function_name_case.unwrap_or(false),
        check_deprecated_usage: config.check_deprecated_usage.unwrap_or(false),
        check_empty_method_bodies: config.check_empty_method_bodies.unwrap_or(false),
        check_missing_checked_exceptions: config.check_missing_checked_exceptions.unwrap_or(false),
        check_strict_printf_placeholder_types: config
            .check_strict_printf_placeholder_types
            .unwrap_or(false),
        check_too_wide_throw_type_in_protected_and_public_methods: config
            .check_too_wide_throw_type_in_protected_and_public_methods
            .unwrap_or(false),
        check_too_wide_return_type_in_protected_and_public_methods: config
            .check_too_wide_return_type_in_protected_and_public_methods
            .unwrap_or(false),
        check_too_wide_by_ref_type_in_protected_and_public_methods: config
            .check_too_wide_by_ref_type_in_protected_and_public_methods
            .unwrap_or(false),
        check_method_throw_type_covariance: config
            .check_method_throw_type_covariance
            .unwrap_or(false),
        check_benevolent_union_types: config.check_benevolent_union_types.unwrap_or(false),
        check_missing_callable_signature: config.check_missing_callable_signature.unwrap_or(false),
        check_duplicate_function_declarations: config
            .check_duplicate_function_declarations
            .unwrap_or(false),
        check_duplicate_class_declarations: config
            .check_duplicate_class_declarations
            .unwrap_or(false),
        check_boolean_trap: config.check_boolean_trap.unwrap_or(false),
        check_array_key_cast: config.check_array_key_cast.unwrap_or(false),
        check_parent_constructor_call: config.check_parent_constructor_call.unwrap_or(false),
        check_promoted_property_suggestions: config
            .check_promoted_property_suggestions
            .unwrap_or(false),
        check_boolean_conditions: config.check_boolean_conditions.unwrap_or(false),
        check_dynamic_properties: config.check_dynamic_properties.unwrap_or(false),
        check_swapped_arguments: config.check_swapped_arguments.unwrap_or(false),
        check_never_return_suggestions: config.check_never_return_suggestions.unwrap_or(false),
        check_switch_missing_default: config.check_switch_missing_default.unwrap_or(false),
        check_throw_without_message: config.check_throw_without_message.unwrap_or(false),
        check_throw_base_class: config.check_throw_base_class.unwrap_or(false),
        check_exception_hygiene: config.check_exception_hygiene.unwrap_or(false),
        check_identity_vs_equals: config.check_identity_vs_equals.unwrap_or(false),
        check_loose_haystack_comparison: config.check_loose_haystack_comparison.unwrap_or(false),
        check_strict_function_calls: config.check_strict_function_calls.unwrap_or(false),
        check_switch_case_types: config.check_switch_case_types.unwrap_or(false),
        check_loop_variable_overwrite: config.check_loop_variable_overwrite.unwrap_or(false),
        check_dynamic_static_calls: config.check_dynamic_static_calls.unwrap_or(false),
        check_illegal_constructor_calls: config.check_illegal_constructor_calls.unwrap_or(false),
        check_enum_exhaustiveness: config.check_enum_exhaustiveness.unwrap_or(false),
        check_null_coercion: config.check_null_coercion.unwrap_or(false),
        check_new_exception_discarded: config.check_new_exception_discarded.unwrap_or(false),
        check_risky_tests: config.check_risky_tests.unwrap_or(false),
        php_version,
        assertion_entries: config.assertions.take().unwrap_or_default(),
        class_argument_returns: config.class_argument_returns.take().unwrap_or_default(),
        restricted_symbol_entries,
        dynamic_constant_names: config
            .dynamic_constant_names
            .as_ref()
            .map(crate::config::DynamicConstantNames::entries)
            .unwrap_or_default(),
        doctrine_custom_function_entries: config.doctrine_custom_functions.unwrap_or_default(),
        sql_reflection_config,
        sql_reflection_schema,
        phpstan_api_check_enabled,
        vendor_context,
        extra_context_dirs,
        baseline,
        ignore_error_entries: config.ignore_errors.unwrap_or_default(),
        analyze_paths,
    }
}
