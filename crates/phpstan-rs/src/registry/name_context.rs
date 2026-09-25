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
        // `object` (the native type-hint keyword, not a real class) is
        // represented as `Type::Object("object")` - `parse_type_string`'s
        // own sentinel for it (see its `"object" => Type::Object(...)`
        // arm) - and is never a genuine reference to resolve: `object` is
        // a PHP-reserved word, so no real class can ever be named that.
        // Without this, a plain `object` type hint got namespace-qualified
        // into nonsense like `App\Some\Namespace\object`.
        Type::Object(name, args) if name == "object" => Type::Object(name, args),
        // Same reasoning as `object` above for the *name* itself, but
        // unlike `object`/`resource` (always argument-less in practice -
        // real PHP forbids generics on either native keyword), a real,
        // programmer-written `@return Traversable<Edge>`/`Iterator<K, V>`
        // tag is common (confirmed via a real corpus false positive:
        // `bacon/bacon-qr-code`'s own `@return Traversable<Edge>`, `Edge`
        // a real same-namespace class that never got resolved past its
        // bare spelling) - `Traversable` isn't only this project's own
        // internal `iterable`-desugaring synthetic, it's also real,
        // directly-written PHPDoc syntax, so its own type arguments still
        // need ordinary name resolution here, the same as `$$valueof`'s
        // wrapped argument just below - only the outer name itself must
        // stay untouched, for the reason already given.
        Type::Object(name, args) if name == "Traversable" => Type::Object(
            name,
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        // Same reasoning as `object`/`Traversable` above: `resource` is
        // a sentinel for a real PHP value kind with no actual class
        // behind it (`types::parse_type_keyword_or_class`'s own
        // `"resource" => Type::Object("resource", ...)` arm), not a
        // class reference to resolve - without this, the same pseudo-
        // type used in two different namespaces was treated as two
        // unrelated "classes" (`App\resource` vs. plain `resource`).
        Type::Object(name, args) if name == "resource" || name == "closed-resource" => {
            Type::Object(name, args)
        }
        // `self<T>`/`static<T>` sentinels: only their arguments name
        // classes.
        Type::Object(name, args) if name == "$$self" || name == "$$static" => Type::Object(
            name,
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        // Same reasoning again: `"$$interface"`/`"$$trait"`
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
        //
        // `"$$covariant"`/`"$$contravariant"`/`"$$bivariant"`
        // (`types::parse_generic_arg`'s own call-site variance sentinels)
        // need the exact same treatment, for the exact same reason: a
        // fixture inside any `namespace` block would otherwise mangle
        // e.g. `"$$covariant"` into `App\$$covariant` before
        // `Registry::effective_slot_variance` ever gets a chance to
        // recognize it.
        Type::Object(name, args)
            if matches!(
                name.as_str(),
                "$$valueof" | "$$classconst" | "$$covariant" | "$$contravariant" | "$$bivariant"
            ) =>
        {
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
        Type::Array(key, inner) => Type::Array(
            Box::new(resolve_names_in_type(*key, ctx)),
            Box::new(resolve_names_in_type(*inner, ctx)),
        ),
        Type::ClassString(inner) => Type::ClassString(Box::new(resolve_names_in_type(*inner, ctx))),
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| resolve_names_in_type(m, ctx))
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
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| resolve_names_in_type(m, ctx))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_names_in_type(t, ctx), opt))
                .collect(),
        ),
        // `object{...}`'s own field types need resolving too - a genuine
        // class reference nested inside one
        // (`object{repo: SomeRepository}`) must not stay unqualified
        // inside a namespaced file.
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_names_in_type(t, ctx), opt))
                .collect(),
        ),
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
        // Mirrors `mark_template_params`/`substitute_type_aliases`'s own
        // identical arms - a real class reference inside one of a
        // conditional return type's own branches (e.g. `Connection` in
        // `@phpstan-return (... ? T : Connection)`) needs the same
        // namespace resolution as anywhere else.
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
            template_subject,
        } => Type::Conditional {
            subject,
            target: Box::new(resolve_names_in_type(*target, ctx)),
            if_true: Box::new(resolve_names_in_type(*if_true, ctx)),
            if_false: Box::new(resolve_names_in_type(*if_false, ctx)),
            negated,
            template_subject,
        },
        other => other,
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
        // `object`/`resource`/`$$interface`/`$$trait` are never a real
        // class reference to check the case of, and (unlike the
        // `$$valueof`/variance sentinels just below) their own argument
        // list isn't recursed into there either, so this doesn't either -
        // both are always argument-less in practice regardless.
        Type::Object(name, _)
            if matches!(
                name.as_str(),
                "object"
                    | "resource"
                    | "closed-resource"
                    | "$$interface"
                    | "$$trait"
                    | "$$self"
                    | "$$static"
            ) => {}
        Type::Object(name, args)
            if matches!(
                name.as_str(),
                "$$valueof"
                    | "$$classconst"
                    | "$$covariant"
                    | "$$contravariant"
                    | "$$bivariant"
                    | "Traversable"
            ) =>
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
        Type::Array(key, inner) => {
            collect_class_name_case_refs(key, ctx, out);
            collect_class_name_case_refs(inner, ctx, out);
        }
        Type::ClassString(inner) => collect_class_name_case_refs(inner, ctx, out),
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                collect_class_name_case_refs(m, ctx, out);
            }
        }
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => {
            for (_, t, _) in fields {
                collect_class_name_case_refs(t, ctx, out);
            }
        }
        Type::Callable(Some(sig)) => {
            for p in &sig.params {
                collect_class_name_case_refs(&p.ty, ctx, out);
            }
            collect_class_name_case_refs(&sig.return_type, ctx, out);
        }
        Type::Conditional {
            target,
            if_true,
            if_false,
            ..
        } => {
            collect_class_name_case_refs(target, ctx, out);
            collect_class_name_case_refs(if_true, ctx, out);
            collect_class_name_case_refs(if_false, ctx, out);
        }
        _ => {}
    }
}
