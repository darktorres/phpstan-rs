//! Flags an `abstract class` with zero concrete descendants anywhere in
//! the closed universe - a permanent structural fact, not "currently
//! unused" the way `class.unused` is. Real PHPStan has no such rule at
//! all: its `Reflection/` layer has no descendant-enumeration API (an
//! open-world consumer could always add one).
//!
//! Distinct from `class.unused`: a polymorphic-dispatch-only abstract
//! class (referenced only through a type hint on its own name, never
//! `new`'d directly - which real PHP forbids for an abstract class
//! anyway) has non-zero Tier-1 references there even with zero real
//! subclasses, so `class.unused` would never catch this; this check would, since it
//! asks a structurally different question (does a *descendant* exist,
//! not does a *reference* exist).
//!
//! Narrower than inferring a *sealed* hierarchy for
//! `ObjectType::isInstanceOf` to answer a certain `false` instead of
//! PHPStan's permanent `Maybe`: this only asks whether *any* concrete
//! descendant exists - answered directly here
//! via `Registry::all_ancestor_names`, inverted (a witness class's own
//! ancestors include every abstract superclass it has a concrete
//! descendant for), rather than building a persistent reverse index on
//! `Registry` that nothing else yet needs.
//!
//! Real, disclosed false-positive risk (ship opt-in): a concrete
//! subclass generated only at runtime by framework magic (a Doctrine
//! proxy, a mocking framework) is invisible to this project's static,
//! closed-world registry - a real, structural absence in the *analyzed*
//! universe is reported as one in the *real* universe, which isn't
//! always true.

use crate::registry::Registry;
use syntax::ast::ClassKind;
use syntax::name::NameSet;

pub(crate) fn find_dead_abstract_classes(registry: &Registry) -> Vec<String> {
    let mut has_concrete_descendant: NameSet = NameSet::default();
    for (fqn, info) in &registry.classes {
        // Only a real, instantiable class counts as a witness - an
        // abstract subclass, an interface, or a trait proves nothing
        // about whether *this* class can ever actually be constructed.
        if info.kind() != ClassKind::Class || info.is_abstract() {
            continue;
        }
        has_concrete_descendant.extend(registry.all_ancestor_names(fqn));
    }

    registry
        .classes
        .iter()
        .filter(|(_, info)| info.kind() == ClassKind::Class && info.is_abstract())
        .filter(|(fqn, _)| !has_concrete_descendant.contains(fqn.as_ref()))
        .map(|(fqn, _)| fqn.to_string())
        .collect()
}
