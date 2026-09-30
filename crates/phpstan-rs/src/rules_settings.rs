//! `[rules]`: the core rules' parameters, and the whole-program checks
//! each turned on on its own.

use serde::Deserialize;

/// `[rules]`. Its sub-tables are sections of their own:
/// `[rules.banned]` (`banned::BannedSettings`), `[[rules.architecture]]`
/// (`architecture::ArchitectureSettings`) and `[rules.class_shape]`
/// (`parent_calls::ClassShapeSettings`).
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct RulesSettings {
    /// An untyped array (`array`, `Array(Mixed)`) no longer satisfies
    /// every array or shape type it's checked against.
    pub strict_mixed_arrays: bool,
    /// PHPStan's `checkBenevolentUnionTypes`.
    pub check_benevolent_union_types: bool,
    /// PHPStan's `reportPossiblyNonexistentGeneralArrayOffset`: a read of
    /// a general array (`array<K, V>`) at a key it may not hold is
    /// reported (`offsetAccess.mightNotExist`), like one a shape says may
    /// be missing.
    pub report_possibly_nonexistent_general_array_offset: bool,
    /// PHPStan's `checkUninitializedProperties`: a typed property nothing
    /// in the class body assigns (`property.uninitialized`).
    pub check_uninitialized_properties: bool,
    /// `@internal` symbols used from outside their top-level namespace
    /// (`internal.*`).
    pub check_internal_usage: bool,
    /// PHPStan's `checkFunctionNameCase` (`function.nameCase`).
    pub check_function_name_case: bool,
    /// PHPStan's `checkStrictPrintfPlaceholderTypes`.
    pub check_strict_printf_placeholder_types: bool,
    /// PHPStan's `checkTooWideThrowTypesInProtectedAndPublicMethods`.
    pub check_too_wide_throw_type_in_protected_and_public_methods: bool,
    /// The return-type analogue.
    pub check_too_wide_return_type_in_protected_and_public_methods: bool,
    /// The by-ref-parameter analogue.
    pub check_too_wide_by_ref_type_in_protected_and_public_methods: bool,
    /// PHPStan's `MethodThrowTypeCovarianceRule`.
    pub check_method_throw_type_covariance: bool,
    /// PHPStan's `checkDynamicProperties` inside `isset()`/`empty()`/`??`.
    pub check_dynamic_properties: bool,
    /// PHPStan's `checkMissingCallableSignature` (`missingType.callable`).
    pub check_missing_callable_signature: bool,
    /// Two declarations of one function across the project.
    pub check_duplicate_function_declarations: bool,
    /// Two declarations of one class-like across the project.
    pub check_duplicate_class_declarations: bool,
    /// Enum cases nothing uses.
    pub check_dead_enum_cases: bool,
    /// Functions and classes nothing uses.
    pub check_dead_code: bool,
    /// Namespaces that depend on each other in a cycle.
    pub check_circular_dependencies: bool,
    /// Abstract classes nothing extends.
    pub check_dead_abstract_classes: bool,
    /// Protected and public members nothing uses.
    pub check_dead_members: bool,
    /// Named arguments an override's parameter rename breaks
    /// (`argument.parameterRenamedInSubtype`).
    pub check_renamed_named_arguments: bool,
    /// The analysed code against the root `composer.json`: declared,
    /// unused and misplaced dependencies, PSR-4 locations, required
    /// extensions, `provide`, `scripts`, `config.platform.php`.
    pub check_composer_hygiene: bool,
}

impl config::Section for RulesSettings {
    const PATH: &'static [&'static str] = &["rules"];
}
