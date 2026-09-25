//! Late-resolvable types: `key-of<T>`, `value-of<T>` and `T[K]`, whose
//! answer depends on a `T` that may be a template until a call binds it
//! (PHPStan's `LateResolvableType`).

use crate::types::{Type, parse::canonical_int_key};

/// `key-of<ty>` when `ty` is an array type: its key type. `None` for
/// anything whose keys aren't known from the type alone.
pub(crate) fn key_of(ty: &Type) -> Option<Type> {
    match ty {
        Type::Mixed(_) => Some(Type::Mixed(false)),
        Type::Array(key, _) => Some((**key).clone()),
        Type::ArrayShape(fields) => Some(join(
            fields
                .iter()
                .filter(|(name, _, _)| name != "...")
                .map(|(name, _, _)| shape_key_literal(name))
                .collect(),
        )),
        Type::Intersection(members) => members.iter().find_map(key_of),
        Type::Union(members) => members.iter().map(key_of).collect::<Option<_>>().map(join),
        _ => None,
    }
}

/// `value-of<ty>` when `ty` is an array type: its value type. An enum's
/// case values need the registry (`Registry::resolve_deferred_value_of`).
pub(crate) fn value_of(ty: &Type) -> Option<Type> {
    match ty {
        Type::Mixed(_) => Some(Type::Mixed(false)),
        Type::Array(_, value) => Some((**value).clone()),
        Type::ArrayShape(fields) => Some(join(
            fields
                .iter()
                .filter(|(name, _, _)| name != "...")
                .map(|(_, ty, _)| ty.clone())
                .collect(),
        )),
        Type::Intersection(members) => members.iter().find_map(value_of),
        Type::Union(members) => members
            .iter()
            .map(value_of)
            .collect::<Option<_>>()
            .map(join),
        _ => None,
    }
}

/// `base[key]` (PHPStan's `OffsetAccessType::getResult()`): the value
/// type `base` holds at `key`. A shape without the key, or a non-array
/// `base`, gives `Mixed`; `None` when `base` isn't known yet.
pub(crate) fn offset_access(base: &Type, key: &Type) -> Option<Type> {
    match base {
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => Some(
            fields
                .iter()
                .find(|(name, _, _)| shape_key_literal(name) == *key)
                .map(|(_, ty, _)| ty.clone())
                .unwrap_or(Type::Mixed(false)),
        ),
        Type::Array(_, value) => Some((**value).clone()),
        Type::Intersection(members) => members.iter().find_map(|m| offset_access(m, key)),
        Type::Union(members) => members
            .iter()
            .map(|m| offset_access(m, key))
            .collect::<Option<_>>()
            .map(join),
        Type::TemplateParam(_) | Type::Object(_, _) => None,
        _ => Some(Type::Mixed(false)),
    }
}

/// A shape field name as the key type it is: an integer-looking name is
/// an int key, matching PHP's own key canonicalization.
pub(crate) fn shape_key_literal(name: &str) -> Type {
    match canonical_int_key(name) {
        Some(n) => Type::IntLiteral(n),
        None => Type::StringLiteral(name.to_string()),
    }
}

/// A late-resolvable node answered now that its operand may be known -
/// what a substitution applies to each node it rebuilds. A `key-of`/offset
/// access whose operand no longer holds a template is answered, `Mixed`
/// when the operand isn't an array type; a `value-of` is answered only
/// over an array type (an enum's needs the registry).
pub(crate) fn settle(ty: Type) -> Type {
    match &ty {
        Type::KeyOf(inner) | Type::OffsetAccess(inner, _) if !mentions_template(inner) => {
            resolve_node(&ty, &|t| t.clone()).unwrap_or(Type::Mixed(false))
        }
        Type::ValueOf(_) => resolve_node(&ty, &|t| t.clone()).unwrap_or(ty),
        _ => ty,
    }
}

fn mentions_template(ty: &Type) -> bool {
    matches!(ty, Type::TemplateParam(_)) || ty.any_child(mentions_template)
}

/// `ty` with every late-resolvable node answered as far as its
/// templates' bounds allow - what a template-typed value can be relied
/// on to be inside the generic code itself. An answer the bound doesn't
/// give is `Mixed`.
pub(crate) fn resolve_late_to_bounds(ty: &Type) -> Type {
    let ty = ty.map_children(&mut resolve_late_to_bounds);
    match ty {
        Type::KeyOf(_) | Type::OffsetAccess(..) => {
            resolve_node(&ty, &upper_bound).unwrap_or(Type::Mixed(false))
        }
        Type::ValueOf(ref inner) if matches!(**inner, Type::TemplateParam(_)) => {
            resolve_node(&ty, &upper_bound).unwrap_or(Type::Mixed(false))
        }
        other => other,
    }
}

/// Whether `ty` holds a `KeyOf`/`OffsetAccess` anywhere.
pub(crate) fn contains_late(ty: &Type) -> bool {
    matches!(ty, Type::KeyOf(_) | Type::OffsetAccess(..)) || ty.any_child(contains_late)
}

/// A template's bound, recursively (`T of U`, `U of array`).
fn upper_bound(ty: &Type) -> Type {
    match ty {
        Type::TemplateParam(tp) => upper_bound(&tp.bound),
        other => other.clone(),
    }
}

/// One late-resolvable node's answer, reading its operand through
/// `operand`; `None` when it isn't late-resolvable or can't be answered
/// yet.
fn resolve_node(ty: &Type, operand: &dyn Fn(&Type) -> Type) -> Option<Type> {
    match ty {
        Type::KeyOf(inner) => key_of(&operand(inner)),
        Type::ValueOf(inner) => value_of(&operand(inner)),
        Type::OffsetAccess(base, key) => offset_access(&operand(base), key),
        _ => None,
    }
}

/// The union of `members`, flattened and deduplicated; `Never` for none.
fn join(members: Vec<Type>) -> Type {
    let mut out: Vec<Type> = Vec::new();
    for m in members {
        let parts = match m {
            Type::Union(inner) => inner,
            other => vec![other],
        };
        for p in parts {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    match out.len() {
        0 => Type::Never,
        1 => out.pop().expect("len checked"),
        _ => Type::Union(out),
    }
}
