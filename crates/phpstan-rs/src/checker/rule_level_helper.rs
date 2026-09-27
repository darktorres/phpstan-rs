//! PHPStan's `RuleLevelHelper`: the settings that make type acceptance
//! more or less lenient, applied over a class hierarchy's relations.

use types::{ClassHierarchy, Relation};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RuleLevelHelper {
    /// The configured rule level, before any default is applied.
    pub configured_level: Option<u8>,
    /// `array<mixed>` and a `mixed` shape field stop meaning "unknown".
    pub strict_mixed_arrays: bool,
    /// PHPStan's `checkBenevolentUnionTypes`.
    pub check_benevolent_union_types: bool,
}

impl RuleLevelHelper {
    /// The relations over `hierarchy`, as lenient as these settings say.
    pub fn relation<'h>(&self, hierarchy: &'h dyn ClassHierarchy) -> Relation<'h> {
        Relation {
            hierarchy,
            strict_mixed_arrays: self.strict_mixed_arrays,
            configured_level: self.configured_level,
            check_benevolent_union_types: self.check_benevolent_union_types,
        }
    }
}

#[cfg(test)]
mod tests;
