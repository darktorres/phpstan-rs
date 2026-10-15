//! What the registry works out when it is asked rather than when it is
//! built: whether a function or method is pure (`registry::purity`) and
//! whose properties calling it may write (`registry::property_writes`).
//! Only a few declarations are asked about, and each is settled by
//! following the calls of its body, so no declaration is settled that
//! nothing asks about. An answer depends on the declarations and the
//! bodies alone, so it is kept until they change.

use std::sync::{PoisonError, RwLock};

use rustc_hash::FxHashMap;

use crate::registry::property_writes::{DeclKey, DeclQuery, PropertyWrites};

/// One answer, with what it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Pure(bool),
    Writes(PropertyWrites),
}

/// The answers found so far, by declaration. Searches run beside each
/// other, so these are behind locks; a search holds none while it runs.
#[derive(Debug, Default)]
pub(in crate::registry) struct Demand {
    pure: RwLock<FxHashMap<DeclKey, bool>>,
    writes: RwLock<FxHashMap<DeclKey, PropertyWrites>>,
}

/// A copy of a registry starts with no answers: they are no part of what
/// its declarations are.
impl Clone for Demand {
    fn clone(&self) -> Demand {
        Demand::default()
    }
}

impl Demand {
    pub(in crate::registry) fn pure_of(&self, query: &dyn DeclQuery) -> Option<bool> {
        self.pure
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(query)
            .copied()
    }

    pub(in crate::registry) fn writes_of(&self, query: &dyn DeclQuery) -> Option<PropertyWrites> {
        self.writes
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(query)
            .copied()
    }

    pub(in crate::registry) fn record_pure(&self, found: Vec<(DeclKey, bool)>) {
        self.pure
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(found);
    }

    pub(in crate::registry) fn record_writes(&self, found: Vec<(DeclKey, PropertyWrites)>) {
        self.writes
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(found);
    }

    /// Takes every answer, for the declarations to change.
    pub(in crate::registry) fn take(&self) -> Vec<(DeclKey, Answer)> {
        let pure = std::mem::take(&mut *self.pure.write().unwrap_or_else(PoisonError::into_inner));
        let writes =
            std::mem::take(&mut *self.writes.write().unwrap_or_else(PoisonError::into_inner));
        pure.into_iter()
            .map(|(key, pure)| (key, Answer::Pure(pure)))
            .chain(
                writes
                    .into_iter()
                    .map(|(key, writes)| (key, Answer::Writes(writes))),
            )
            .collect()
    }
}
