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

    /// The classes this type names with no generic arguments, at any
    /// depth but inside a template. Each once, in order.
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

/// The value type of `iterable`'s two-member shape (`Union([Array(value),
/// Object("Traversable", [] or [key, value])])`, as `types::parse_type_string`
/// builds it) when `members` is that shape, checked by content rather than
/// position.
fn iterable_union_value_type(members: &[Type]) -> Option<&Type> {
    if members.len() != 2 {
        return None;
    }
    let array_inner = members.iter().find_map(|m| match m.repr() {
        Repr::Array(_, inner) => Some(inner.as_ref()),
        _ => None,
    })?;
    let has_traversable = members.iter().any(|m| {
        matches!(m.repr(), Repr::Object(name, _) if name.trim_start_matches('\\') == "Traversable")
    });
    has_traversable.then_some(array_inner)
}

/// Walks `ty` for every iterable-shaped member (`array`, the `iterable`
/// pseudo-type, `list`/`non-empty-array`/`non-empty-list`) whose own
/// element type is implicit `mixed` - each finding is that member's own
/// `Display`ed description (`"array"`, `"iterable"`, `"list"`, ...),
/// ready to drop straight into a `"... has no value type specified in
/// {desc}."` message: `iterable type ` plus the type's display, exactly
/// as real PHPStan's own descriptions read. Mirrors real
/// PHPStan's `MissingTypehintCheck::getIterableTypesWithMissingValueTypehint`,
/// recursing into every nested type (generic arguments, union and
/// intersection members, shape fields, callable signatures) except a
/// `@template` placeholder itself, matching that function's own
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
    syntax::stack::guard(|| {
        let mut missing = |desc: String| {
            if !explicit_mixed {
                out.push(desc);
            }
        };
        match ty.repr() {
            Repr::TemplateParam(_) => return,
            Repr::Array(_, inner) if inner.is_mixed() => {
                return missing(format!("iterable type {ty}"));
            }
            Repr::Union(members) => {
                if let Some(inner) = iterable_union_value_type(members) {
                    if inner.is_mixed() {
                        missing("iterable type iterable".to_string());
                    } else {
                        collect_iterable_missing_value_types(inner, explicit_mixed, out);
                    }
                    return;
                }
            }
            // `list<V>`, `non-empty-array<V>`, `list<V>&non-empty-array`: the
            // value type is whatever any `Array` member specifies, so only
            // an intersection whose every array member is bare lacks one.
            Repr::Intersection(members)
                if members.iter().any(|m| matches!(m.repr(), Repr::Array(..))) =>
            {
                let specified: Vec<&Type> = members
                    .iter()
                    .filter_map(|m| match m.repr() {
                        Repr::Array(_, inner) if !inner.is_mixed() => Some(inner.as_ref()),
                        _ => None,
                    })
                    .collect();
                if specified.is_empty() {
                    return missing(format!("iterable type {ty}"));
                }
                for inner in specified {
                    collect_iterable_missing_value_types(inner, explicit_mixed, out);
                }
                return;
            }
            _ => {}
        }
        ty.for_each_child(&mut |child| {
            collect_iterable_missing_value_types(child, explicit_mixed, out)
        });
    })
}

/// Mirrors real PHPStan's `MissingTypehintCheck::
/// getCallablesWithMissingSignature`: every `callable`/`Closure` in `ty`
/// written with no `(params): return` signature, at any depth - a
/// written signature's own parameter and return types included. A bare
/// `callable` is `Repr::Callable(None)`; a bare `Closure` is a plain
/// class reference. Each finding is the type's own display text.
fn collect_callables_with_missing_signature(ty: &Type, out: &mut Vec<String>) {
    syntax::stack::guard(|| {
        match ty.repr() {
            Repr::TemplateParam(_) => return,
            Repr::Callable(None) => return out.push(ty.to_string()),
            Repr::Object(name, args)
                if args.is_empty()
                    && name
                        .trim_start_matches('\\')
                        .eq_ignore_ascii_case("Closure") =>
            {
                return out.push("Closure".to_string());
            }
            // A `Closure(...)` shape (`Closure&callable(...)`) carries its
            // signature; only that signature's own types are walked.
            Repr::Intersection(members) if ty.callable_sig().is_some() => {
                for m in members {
                    if matches!(m.repr(), Repr::Callable(Some(_))) {
                        collect_callables_with_missing_signature(m, out);
                    }
                }
                return;
            }
            _ => {}
        }
        ty.for_each_child(&mut |child| collect_callables_with_missing_signature(child, out));
    })
}

/// Every class `ty` names with no generic arguments, at any depth but
/// inside a template.
fn collect_bare_object_class_names<'a>(ty: &'a Type, out: &mut Vec<&'a crate::Name>) {
    syntax::stack::guard(|| match ty.repr() {
        Repr::TemplateParam(_) => {}
        Repr::Object(name, args) if args.is_empty() => {
            if !out.contains(&name) {
                out.push(name);
            }
        }
        _ => ty.for_each_child(&mut |child| collect_bare_object_class_names(child, out)),
    })
}
