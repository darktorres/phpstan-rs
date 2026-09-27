//! phpstan/phpstan-strict-rules: each rule toggled on its own.

use serde::Deserialize;

/// `[ext.strict-rules]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct StrictRulesSettings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// A constructor that doesn't call its parent's (`constructor.missingParentCall`).
    pub check_parent_constructor_call: T,
    /// A non-boolean condition (`if.condNotBoolean`, ...).
    pub check_boolean_conditions: T,
    /// A cast to a type the expression already has (`cast.useless`).
    pub check_useless_casts: T,
    /// `array_filter()` without a callback over values of ambiguous truthiness (`arrayFilter.strict`).
    pub check_strict_array_filter: T,
    /// `in_array()`, `array_search()`, `array_keys()`, `base64_decode()` without their strict argument (`function.strict`).
    pub check_strict_function_calls: T,
    /// A `switch` case whose type never matches the subject's (`switch.type`).
    pub check_switch_case_types: T,
    /// A loop variable overwriting an existing one (`foreach.keyOverwrite`, ...).
    pub check_loop_variable_overwrite: T,
    /// A static method called through an instance (`staticMethod.dynamicCall`).
    pub check_dynamic_static_calls: T,
    /// `__construct()` called on an existing object (`constructor.call`).
    pub check_illegal_constructor_calls: T,
    /// `$x[...] = ...` creating `$x` implicitly (`variable.implicitArray`).
    pub check_implicit_array_creation: T,
    /// Non-numeric arithmetic operands (`plus.leftNonNumeric`, ...).
    pub check_arithmetic_operands: T,
}

impl config::Section for StrictRulesSettings {
    const PATH: &'static [&'static str] = &["ext", "strict-rules"];
}

impl config::Extension for StrictRulesSettings {
    const PACKAGES: &'static [&'static str] = &["phpstan/phpstan-strict-rules"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl StrictRulesSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> StrictRulesSettings<bool> {
        StrictRulesSettings {
            enabled: active,
            check_parent_constructor_call: self.check_parent_constructor_call.unwrap_or(active),
            check_boolean_conditions: self.check_boolean_conditions.unwrap_or(active),
            check_useless_casts: self.check_useless_casts.unwrap_or(active),
            check_strict_array_filter: self.check_strict_array_filter.unwrap_or(active),
            check_strict_function_calls: self.check_strict_function_calls.unwrap_or(active),
            check_switch_case_types: self.check_switch_case_types.unwrap_or(active),
            check_loop_variable_overwrite: self.check_loop_variable_overwrite.unwrap_or(active),
            check_dynamic_static_calls: self.check_dynamic_static_calls.unwrap_or(active),
            check_illegal_constructor_calls: self.check_illegal_constructor_calls.unwrap_or(active),
            check_implicit_array_creation: self.check_implicit_array_creation.unwrap_or(active),
            check_arithmetic_operands: self.check_arithmetic_operands.unwrap_or(active),
        }
    }
}
