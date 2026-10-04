//! Flags a top-level function or class-like
//! declaration (class/interface/trait/enum) never referenced anywhere in
//! the project - no `new`/`extends`/`implements`/`use`/`instanceof`/type-
//! hint/`catch`/attribute for a class-like, no call or first-class
//! callable for a function, names compared as PHP compares them. An
//! unqualified call in a namespace counts for the namespaced function and
//! for the global one it falls back to, since which is declared when it
//! runs decides. Built on `Project::use_sites`'s Tier 1 index (`RefTarget::
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
//! `Name` node for Tier 1 to resolve; anything
//! reachable only through framework magic/reflection (a DI container
//! definition, a Symfony/Laravel service tag, `class_exists`/
//! `ReflectionClass`). `__autoload` (PHP's own deprecated magic autoload
//! function, called by the engine itself with no source-level reference
//! anywhere) is the only hard-coded exclusion - every other magic-
//! callable shape above is disclosed, not special-cased.

use crate::project::use_sites::{RefTarget, UseSiteIndex};
use crate::project::whole_program::Declaration;

/// The function declarations of the analysed files that nothing may
/// call - see this module's own doc comment for what counts as "used"
/// and what's disclosed, not fixed. A name declared twice is judged at
/// each declaration.
pub(crate) fn find_dead_functions<'a>(
    declarations: impl IntoIterator<Item = Declaration<'a>>,
    use_sites: &UseSiteIndex,
) -> Vec<Declaration<'a>> {
    declarations
        .into_iter()
        .filter(|declaration| !declaration.name.eq_ignore_ascii_case("__autoload"))
        .filter(|declaration| use_sites.function_references(declaration.name).is_empty())
        .collect()
}

/// The class-like declarations of the analysed files, anonymous ones
/// apart, that nothing references - `find_dead_functions` for classes.
pub(crate) fn find_dead_classes<'a>(
    declarations: impl IntoIterator<Item = Declaration<'a>>,
    use_sites: &UseSiteIndex,
) -> Vec<Declaration<'a>> {
    declarations
        .into_iter()
        .filter(|declaration| {
            use_sites
                .references(&RefTarget::Class(declaration.name.to_string()))
                .is_empty()
        })
        .collect()
}
