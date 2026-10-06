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
//! descendant exists, from the registry's descendants index
//! (`Registry::subclasses`).
//!
//! Real, disclosed false-positive risk (ship opt-in): a concrete
//! subclass generated only at runtime by framework magic (a Doctrine
//! proxy, a mocking framework) is invisible to this project's static,
//! closed-world registry - a real, structural absence in the *analyzed*
//! universe is reported as one in the *real* universe, which isn't
//! always true.

use crate::project::whole_program::Declaration;
use crate::registry::{ClassReflection, Registry};
use syntax::ast::ClassKind;

/// The abstract class declarations of the analysed files with no
/// concrete class below them. A name declared twice is judged at each
/// declaration, by what extends that name.
pub(crate) fn find_dead_abstract_classes<'a>(
    declarations: impl IntoIterator<Item = (Declaration<'a>, &'a ClassReflection)>,
    registry: &Registry,
) -> Vec<Declaration<'a>> {
    declarations
        .into_iter()
        .filter(|(_, info)| info.kind() == ClassKind::Class && info.is_abstract())
        .filter(|(declaration, _)| {
            // Only a real, instantiable class counts as a witness - an
            // abstract subclass proves nothing about whether this class
            // can ever actually be constructed.
            !registry.subclasses(declaration.name).iter().any(|name| {
                registry
                    .classes()
                    .get(name)
                    .is_some_and(|info| info.kind() == ClassKind::Class && !info.is_abstract())
            })
        })
        .map(|(declaration, _)| declaration)
        .collect()
}
