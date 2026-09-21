//! Phase 92 (PHASE_BACKLOG): flag a declared enum case never referenced
//! via `EnumName::CaseName`, never a `match`/`switch` arm value, and
//! never provably produced by a literal `::from()`/`::tryFrom()`
//! argument, project-wide. All three signals are already free once
//! Tier 2 (`Project::ensure_member_index_built`) runs: `record_member_ref`
//! already fires for a resolved class-constant access from both
//! `check_expr`'s and `infer_expr`'s `StaticAccess` arms (which is what a
//! plain reference, a `match`/`switch` arm value, and a property/const
//! default value all resolve through - `checker::expr::collect_default_
//! and_const_value_member_refs` reaches the last of those, since
//! `check_program`'s ordinary statement walk never visits it on its own);
//! the `::from()`/`::tryFrom()` signal was the one genuinely missing
//! piece, now wired at both of `narrow_enum_from_call`'s own call sites
//! in `checker/expr.rs`.
//!
//! Any `::cases()` call or a dynamic (`{dynamic}`) member access on a
//! real enum escapes the *whole* enum (conservative, all-or-nothing) -
//! see `Checker::escaped_enums`'s own doc comment for where that's
//! tracked, and `checker/expr.rs`'s own `StaticCall`/`StaticAccess` arms
//! for the exact hooks.
//!
//! Real, disclosed false-positive risks (ship opt-in, per this project's
//! own convention - see `Registry::check_dead_enum_cases`'s own doc
//! comment): a case reachable only through framework magic/reflection; a
//! case referenced only from inside a `Closure`/`ArrowFn` literal's own
//! default parameter value (inherited from `collect_default_and_const_
//! value_member_refs`'s own disclosed gap); a case referenced only via
//! PHP 8.1 first-class-callable syntax on `cases()` specifically
//! (deliberately not handled - see `checker/expr.rs`'s own `ExprKind::
//! FirstClassCallable` arm, an unrelated pre-existing gap this project
//! doesn't chase for such a vanishingly unrealistic pattern); a case
//! referenced only through a receiver this project can't statically
//! resolve to a real class name at all (a dynamic *class* half, e.g.
//! `$class::CONST`).

use std::collections::HashSet;

use crate::ast::ClassKind;
use crate::project::use_sites::{RefTarget, UseSiteIndex};
use crate::registry::Registry;
use crate::types::Type;

/// Every `(enum FQN, case name)` pair with no recorded usage anywhere in
/// the project - see this module's own doc comment for what counts as
/// "used" and what's disclosed, not fixed. Callers resolve each pair's
/// own reportable location via `Project::decl_locations` themselves
/// (this function stays unaware of files/spans/`Diagnostic` entirely -
/// see `Project::check_whole_program`'s own doc comment for why).
pub(crate) fn find_dead_cases(
    registry: &Registry,
    use_sites: &UseSiteIndex,
    escaped_enum_cases: &HashSet<String>,
) -> Vec<(String, String)> {
    let mut dead = Vec::new();
    for (fqn, info) in &registry.classes {
        if info.kind != ClassKind::Enum || escaped_enum_cases.contains(fqn.as_ref()) {
            continue;
        }
        for (case_name, sig) in &info.consts {
            if !matches!(&sig.ty, Type::EnumCase(..)) {
                continue;
            }
            let target = RefTarget::Member(fqn.to_string(), case_name.to_string());
            if use_sites.references(&target).is_empty() {
                dead.push((fqn.to_string(), case_name.to_string()));
            }
        }
    }
    dead
}
