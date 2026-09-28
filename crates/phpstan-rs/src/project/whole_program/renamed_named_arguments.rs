//! The cross-referencing half of Phase 82's named-argument-rename
//! feature (`checker::named_argument_renames`'s own module doc comment
//! covers the call-site recording half) - mirrors real PHPStan's
//! `MethodCallWithPossiblyRenamedNamedArgumentRule::processNode` exactly:
//! for every override anywhere that renamed a parameter relative to its
//! own prototype, look up every recorded call site that used the
//! *prototype's* own (now-stale) parameter name against the prototype's
//! own declaring class/method, and report each one - regardless of
//! whether that particular call site's own receiver could concretely be
//! the renaming subtype, since PHP resolves a named argument against
//! whichever method is *actually* dispatched to at runtime, never the
//! statically-declared one.

use crate::checker::NamedArgumentCallFacts;
use crate::registry::Registry;
use crate::registry::class_decl_name;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;
use std::sync::Arc;
use syntax::ast::{ClassDecl, Stmt, StmtKind, Visibility};
use syntax::name_resolution::NameContext;
use syntax::token::Span;

/// Project-wide merge of every analysed file's `NamedArgumentCallFacts`,
/// each call site tagged with the file it came from.
pub(crate) type NamedArgumentCallFactsWithPath =
    FxHashMap<(Arc<str>, Arc<str>, Arc<str>), Vec<(PathBuf, Span)>>;

/// Merges each analysed file's call sites in the order given, which the
/// caller makes path order.
pub(crate) fn merge_named_argument_calls(
    per_file: Vec<(PathBuf, NamedArgumentCallFacts)>,
) -> NamedArgumentCallFactsWithPath {
    let mut merged: NamedArgumentCallFactsWithPath = FxHashMap::default();
    for (path, calls) in per_file {
        for (key, spans) in calls {
            merged
                .entry(key)
                .or_default()
                .extend(spans.into_iter().map(|span| (path.clone(), span)));
        }
    }
    merged
}

/// An override that renames a parameter relative to its nearest ancestor
/// prototype (`Registry::find_method_signature_prototype`), at the same
/// positional slot.
#[derive(Debug, Clone)]
pub(crate) struct RenamedParameterOverride {
    pub(crate) prototype_class: Arc<str>,
    pub(crate) method: Arc<str>,
    pub(crate) prototype_param: Arc<str>,
    pub(crate) overriding_class: Arc<str>,
    pub(crate) overriding_param: Arc<str>,
}

pub(crate) type RenamedParameterOverrideFacts = Vec<RenamedParameterOverride>;

/// Every renamed-parameter override declared by a top-level class in
/// `programs`: each class's own methods in source order, then the
/// methods it takes from used traits (sorted, skipping `__construct`,
/// its own and abstract ones). Names resolve against the `namespace`
/// and `use` statements seen so far in the file.
pub(crate) fn renamed_parameter_overrides<'p>(
    registry: &Registry,
    programs: impl IntoIterator<Item = &'p [Stmt]>,
) -> RenamedParameterOverrideFacts {
    let mut overrides = Vec::new();
    for program in programs {
        let mut name_ctx = NameContext::default();
        for stmt in program {
            match &stmt.kind {
                StmtKind::Namespace(name) => name_ctx.set_namespace(name),
                StmtKind::Use(imports) => name_ctx.add_uses(imports),
                StmtKind::ClassDecl(c) => {
                    let fqn = name_ctx.declared_name(class_decl_name(c));
                    class_overrides(registry, &fqn, c, &mut overrides);
                }
                _ => {}
            }
        }
    }
    overrides
}

fn class_overrides(
    registry: &Registry,
    fqn: &str,
    c: &ClassDecl,
    out: &mut RenamedParameterOverrideFacts,
) {
    for m in &c.methods {
        method_overrides(registry, fqn, &m.function.name, out);
    }
    if c.uses.is_empty() {
        return;
    }
    let Some(info) = registry.classes.get(fqn) else {
        return;
    };
    let own_method_names: FxHashSet<&str> =
        c.methods.iter().map(|m| m.function.name.as_ref()).collect();
    let mut trait_method_names: Vec<Arc<str>> = info
        .methods()
        .keys()
        .filter(|name| {
            name.as_ref() != "__construct"
                && !own_method_names.contains(name.as_ref())
                && !info.abstract_methods().contains(*name)
        })
        .cloned()
        .collect();
    trait_method_names.sort();
    for name in trait_method_names {
        method_overrides(registry, fqn, &name, out);
    }
}

fn method_overrides(
    registry: &Registry,
    fqn: &str,
    method_name: &str,
    out: &mut RenamedParameterOverrideFacts,
) {
    let Some(info) = registry.classes.get(fqn) else {
        return;
    };
    let Some(own_sig) = info.methods().get(method_name) else {
        return;
    };
    let Some((ancestor_owner, ancestor_sig, _)) =
        registry.find_method_signature_prototype(fqn, method_name)
    else {
        return;
    };
    if ancestor_sig.visibility() == Visibility::Private {
        return;
    }
    for (i, ancestor_param) in ancestor_sig.params().iter().enumerate() {
        let Some(own_param) = own_sig.params().get(i) else {
            break;
        };
        if ancestor_param.name() != own_param.name() {
            out.push(RenamedParameterOverride {
                prototype_class: ancestor_owner.clone(),
                method: syntax::intern::intern(method_name),
                prototype_param: ancestor_param.name().clone(),
                overriding_class: syntax::intern::intern(fqn),
                overriding_param: own_param.name().clone(),
            });
        }
    }
}

pub(crate) fn find_renamed_named_argument_collisions(
    calls: &NamedArgumentCallFactsWithPath,
    overrides: &RenamedParameterOverrideFacts,
) -> Vec<(PathBuf, Span, String)> {
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
        for (path, span) in sites {
            collisions.push((
                path.clone(),
                *span,
                format!(
                    "Call to {prototype_class}::{method}() uses named argument for parameter \
                     ${prototype_param}, but {overriding_class} renames it to ${overriding_param}."
                ),
            ));
        }
    }
    collisions
}
