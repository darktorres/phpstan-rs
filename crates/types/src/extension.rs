//! Extension-owned type payloads: a type an extension tracks more
//! precisely than its PHP class (PHPStan's `QueryBuilderType`-style
//! types), opaque to everything but the relations' hooks below.

use std::any::Any;
use std::fmt;
use std::hash::Hasher;

use crate::{ClassHierarchy, TrinaryLogic, Type};

pub trait ExtensionTypeData: fmt::Debug + Send + Sync {
    /// The type PHP itself sees (`Doctrine\ORM\QueryBuilder`): what a
    /// relation against anything but a payload of the same extension
    /// reads, and what `widen` falls back to.
    fn base(&self) -> Type;

    fn as_any(&self) -> &dyn Any;

    /// Equality with another payload; `false` for another extension's.
    fn eq_data(&self, other: &dyn ExtensionTypeData) -> bool;

    /// Hashes the payload consistently with `eq_data`.
    fn hash_data(&self, state: &mut dyn Hasher);

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    /// Whether this payload is a super-type of `other`, when both are
    /// this extension's; `None` defers to the base types.
    fn is_super_type_of(
        &self,
        other: &dyn ExtensionTypeData,
        hierarchy: &dyn ClassHierarchy,
    ) -> Option<TrinaryLogic>;

    /// What a union past its member cap may collapse this payload to,
    /// instead of `mixed`. Joins below the cap keep distinct payloads
    /// distinct.
    fn widen(&self) -> Option<Type> {
        None
    }
}
