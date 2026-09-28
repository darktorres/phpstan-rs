use rustc_hash::FxHashMap;

use crate::Type;

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
/// Which branch a conditional takes for a subject of type `bound`:
/// `Some(true)` the `if_true` one, `Some(false)` the other, `None` both.
/// Real PHPStan's `ConditionalType`/`ConditionalTypeForParameter` are
/// trinary: `target->isSuperTypeOf(subject)` yes picks one branch, no
/// the other, and maybe (a possibly-empty `array<T>` checked against
/// `non-empty-array<T>`) keeps both.
pub fn conditional_branch(bound: &Type, target: &Type, negated: bool) -> Option<bool> {
    let matches = match crate::Relation::structural().is_super_type_of(target, bound) {
        crate::TrinaryLogic::Yes => Some(true),
        crate::TrinaryLogic::No => Some(false),
        crate::TrinaryLogic::Maybe => None,
    };
    matches.map(|m| m != negated)
}

pub fn resolve_conditional(ty: &Type, param_bindings: &FxHashMap<String, Type>) -> Type {
    match ty {
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
            template_subject,
        } => match param_bindings.get(subject).filter(|_| !*template_subject) {
            Some(bound) if *bound != Type::Mixed(false) => {
                match conditional_branch(bound, target, *negated) {
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
        _ => ty.map_children(&mut |t| resolve_conditional(t, param_bindings)),
    }
}

/// Both branches of an undecided conditional, flattened and deduplicated
/// (`T` and `T|null` give `T|null`, not `T|T|null`), collapsing to the
/// single member when both branches agree.
pub fn union_of_branches(a: Type, b: Type) -> Type {
    let mut members: Vec<Type> = Vec::new();
    for ty in [a, b] {
        let parts = match ty {
            Type::Union(inner) => inner,
            other => vec![other],
        };
        for p in parts {
            if !members.iter().any(|m| crate::types_equal_for_union(m, &p)) {
                members.push(p);
            }
        }
    }
    match members.len() {
        1 => members.pop().expect("len checked"),
        _ => Type::Union(members),
    }
}
