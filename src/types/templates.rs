use std::collections::HashMap;
use std::sync::Arc;

use crate::types::{CallableParam, CallableSig, Type};

/// Unifies a template-bearing param type against an actual argument type,
/// recording the binding it implies. Only the two shapes that come up in
/// practice are handled: a bare `T` param, and `T[]`/`array<T>` unifying
/// against an array argument's element type. Anything else (a template
/// buried in a union, an array shape, etc) just isn't bound, which is safe
/// since `substitute_template` falls back to `Mixed` for anything left
/// unbound. First binding for a given name wins; conflicting uses of the
/// same `T` across multiple params aren't reconciled (no union-widening).
pub fn bind_template_param(param_ty: &Type, arg_ty: &Type, bindings: &mut HashMap<String, Type>) {
    match param_ty {
        Type::TemplateParam(tp) => {
            // Phase 54/77 follow-up: reject a bare `T` binding to a
            // *definite scalar* when `T`'s own declared bound requires an
            // object (`@template T of object`) - real corpus find (`new
            // ReflectionClass($className)`, a plain `string`-typed
            // variable, not a `Foo::class`/object-instance `T`-carrying
            // argument): without this, the bare-`T` union member of
            // `class-string<T>|T` happily bound `T` to the raw `string`
            // itself once the `ClassString` member's own, stricter arm
            // declined to match - producing a bound-violating
            // `ReflectionClass<string>` that then broke real downstream
            // `ReflectionClass<object>` call sites. Deliberately narrower
            // than a full `is_subtype`/bound check: this module has no
            // registry access, so it can't tell a real subclass (`T of
            // Base` satisfied by a `Derived` argument, the common case)
            // from an unrelated class - only a *scalar-vs-object* mismatch
            // is structurally unambiguous without one (confirmed the hard
            // way: a real `is_subtype` version broke six existing tests,
            // every one a legitimate `T of SomeClass` bound satisfied by an
            // actual subclass argument `is_subtype`'s own registry-free,
            // purely-structural comparison can't see as related at all).
            let bound_requires_object =
                matches!(&*tp.bound, Type::Object(name, _) if name == "object");
            let arg_is_definite_scalar = matches!(
                arg_ty,
                Type::String
                    | Type::StringLiteral(_)
                    | Type::Int
                    | Type::IntLiteral(_)
                    | Type::IntRange(_, _)
                    | Type::Float
                    | Type::FloatLiteral(_)
                    | Type::Bool
                    | Type::True
                    | Type::False
                    | Type::Null
            );
            if *arg_ty != Type::Mixed && !(bound_requires_object && arg_is_definite_scalar) {
                bindings
                    .entry(tp.name.clone())
                    .or_insert_with(|| arg_ty.clone());
            }
        }
        Type::Array(_, inner) => {
            if let Type::Array(_, arg_inner) = arg_ty {
                bind_template_param(inner, arg_inner, bindings);
            }
        }
        Type::ClassString(inner) => {
            if let Type::ClassString(arg_inner) = arg_ty {
                bind_template_param(inner, arg_inner, bindings);
            }
        }
        // Phase 77: the missing arm blocking phpstorm-stubs' pervasive
        // `@param class-string<T>|null $name` idiom (and any other
        // nullable/union-wrapped templated parameter) - a real, already-
        // latent bug confirmed via an isolated repro (removing only the
        // `|null` let an otherwise-identical chain correctly flag a type
        // mismatch). Each member is tried against the *same*, whole
        // `arg_ty` (not a decomposed one) - a member that isn't itself
        // `TemplateParam`/`Array`/`ClassString`/`Union` (e.g. the `Null`
        // half of `class-string<T>|null`) is simply a no-op here, exactly
        // as it already is at the top level for any other type that
        // isn't one of those shapes.
        Type::Union(members) => {
            for m in members {
                bind_template_param(m, arg_ty, bindings);
            }
        }
        _ => {}
    }
}

