use std::sync::Arc;

use crate::types::{CallableParam, CallableSig, Type};
use syntax::intern::intern;
use syntax::name_resolution::NameContext;

/// Rewrites every `Object(name)` in a type tree to the fully-qualified
/// name `ctx` resolves it to - mirrors `mark_template_params`'s exact
/// shape (both are a one-time post-processing walk over an already-parsed
/// `Type`, applied right after `parse_type_string` produces it), applied
/// second since a name that turned out to be a `@template` parameter
/// isn't a real class reference to resolve at all.
pub(crate) fn resolve_names_in_type(ty: Type, ctx: &NameContext) -> Type {
    match ty {
        // `Traversable` is this project's own `iterable`-desugaring
        // synthetic, never a reference to resolve, but a real,
        // programmer-written `@return Traversable<Edge>`/`Iterator<K, V>`
        // tag is common (confirmed via a real corpus false positive:
        // `bacon/bacon-qr-code`'s own `@return Traversable<Edge>`, `Edge`
        // a real same-namespace class that never got resolved past its
        // bare spelling) - `Traversable` isn't only this project's own
        // internal `iterable`-desugaring synthetic, it's also real,
        // directly-written PHPDoc syntax, so its own type arguments still
        // need ordinary name resolution here, the same as `$$valueof`'s
        // wrapped argument just below - only the outer name itself must
        // stay untouched.
        Type::Object(name, args) if name == "Traversable" => Type::Object(
            name,
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        // `self<T>`/`static<T>` sentinels: only their arguments name
        // classes.
        Type::Object(name, args) if name == "$$self" || name == "$$static" => Type::Object(
            name,
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        // `"$$interface"`/`"$$trait"`
        // (`types::parse_type_keyword_or_class`'s own bare `interface-
        // string`/`trait-string` sentinels, always argument-less) are
        // internal markers `registry::is_assignable`'s `ClassString` arm
        // recognizes directly, not class references to resolve.
        Type::Object(name, args) if name == "$$interface" || name == "$$trait" => {
            Type::Object(name, args)
        }
        // `"$$valueof"` (`types::parse_type_keyword_or_class`'s own
        // `value-of<T>`-on-a-class/enum sentinel, `Type::Object("$$valueof",
        // [Type::Object(name, [])])`) is a marker, not a class reference
        // itself - resolved later by `Registry::resolve_deferred_value_of`
        // once real class/enum data exists - but unlike `object`/
        // `Traversable`/`resource` above, its own *argument* (the real
        // class/enum name it wraps) still needs ordinary name resolution
        // here, the same as any other `Object`'s type arguments - without
        // this special case, the generic arm just below would instead try
        // to resolve `"$$valueof"` itself as if it were a genuine (and
        // nonsensical) class reference.
        Type::Object(name, args) if matches!(name.as_str(), "$$valueof" | "$$classconst") => {
            Type::Object(
                name,
                args.into_iter()
                    .map(|a| resolve_names_in_type(a, ctx))
                    .collect(),
            )
        }
        Type::Object(name, args) => Type::Object(
            ctx.resolve(&name),
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        // A `Closure(...)` shape names PHP's own global `Closure`, never
        // a same-named class of the current namespace.
        Type::Intersection(members)
            if matches!(
                members.as_slice(),
                [Type::Object(name, args), Type::Callable(Some(_))]
                    if name == "Closure" && args.is_empty()
            ) =>
        {
            let mut members = members;
            let sig = members.pop().unwrap();
            members.push(resolve_names_in_type(sig, ctx));
            Type::Intersection(members)
        }
        // Mirrors `mark_template_params`/`substitute_type_aliases`'s own
        // equivalent arms - a class reference inside a `Closure(...)`
        // shape's own param/return types needs the same namespace
        // resolution as anywhere else.
        Type::Callable(Some(sig)) => Type::Callable(Some(std::sync::Arc::new(
            CallableSig {
                params: sig
                    .params
                    .iter()
                    .map(|p| CallableParam {
                        ty: resolve_names_in_type(p.ty.clone(), ctx),
                        variadic: p.variadic,
                        optional: p.optional,
                        by_ref: p.by_ref,
                    })
                    .collect(),
                return_type: Box::new(resolve_names_in_type((*sig.return_type).clone(), ctx)),
                templates: sig
                    .templates
                    .iter()
                    .map(|t| crate::types::TemplateParam {
                        bound: Box::new(resolve_names_in_type((*t.bound).clone(), ctx)),
                        default: t
                            .default
                            .as_ref()
                            .map(|d| Box::new(resolve_names_in_type((**d).clone(), ctx))),
                        ..t.clone()
                    })
                    .collect(),
            }
            .rebind_templates(),
        ))),
        // Every other nested position - a generic argument, a shape
        // field, a conditional return type's branches - needs the same
        // namespace resolution as anywhere else.
        other => other.map_children(&mut |t| resolve_names_in_type(t.clone(), ctx)),
    }
}

/// The read-only, `(raw, resolved)`-collecting sibling of
/// `resolve_names_in_type` just above - same
/// recursive shape, same sentinel skip-list, but pushes into `out`
/// instead of rewriting `ty` in place. `class.nameCase` for a PHPDoc-typed
/// position needs the pre-resolution *spelling* (`ctx.is_use_imported`'s
/// own input) alongside the resolved FQN, which `resolve_names_in_type`
/// itself destroys the moment it overwrites `Object`'s own name field -
/// this is a parallel traversal run one step earlier (right after
/// `mark_template_params`, right before `resolve_names_in_type`) rather
/// than a new field threaded through `Type` itself (`Type` derives
/// `PartialEq`, used for union dedup/narrowing/subtype tests throughout;
/// a raw-text field would need a hand-written exclusion, and even then
/// wouldn't carry the pre-resolution string this specifically needs).
///
/// Must be kept in sync with `resolve_names_in_type`'s own arms by hand -
/// a case missed in one silently misses a nested reference the other
/// still resolves (or, conversely, collects a case-ref for a sentinel
/// `resolve_names_in_type` itself never treats as a real class
/// reference).
pub(crate) fn collect_class_name_case_refs(
    ty: &Type,
    ctx: &NameContext,
    out: &mut Vec<(Arc<str>, Arc<str>)>,
) {
    match ty {
        // Mirrors `resolve_names_in_type`'s own sentinel arms exactly:
        // `$$interface`/`$$trait` are never a real class reference to
        // check the case of, and (unlike the `$$valueof` sentinels just
        // below) their own argument list isn't recursed into there
        // either, so this doesn't either - both are always
        // argument-less in practice regardless.
        Type::Object(name, _)
            if matches!(
                name.as_str(),
                "$$interface" | "$$trait" | "$$self" | "$$static"
            ) => {}
        Type::Object(name, args)
            if matches!(name.as_str(), "$$valueof" | "$$classconst" | "Traversable") =>
        {
            for a in args {
                collect_class_name_case_refs(a, ctx, out);
            }
        }
        Type::Object(name, args) => {
            out.push((intern(name), intern(&ctx.resolve(name))));
            for a in args {
                collect_class_name_case_refs(a, ctx, out);
            }
        }
        _ => ty.for_each_child(&mut |t| collect_class_name_case_refs(t, ctx, out)),
    }
}
