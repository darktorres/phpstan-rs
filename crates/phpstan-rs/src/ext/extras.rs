//! The phpstan-rs extras: opt-in checks no PHPStan extension provides in this shape.

use serde::Deserialize;

/// `[ext.extras]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ExtrasSettings<T = Option<bool>> {
    /// Whether the extras are active; no package provides them, so unset
    /// is off.
    pub enabled: T,
    /// A thrown or called checked exception neither caught nor declared.
    pub check_missing_checked_exceptions: T,
    /// A concrete method with a non-void return type and an empty body (`return.missing`/`return.never`).
    pub check_empty_method_bodies: T,
    /// A `bool` parameter a call passes a literal to.
    pub check_boolean_trap: T,
    /// Adjacent arguments whose variable names match each other's parameters (`arguments.possiblySwapped`).
    pub check_swapped_arguments: T,
    /// An array key PHP silently casts (`array.keyCast`, `offsetAccess.keyCast`).
    pub check_array_key_cast: T,
    /// A function that never returns without a `never` return type.
    pub check_never_return_suggestions: T,
    /// A constructor property assignment that could be promoted.
    pub check_promoted_property_suggestions: T,
    /// A `switch` with no `default:` arm.
    pub check_switch_missing_default: T,
    /// An exception thrown without a message.
    pub check_throw_without_message: T,
    /// A bare `Exception` or `Error` thrown.
    pub check_throw_base_class: T,
    /// A `catch` that neither chains the caught exception nor, for a base type, rethrows.
    pub check_exception_hygiene: T,
    /// A `new` exception never thrown, returned or stored.
    pub check_new_exception_discarded: T,
    /// `in_array()`/`array_search()`/`array_keys()` compared loosely where `==` and `===` differ (`function.looseComparison`).
    pub check_loose_haystack_comparison: T,
    /// `===`/`!==` between objects whose class has an `equals()` accepting the other (`identical.equalsMethod`).
    pub check_identity_vs_equals: T,
    /// A PHPDoc-only parameter or return type the PHP version can declare natively (`missingNativeType.*`).
    pub check_native_type_hints: T,
}

impl config::Section for ExtrasSettings {
    const PATH: &'static [&'static str] = &["ext", "extras"];
}

impl config::Extension for ExtrasSettings {
    const PACKAGES: &'static [&'static str] = &[];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl ExtrasSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> ExtrasSettings<bool> {
        ExtrasSettings {
            enabled: active,
            check_missing_checked_exceptions: self
                .check_missing_checked_exceptions
                .unwrap_or(active),
            check_empty_method_bodies: self.check_empty_method_bodies.unwrap_or(active),
            check_boolean_trap: self.check_boolean_trap.unwrap_or(active),
            check_swapped_arguments: self.check_swapped_arguments.unwrap_or(active),
            check_array_key_cast: self.check_array_key_cast.unwrap_or(active),
            check_never_return_suggestions: self.check_never_return_suggestions.unwrap_or(active),
            check_promoted_property_suggestions: self
                .check_promoted_property_suggestions
                .unwrap_or(active),
            check_switch_missing_default: self.check_switch_missing_default.unwrap_or(active),
            check_throw_without_message: self.check_throw_without_message.unwrap_or(active),
            check_throw_base_class: self.check_throw_base_class.unwrap_or(active),
            check_exception_hygiene: self.check_exception_hygiene.unwrap_or(active),
            check_new_exception_discarded: self.check_new_exception_discarded.unwrap_or(active),
            check_loose_haystack_comparison: self.check_loose_haystack_comparison.unwrap_or(active),
            check_identity_vs_equals: self.check_identity_vs_equals.unwrap_or(active),
            check_native_type_hints: self.check_native_type_hints.unwrap_or(active),
        }
    }
}
