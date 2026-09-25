//! The type relations: whether one type accepts another, is a super-type
//! of it, or provably shares no value with it, read against a
//! `ClassHierarchy`.

mod accepts;
mod super_type;

use crate::ClassHierarchy;

/// The relations over one class hierarchy, with the settings that make
/// `accepts` more or less lenient.
pub struct Relation<'h> {
    pub hierarchy: &'h dyn ClassHierarchy,
    /// `array<mixed>` and a `mixed` shape field stop meaning "unknown".
    pub strict_mixed_arrays: bool,
    /// The configured rule level, before any default is applied.
    pub configured_level: Option<u8>,
    /// PHPStan's `checkBenevolentUnionTypes`.
    pub check_benevolent_union_types: bool,
}
