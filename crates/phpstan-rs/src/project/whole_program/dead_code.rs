//! Flags a top-level function or class-like
//! declaration (class/interface/trait/enum) never referenced anywhere in
//! the project - no `new`/`extends`/`implements`/`use`/`instanceof`/type-
//! hint/`catch` for a class-like, no call or bare-callable `Name` for a
//! function. Built on `Project::use_sites`'s Tier 1 index (`RefTarget::
//! Class`/`RefTarget::Function`) - see `project::use_sites::index_class_
//! fn_refs_without_source`'s own doc comment for why that index is real
//! and canonically name-resolved even in a batch `check-project` run with
//! no retained source text, unlike `Project.dependents` (raw, unresolved,
//! bare-last-segment string matching - good enough for "might need a
//! re-check", nowhere near precise enough for "is this ever really
//! referenced": a class referenced only via a relatively- or fully-
//! qualified name (not its own bare, `use`-imported spelling) would
//! silently miss `dependents`'s own bare-name-keyed lookup and read as
//! dead when it plainly isn't - a real, ordinary-code false-positive
//! risk this check avoids entirely by relying on `use_sites`'s real
//! `NameContext`-resolved canonical FQNs instead).
//!
//! Real, disclosed false-positive risks (ship opt-in, per this project's
//! own convention - see `RulesSettings::check_dead_code`'s own doc comment):
//! a function/class that's this project's own public API, called only by
//! code outside the analyzed set (a library, not an application - real
//! PHPStan has no such rule at all, for exactly this open-world reason);
//! a function referenced only via a string-callable (`array_map(
//! 'myFunc', ...)`, `call_user_func('myFunc')`) - a string literal has no
//! `Name` node for Tier 1 to resolve; a class referenced only as a PHP
//! attribute target (`#[MyAttribute]` on some other declaration) -
//! `use_sites::ClassFnRefWalker` doesn't walk `attrs` fields at all (a
//! separate, pre-existing gap, not introduced here - unlike `deps.rs`'s
//! own identically-shaped gap, fixed separately for `Project.dependents`,
//! fixing this one risks corrupting `textDocument/rename`'s real,
//! precise rewrite spans for comparatively little benefit); anything
//! reachable only through framework magic/reflection (a DI container
//! definition, a Symfony/Laravel service tag, `class_exists`/
//! `ReflectionClass`). `__autoload` (PHP's own deprecated magic autoload
//! function, called by the engine itself with no source-level reference
//! anywhere) is the only hard-coded exclusion - every other magic-
//! callable shape above is disclosed, not special-cased.

use crate::project::use_sites::{RefTarget, UseSiteIndex};
use crate::registry::Registry;

/// Every top-level function with zero recorded `RefTarget::Function`
/// sites anywhere in the project - see this module's own doc comment for
/// what counts as "used" and what's disclosed, not fixed. Callers resolve
/// each name's own reportable location via `Project::decl_locations`
/// themselves (this function stays unaware of files/spans/`Diagnostic`
/// entirely - see `Project::check_whole_program`'s own doc comment for
/// why).
pub(crate) fn find_dead_functions(registry: &Registry, use_sites: &UseSiteIndex) -> Vec<String> {
    registry
        .functions
        .keys()
        .filter(|fqn| !fqn.eq_ignore_ascii_case("__autoload"))
        .filter(|fqn| {
            use_sites
                .references(&RefTarget::Function(fqn.to_string()))
                .is_empty()
        })
        .map(|fqn| fqn.to_string())
        .collect()
}

/// Every class/interface/trait/enum with zero recorded `RefTarget::Class`
/// sites anywhere in the project - same shape as `find_dead_functions`
/// above, for `Registry.classes` instead. An anonymous class is used
/// where it is declared.
pub(crate) fn find_dead_classes(registry: &Registry, use_sites: &UseSiteIndex) -> Vec<String> {
    registry
        .classes
        .iter()
        .filter(|(fqn, info)| {
            !info.is_anonymous()
                && use_sites
                    .references(&RefTarget::Class(fqn.to_string()))
                    .is_empty()
        })
        .map(|(fqn, _)| fqn.to_string())
        .collect()
}
