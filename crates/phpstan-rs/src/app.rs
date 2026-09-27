//! What the command-line front ends run, below their argument parsing:
//! the composition root, which knows every owner of a config section.

pub mod check_project;

use crate::architecture::ArchitectureSettings;
use crate::assertion_tables::OverlaySettings;
use crate::banned::BannedSettings;
use crate::checker::settings::EngineSettings;
use crate::ext::deprecation_rules::DeprecationRulesSettings;
use crate::ext::disallowed_calls::DisallowedCallsSettings;
use crate::ext::doctrine::DoctrineSettings;
use crate::ext::extras::ExtrasSettings;
use crate::ext::language_extensions::LanguageExtensionsSettings;
use crate::ext::mockstan::MockstanSettings;
use crate::ext::pest::PestSettings;
use crate::ext::phpunit::PhpunitSettings;
use crate::ext::psr3::Psr3Settings;
use crate::ext::shipmonk::ShipmonkSettings;
use crate::ext::sidz::SidzSettings;
use crate::ext::slam::SlamSettings;
use crate::ext::sql::SqlSettings;
use crate::ext::strict_rules::StrictRulesSettings;
use crate::ext::voku::VokuSettings;
use crate::parent_calls::ClassShapeSettings;
use crate::project::session::SessionSettings;
use crate::registry::settings::ReflectionSettings;
use crate::rules_settings::RulesSettings;

/// Every config section, and where each key of the flat layout moved.
pub fn config_schema() -> config::Schema {
    let mut schema = config::Schema::new()
        .section::<SessionSettings>()
        .section::<ReflectionSettings>()
        .section::<EngineSettings>()
        .section::<OverlaySettings>()
        .section::<RulesSettings>()
        .section::<BannedSettings>()
        .section::<ArchitectureSettings>()
        .section::<ClassShapeSettings>()
        .section::<ExtrasSettings>()
        .section::<StrictRulesSettings>()
        .section::<ShipmonkSettings>()
        .section::<VokuSettings>()
        .section::<SlamSettings>()
        .section::<Psr3Settings>()
        .section::<SidzSettings>()
        .section::<PhpunitSettings>()
        .section::<PestSettings>()
        .section::<DeprecationRulesSettings>()
        .section::<DisallowedCallsSettings>()
        .section::<DoctrineSettings>()
        .section::<SqlSettings>()
        .section::<MockstanSettings>()
        .section::<LanguageExtensionsSettings>();
    for &(old, new) in MOVED_KEYS {
        schema = schema.moved(old, new);
    }
    schema
}