/// Applies a set of template bindings through a type tree, replacing every
/// `TemplateParam` with its bound type. Phase 54: a name left unbound now
/// falls back to that template's own declared `@template T = Default`
/// when it has one, rather than unconditionally `Mixed` - e.g. a
/// function-own `@template T = int` whose `T` never actually gets bound
/// from any real call-site argument (used only in the return type, say)
/// now resolves to `int` instead of `Mixed`. `checker::generics::
/// substitute_scoped` mirrors this same fallback for the separate
/// class-level substitution path (an "erased" - too-few-arguments, or
/// bare - generic class reference).
pub fn substitute_template(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TemplateParam(tp) => bindings.get(&tp.name).cloned().unwrap_or_else(|| {
            tp.default
                .as_ref()
                .map(|d| substitute_template(d, bindings))
                .unwrap_or(Type::Mixed)
        }),
        // Phase 50: substitutes through the key type too (not just reset
        // to "unknown" every time) - a real, if rare, `array<TKey,
        // TValue>` bound key shouldn't be lost on every substitution
        // pass.
        Type::Array(key, inner) => Type::Array(
            Box::new(substitute_template(key, bindings)),
            Box::new(substitute_template(inner, bindings)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(substitute_template(inner, bindings)))
        }
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| substitute_template(m, bindings))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| substitute_template(m, bindings))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_template(t, bindings), *opt))
                .collect(),
        ),
        // A real, previously-missing case (found the same way as the
        // `Object` fix just below - this session's own corpus diff):
        // `object{...}`'s own field types were never substituted either.
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .iter()
                .map(|(k, t, opt)| (k.clone(), substitute_template(t, bindings), *opt))
                .collect(),
        ),
        // Phase 54: a real, previously-missing case - every other
        // structural wrapper above recurses into its own nested type(s);
        // `Object`'s own generic argument list didn't, so substituting
        // `T -> int` through e.g. `Box<T>` silently left it as `Box<T>`
        // completely unchanged instead of `Box<int>`. Also what carries
        // a call-site variance sentinel's own wrapped inner type through
        // substitution correctly (`"$$covariant"`/`"$$contravariant"`/
        // `"$$bivariant"` are all just `Object`s with this same shape -
        // no separate arm needed for them).
        // Phase 54: a real, previously-missing case - every other
        // structural wrapper above recurses into its own nested type(s);
        // `Object`'s own generic argument list didn't, so substituting
        // `T -> int` through e.g. `Box<T>` silently left it as `Box<T>`
        // completely unchanged instead of `Box<int>`. Also what carries
        // a call-site variance sentinel's own wrapped inner type through
        // substitution correctly (`"$$covariant"`/`"$$contravariant"`/
        // `"$$bivariant"` are all just `Object`s with this same shape -
        // no separate arm needed for them).
        Type::Object(name, args) => Type::Object(
            name.clone(),
            args.iter()
                .map(|a| substitute_template(a, bindings))
                .collect(),
        ),
        // Phase 53: mirrors the other three walkers' own equivalent arms -
        // a template param inside a `Closure(...)` shape's own param/return
        // types (e.g. `Closure(T): U`) needs the same substitution as
        // anywhere else.
        Type::Callable(Some(sig)) => Type::Callable(Some(Arc::new(CallableSig {
            params: sig
                .params
                .iter()
                .map(|p| CallableParam {
                    ty: substitute_template(&p.ty, bindings),
                    variadic: p.variadic,
                    optional: p.optional,
                    by_ref: p.by_ref,
                })
                .collect(),
            return_type: Box::new(substitute_template(&sig.return_type, bindings)),
        }))),
        // Phase 80 follow-up: recurses into `target`/`if_true`/`if_false` -
        // `subject` is a raw parameter name, not a `Type`, so there's
        // nothing to substitute there. Without this arm, a template
        // param used inside one of a conditional return type's own
        // branches (`@return ($x is T ? T : null)`, a real, if narrower,
        // shape) would silently keep reading as the bare, unsubstituted
        // template instead of the caller's actual bound type.
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        } => Type::Conditional {
            subject: subject.clone(),
            target: Box::new(substitute_template(target, bindings)),
            if_true: Box::new(substitute_template(if_true, bindings)),
            if_false: Box::new(substitute_template(if_false, bindings)),
            negated: *negated,
        },
        other => other.clone(),
    }
}
