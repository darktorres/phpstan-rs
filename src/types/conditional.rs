use std::collections::HashMap;
use std::sync::Arc;

use crate::types::{CallableParam, CallableSig, Type};

/// Resolves every `Type::Conditional` node in `ty` against `param_bindings`
/// (parameter *name* -> the actual, already-inferred type bound to it at
/// this call site - a genuinely different keyspace from `TemplateBindings`,
/// which is keyed by template *name*, to avoid a real if rare name
/// collision between the two). A genuinely separate mechanism from
/// `substitute_template` (not folded into it): that function resolves
/// `TemplateParam` nodes against a template-name-keyed map, this one
/// resolves `Conditional` nodes against a parameter-name-keyed one - the
/// two node kinds carry unrelated information and can appear nested inside
/// each other (a conditional's own branch mentioning a template, or vice
/// versa), so this recurses through the identical structural wrappers
/// `substitute_template` already covers (`Array`/`Union`/`ClassString`/
/// `Intersection`/`ArrayShape`/`ObjectShape`/`Object`/`Callable`) to reach
/// every nested `Conditional`, not just a top-level one.
///
/// For a `Conditional` node: `param_bindings.get(subject)` - unbound, or
/// bound to `Mixed` (the "genuinely unknown argument type" sentinel this
/// project uses throughout) - falls back to the safe two-branch union
/// (also, itself, recursively resolved - a real, if rarer, nested-
/// conditional shape). A real bound resolves trinarily: a subtype of
/// `target` picks one branch, a provably disjoint one the other, and
/// anything else keeps both (see the arm's own comment).
pub fn resolve_conditional(ty: &Type, param_bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        } => match param_bindings.get(subject) {
            Some(bound) if *bound != Type::Mixed(false) => {
                // Real PHPStan's `ConditionalTypeForParameter::resolve` is
                // trinary: `target->isSuperTypeOf(subject)` yes picks one
                // branch, no picks the other, and maybe (a possibly-empty
                // `array<T>` checked against `non-empty-array<T>`) keeps
                // both. "No" here is `types_definitely_disjoint`'s
                // conservative answer; anything it can't prove disjoint
                // stays "maybe" rather than guessing a branch.
                let matches = if crate::types::is_subtype(bound, target) {
                    Some(true)
                } else if crate::types::types_definitely_disjoint(bound, target) {
                    Some(false)
                } else {
                    None
                };
                match matches.map(|m| m != *negated) {
                    Some(true) => resolve_conditional(if_true, param_bindings),
                    Some(false) => resolve_conditional(if_false, param_bindings),
                    None => union_of_branches(
                        resolve_conditional(if_true, param_bindings),
                        resolve_conditional(if_false, param_bindings),
                    ),
                }
            }
            _ => union_of_branches(
                resolve_conditional(if_true, param_bindings),
                resolve_conditional(if_false, param_bindings),
            ),
        },
        Type::Array(key, inner) => Type::Array(
            Box::new(resolve_conditional(key, param_bindings)),
            Box::new(resolve_conditional(inner, param_bindings)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(resolve_conditional(inner, param_bindings)))
        }
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| resolve_conditional(m, param_bindings))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| resolve_conditional(m, param_bindings))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), resolve_conditional(t, param_bindings), *opt))
                .collect(),
        ),
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), resolve_conditional(t, param_bindings), *opt))
                .collect(),
        ),
        Type::Object(name, args) => Type::Object(
            name.clone(),
            args.iter()
                .map(|a| resolve_conditional(a, param_bindings))
                .collect(),
        ),
        Type::Callable(Some(sig)) => Type::Callable(Some(Arc::new(CallableSig {
            params: sig
                .params
                .iter()
                .map(|p| CallableParam {
                    ty: resolve_conditional(&p.ty, param_bindings),
                    variadic: p.variadic,
                    optional: p.optional,
                    by_ref: p.by_ref,
                })
                .collect(),
            return_type: Box::new(resolve_conditional(&sig.return_type, param_bindings)),
        }))),
        other => other.clone(),
    }
}

/// Both branches of an undecided conditional, flattened and deduplicated
/// (`T` and `T|null` give `T|null`, not `T|T|null`), collapsing to the
/// single member when both branches agree.
fn union_of_branches(a: Type, b: Type) -> Type {
    let mut members: Vec<Type> = Vec::new();
    for ty in [a, b] {
        let parts = match ty {
            Type::Union(inner) => inner,
            other => vec![other],
        };
        for p in parts {
            if !members
                .iter()
                .any(|m| crate::types::types_equal_for_union(m, &p))
            {
                members.push(p);
            }
        }
    }
    match members.len() {
        1 => members.pop().expect("len checked"),
        _ => Type::Union(members),
    }
}
