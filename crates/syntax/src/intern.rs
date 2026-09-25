//! Interning into the process-wide [`Interner`], for code with no
//! interner of its own.

use std::sync::Arc;

use crate::name::Interner;

/// The shared copy of `s` in the global interner.
pub fn intern(s: &str) -> Arc<str> {
    Interner::global().str(s)
}