/// The flat layout's keys, and their place now.
const MOVED_KEYS: &[(&str, &[&str])] = &[
    ("strict_mixed_arrays", &["rules", "strict_mixed_arrays"]),
    (
        "check_benevolent_union_types",
        &["rules", "check_benevolent_union_types"],
    ),
    (
        "check_uninitialized_properties",
        &["rules", "check_uninitialized_properties"],
    ),
    ("check_internal_usage", &["rules", "check_internal_usage"]),
    (
        "check_function_name_case",
        &["rules", "check_function_name_case"],
    ),
    (
        "check_strict_printf_placeholder_types",
        &["rules", "check_strict_printf_placeholder_types"],
    ),
    (
        "check_too_wide_throw_type_in_protected_and_public_methods",
        &[
            "rules",
            "check_too_wide_throw_type_in_protected_and_public_methods",
        ],
    ),
    (
        "check_too_wide_return_type_in_protected_and_public_methods",
        &[
            "rules",
            "check_too_wide_return_type_in_protected_and_public_methods",
        ],
    ),
    (
        "check_too_wide_by_ref_type_in_protected_and_public_methods",
        &[
            "rules",
            "check_too_wide_by_ref_type_in_protected_and_public_methods",
        ],
    ),
    (
        "check_method_throw_type_covariance",
        &["rules", "check_method_throw_type_covariance"],
    ),
    (
        "check_dynamic_properties",
        &["rules", "check_dynamic_properties"],
    ),
    (
        "check_missing_callable_signature",
        &["rules", "check_missing_callable_signature"],
    ),
    (
        "check_duplicate_function_declarations",
        &["rules", "check_duplicate_function_declarations"],
    ),
    (
        "check_duplicate_class_declarations",
        &["rules", "check_duplicate_class_declarations"],
    ),
    ("check_dead_enum_cases", &["rules", "check_dead_enum_cases"]),
    ("check_dead_code", &["rules", "check_dead_code"]),
    (
        "check_circular_dependencies",
        &["rules", "check_circular_dependencies"],
    ),
    (
        "check_dead_abstract_classes",
        &["rules", "check_dead_abstract_classes"],
    ),
    ("check_dead_members", &["rules", "check_dead_members"]),
    (
        "check_renamed_named_arguments",
        &["rules", "check_renamed_named_arguments"],
    ),
    (
        "check_composer_hygiene",
        &["rules", "check_composer_hygiene"],
    ),
    ("banned_constructs", &["rules", "banned", "constructs"]),
    ("banned_types", &["rules", "banned", "types"]),
    ("architecture", &["rules", "architecture"]),
    (
        "check_closed_classes",
        &["rules", "class_shape", "check_closed_classes"],
    ),
    ("parent_calls", &["rules", "class_shape", "parent_calls"]),
    (
        "default_parent_calls",
        &["rules", "class_shape", "default_parent_calls"],
    ),
    (
        "check_missing_checked_exceptions",
        &["ext", "extras", "check_missing_checked_exceptions"],
    ),
    (
        "check_empty_method_bodies",
        &["ext", "extras", "check_empty_method_bodies"],
    ),
    (
        "check_boolean_trap",
        &["ext", "extras", "check_boolean_trap"],
    ),
    (
        "check_swapped_arguments",
        &["ext", "extras", "check_swapped_arguments"],
    ),
    (
        "check_array_key_cast",
        &["ext", "extras", "check_array_key_cast"],
    ),
    (
        "check_never_return_suggestions",
        &["ext", "extras", "check_never_return_suggestions"],
    ),
    (
        "check_promoted_property_suggestions",
        &["ext", "extras", "check_promoted_property_suggestions"],
    ),
    (
        "check_switch_missing_default",
        &["ext", "extras", "check_switch_missing_default"],
    ),
    (
        "check_throw_without_message",
        &["ext", "extras", "check_throw_without_message"],
    ),
    (
        "check_throw_base_class",
        &["ext", "extras", "check_throw_base_class"],
    ),
    (
        "check_exception_hygiene",
        &["ext", "extras", "check_exception_hygiene"],
    ),
    (
        "check_new_exception_discarded",
        &["ext", "extras", "check_new_exception_discarded"],
    ),
    (
        "check_loose_haystack_comparison",
        &["ext", "extras", "check_loose_haystack_comparison"],
    ),
    (
        "check_identity_vs_equals",
        &["ext", "extras", "check_identity_vs_equals"],
    ),
    (
        "check_native_type_hints",
        &["ext", "extras", "check_native_type_hints"],
    ),
    (
        "check_parent_constructor_call",
        &["ext", "strict-rules", "check_parent_constructor_call"],
    ),
    (
        "check_boolean_conditions",
        &["ext", "strict-rules", "check_boolean_conditions"],
    ),
    (
        "check_useless_casts",
        &["ext", "strict-rules", "check_useless_casts"],
    ),
    (
        "check_strict_array_filter",
        &["ext", "strict-rules", "check_strict_array_filter"],
    ),
    (
        "check_strict_function_calls",
        &["ext", "strict-rules", "check_strict_function_calls"],
    ),
    (
        "check_switch_case_types",
        &["ext", "strict-rules", "check_switch_case_types"],
    ),
    (
        "check_loop_variable_overwrite",
        &["ext", "strict-rules", "check_loop_variable_overwrite"],
    ),
    (
        "check_dynamic_static_calls",
        &["ext", "strict-rules", "check_dynamic_static_calls"],
    ),
    (
        "check_illegal_constructor_calls",
        &["ext", "strict-rules", "check_illegal_constructor_calls"],
    ),
    (
        "check_implicit_array_creation",
        &["ext", "strict-rules", "check_implicit_array_creation"],
    ),
    (
        "check_arithmetic_operands",
        &["ext", "strict-rules", "check_arithmetic_operands"],
    ),
    (
        "check_comparable_types",
        &["ext", "shipmonk", "check_comparable_types"],
    ),
    (
        "check_null_coercion",
        &["ext", "shipmonk", "check_null_coercion"],
    ),
    (
        "check_enum_exhaustiveness",
        &["ext", "shipmonk", "check_enum_exhaustiveness"],
    ),
    (
        "check_mixed_type_operands",
        &["ext", "voku", "check_mixed_type_operands"],
    ),
    (
        "check_current_time_dependence",
        &["ext", "slam", "check_current_time_dependence"],
    ),
    ("check_psr3_logging", &["ext", "psr3", "check_psr3_logging"]),
    (
        "check_psr3_exception_context",
        &["ext", "psr3", "check_psr3_exception_context"],
    ),
    (
        "psr3_context_key_pattern",
        &["ext", "psr3", "psr3_context_key_pattern"],
    ),
    (
        "check_magic_numbers",
        &["ext", "sidz", "check_magic_numbers"],
    ),
    (
        "magic_number_exemptions",
        &["ext", "sidz", "magic_number_exemptions"],
    ),
    (
        "magic_numbers_ignore_numeric_strings",
        &["ext", "sidz", "magic_numbers_ignore_numeric_strings"],
    ),
    (
        "check_risky_tests",
        &["ext", "phpunit", "check_risky_tests"],
    ),
    (
        "check_deprecated_usage",
        &["ext", "deprecation-rules", "enabled"],
    ),
    (
        "restricted_symbols",
        &["ext", "disallowed-calls", "restricted_symbols"],
    ),
    (
        "restricted_symbol_presets",
        &["ext", "disallowed-calls", "restricted_symbol_presets"],
    ),
    (
        "doctrine_custom_functions",
        &["ext", "doctrine", "custom_functions"],
    ),
    ("sql_reflection", &["ext", "sql", "reflection"]),
    (
        "check_mock_hygiene",
        &["ext", "mockstan", "check_mock_hygiene"],
    ),
    (
        "mock_banned_classes",
        &["ext", "mockstan", "mock_banned_classes"],
    ),
];

#[cfg(test)]
mod tests;
