//! What a declared type leaves unsaid: PHPStan's `MissingTypehintCheck`
//! walks.

use crate::{Repr, Type};

impl Type {
    /// The iterable types in this type whose value type is implicit
    /// `mixed`, each as `iterable type <display>`; `explicit_mixed` when
    /// the source spelled `mixed` out, which silences those findings.
    pub fn iterable_types_missing_value_type(&self, explicit_mixed: bool) -> Vec<String> {
        let mut out = Vec::new();
        collect_iterable_missing_value_types(self, explicit_mixed, &mut out);
        out
    }

    /// The classes this type names with no generic arguments, looking
    /// inside other classes' arguments, arrays, class-strings, unions,
    /// intersections and shapes, but not templates. Each once, in order.
    pub fn bare_object_class_names(&self) -> Vec<&crate::Name> {
        let mut out = Vec::new();
        collect_bare_object_class_names(self, &mut out);
        out
    }

    /// The `callable`s and `Closure`s in this type written with no
    /// signature, each as its display.
    pub fn callables_missing_signature(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_callables_with_missing_signature(self, &mut out);
        out
    }
}

/// A bare `iterable`'s own two-member shape (`types::parse_type_string`
/// always builds it as exactly `Union([Array(value), Object("Traversable",
/// [])])` - see that function's own doc comment) - `Some(value)` when
/// `members` is that exact shape, checked by content rather than
/// positionally in case something upstream ever reorders a `Union`'s own
/// members.
fn iterable_union_value_type(members: &[Type]) -> Option<&Type> {
    if members.len() != 2 {
        return None;
    }
    let array_inner = members.iter().find_map(|m| match m {
        Type(Repr::Array(_, inner)) => Some(inner.as_ref()),
        _ => None,
    })?;
    let has_bare_traversable = members.iter().any(
        |m| matches!(m, Type(Repr::Object(name, args)) if name == "Traversable" && args.is_empty()),
    );
    has_bare_traversable.then_some(array_inner)
}

/// Walks `ty` for every iterable-shaped member (`array`, the `iterable`
/// pseudo-type, `list`/`non-empty-array`/`non-empty-list`) whose own
/// element type is implicit `mixed` - each finding is that member's own
/// `Display`ed description (`"array"`, `"iterable"`, `"list"`, ...),
/// ready to drop straight into a `"... has no value type specified in
/// {desc}."` message: `iterable type ` plus the type's display, exactly
/// as real PHPStan's own descriptions read. Mirrors real
/// PHPStan's `MissingTypehintCheck::getIterableTypesWithMissingValueTypehint`,
/// recursing into everything (a class's own generic type arguments, a
/// union/intersection's own members, an array shape's own field types)
/// except a `@template` placeholder itself, matching that function's own
/// `TemplateType` early-return.
///
/// `explicit_mixed` suppresses a finding whose own element type is
/// `Mixed` when the source text actually spelled `mixed` out explicitly
/// (e.g. `array<string, mixed>`) - real PHPStan's own `$iterableValue
/// instanceof MixedType && !$iterableValue->isExplicitMixed()` guard.
/// Writing `mixed` explicitly is a common,
/// deliberate idiom for "I don't know/care about the element type, and
/// I don't want this rule to nag about it" - unlike a bare `array` (no
/// generic arguments at all), which real PHPStan still flags exactly as
/// before.
fn collect_iterable_missing_value_types(ty: &Type, explicit_mixed: bool, out: &mut Vec<String>) {
    match ty {
        Type(Repr::TemplateParam(_)) | Type(Repr::Accessory(_)) => {}
        Type(Repr::Array(_, inner)) => {
            if inner.as_ref().is_mixed() {
                if !explicit_mixed {
                    out.push(format!("iterable type {ty}"));
                }
            } else {
                collect_iterable_missing_value_types(inner, explicit_mixed, out);
            }
        }
        Type(Repr::Union(members)) => {
            if let Some(inner) = iterable_union_value_type(members) {
                if inner.is_mixed() {
                    if !explicit_mixed {
                        out.push("iterable type iterable".to_string());
                    }
                } else {
                    collect_iterable_missing_value_types(inner, explicit_mixed, out);
                }
                return;
            }
            for m in members {
                collect_iterable_missing_value_types(m, explicit_mixed, out);
            }
        }
        Type(Repr::Intersection(members)) => {
            // `list<V>`, `non-empty-array<V>`, `list<V>&non-empty-array`: the
            // value type is whatever any `Array` member specifies, so only
            // an intersection whose every array member is bare lacks one.
            let values: Vec<&Type> = members
                .iter()
                .filter_map(|m| match m {
                    Type(Repr::Array(_, inner)) => Some(inner.as_ref()),
                    _ => None,
                })
                .collect();
            if !values.is_empty() {
                let specified: Vec<&Type> =
                    values.iter().copied().filter(|v| !v.is_mixed()).collect();
                if specified.is_empty() {
                    if !explicit_mixed {
                        out.push(format!("iterable type {ty}"));
                    }
                } else {
                    for inner in specified {
                        collect_iterable_missing_value_types(inner, explicit_mixed, out);
                    }
                }
                return;
            }
            for m in members {
                collect_iterable_missing_value_types(m, explicit_mixed, out);
            }
        }
        Type(Repr::Object(_, args)) => {
            for a in args {
                collect_iterable_missing_value_types(a, explicit_mixed, out);
            }
        }
        Type(Repr::VarianceArg(_, inner)) => {
            collect_iterable_missing_value_types(inner, explicit_mixed, out)
        }
        Type(Repr::ClassString(inner)) => {
            collect_iterable_missing_value_types(inner, explicit_mixed, out)
        }
        Type(Repr::ArrayShape(fields)) => {
            for t in fields.types() {
                collect_iterable_missing_value_types(t, explicit_mixed, out);
            }
        }
        _ => {}
    }
}

