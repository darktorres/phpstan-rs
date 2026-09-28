//! What a type is: the questions asked of a type from outside this crate.

use crate::Type;

impl Type {
    /// Whether this is `mixed`, written or not.
    pub fn is_mixed(&self) -> bool {
        matches!(self, Type::Mixed(_))
    }

    /// Whether this is a `mixed` a written type resolved to.
    pub fn is_explicit_mixed(&self) -> bool {
        matches!(self, Type::Mixed(true))
    }

    /// Whether this is a `mixed` with no type information behind it.
    pub fn is_implicit_mixed(&self) -> bool {
        matches!(self, Type::Mixed(false))
    }
}
