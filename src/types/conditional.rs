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
/// conditional shape). A real bound picks `if_true`/`if_false` via
/// `is_subtype(bound, target) != negated`, then recurses into *only* the
/// picked branch (never both) - the whole point of this mechanism over the
/// pre-Phase-80 eager-union behavior still preserved by `types::parse`'s
/// own fallback for an unrepresentable subject.
pub fn resolve_conditional(ty: &Type, param_bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        } => match param_bindings.get(subject) {
            Some(bound) if *bound != Type::Mixed => {
                let picked = if crate::types::is_subtype(bound, target) != *negated {
                    if_true
                } else {
                    if_false
                };
                resolve_conditional(picked, param_bindings)
            }
            _ => Type::Union(vec![
                resolve_conditional(if_true, param_bindings),
                resolve_conditional(if_false, param_bindings),
            ]),
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