/// Mirrors real PHPStan's `MissingTypehintCheck::
/// getCallablesWithMissingSignature`: every `callable`/`Closure` in `ty`
/// written with no `(params): return` signature. `Repr::Callable(None)`
/// is exactly a bare `callable` (a written signature parses to
/// `Repr::Callable(Some(sig))`, whose own parameter/return types are
/// still walked); a bare `Closure` resolves to a plain class reference
/// instead. Each finding is the type's own display text.
fn collect_callables_with_missing_signature(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type(Repr::Callable(None)) => out.push(ty.to_string()),
        Type(Repr::Callable(Some(sig))) => {
            for p in &sig.params {
                collect_callables_with_missing_signature(&p.ty, out);
            }
            collect_callables_with_missing_signature(&sig.return_type, out);
        }
        Type(Repr::Object(name, args)) => {
            if args.is_empty()
                && name
                    .trim_start_matches('\\')
                    .eq_ignore_ascii_case("Closure")
            {
                out.push("Closure".to_string());
            }
            for a in args {
                collect_callables_with_missing_signature(a, out);
            }
        }
        Type(Repr::Array(key, inner)) => {
            collect_callables_with_missing_signature(key, out);
            collect_callables_with_missing_signature(inner, out);
        }
        Type(Repr::ClassString(inner)) | Type(Repr::VarianceArg(_, inner)) => {
            collect_callables_with_missing_signature(inner, out)
        }
        // A `Closure(...)` shape (`Closure&callable(...)`) carries its
        // signature; only that signature's own types are walked.
        Type(Repr::Intersection(members)) if ty.callable_sig().is_some() => {
            for m in members {
                if matches!(m, Type(Repr::Callable(Some(_)))) {
                    collect_callables_with_missing_signature(m, out);
                }
            }
        }
        Type(Repr::Union(members)) | Type(Repr::Intersection(members)) => {
            for m in members {
                collect_callables_with_missing_signature(m, out);
            }
        }
        Type(Repr::ArrayShape(fields)) => {
            for t in fields.types() {
                collect_callables_with_missing_signature(t, out);
            }
        }
        _ => {}
    }
}

fn collect_bare_object_class_names<'a>(ty: &'a Type, out: &mut Vec<&'a crate::Name>) {
    match ty {
        Type(Repr::TemplateParam(_)) => {}
        Type(Repr::VarianceArg(_, inner)) | Type(Repr::ClassString(inner)) => {
            collect_bare_object_class_names(inner, out)
        }
        Type(Repr::Object(name, args)) => {
            if args.is_empty() {
                if !out.contains(&name) {
                    out.push(name);
                }
            } else {
                for a in args {
                    collect_bare_object_class_names(a, out);
                }
            }
        }
        Type(Repr::Array(key, inner)) => {
            collect_bare_object_class_names(key, out);
            collect_bare_object_class_names(inner, out);
        }
        Type(Repr::Union(members)) | Type(Repr::Intersection(members)) => {
            for m in members {
                collect_bare_object_class_names(m, out);
            }
        }
        Type(Repr::ArrayShape(fields)) => {
            for t in fields.types() {
                collect_bare_object_class_names(t, out);
            }
        }
        _ => {}
    }
}
