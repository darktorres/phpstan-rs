//! The cross-referencing half of Phase 82's named-argument-rename
//! feature (`checker::named_argument_renames`'s own module doc comment
//! covers the recording half) - mirrors real PHPStan's
//! `MethodCallWithPossiblyRenamedNamedArgumentRule::processNode` exactly:
//! for every override anywhere that renamed a parameter relative to its
//! own prototype, look up every recorded call site that used the
//! *prototype's* own (now-stale) parameter name against the prototype's
//! own declaring class/method, and report each one - regardless of
//! whether that particular call site's own receiver could concretely be
//! the renaming subtype, since PHP resolves a named argument against
//! whichever method is *actually* dispatched to at runtime, never the
//! statically-declared one.

use crate::checker::{RenamedParameterOverride, RenamedParameterOverrideFacts};
use crate::token::Span;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Project-wide merge of every file's own `NamedArgumentCallFacts`,
/// each call site now tagged with the file it came from (a bare
/// `Checker` has no notion of its own path - `Project::collect_named_
/// argument_facts` attaches it while merging).
pub(crate) type NamedArgumentCallFactsWithPath =
    HashMap<(Arc<str>, Arc<str>, Arc<str>), Vec<(PathBuf, Span)>>;

pub(crate) fn find_renamed_named_argument_collisions(
    calls: &NamedArgumentCallFactsWithPath,
    overrides: &RenamedParameterOverrideFacts,
) -> Vec<(PathBuf, Span, String, String)> {
    let mut collisions = Vec::new();
    for RenamedParameterOverride {
        prototype_class,
        method,
        prototype_param,
        overriding_class,
        overriding_param,
    } in overrides
    {
        let Some(sites) = calls.get(&(
            prototype_class.clone(),
            method.clone(),
            prototype_param.clone(),
        )) else {
            continue;
        };
        let location = format!("{prototype_class}::{method}");
        for (path, span) in sites {
            collisions.push((
                path.clone(),
                *span,
                location.clone(),
                format!(
                    "Call to {prototype_class}::{method}() uses named argument for parameter \
                     ${prototype_param}, but {overriding_class} renames it to ${overriding_param}."
                ),
            ));
        }
    }
    collisions
}
