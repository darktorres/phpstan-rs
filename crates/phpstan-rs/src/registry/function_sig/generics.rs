use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::docblock::DocBlock;
use crate::intern::intern;
use crate::name_resolution::NameContext;
use crate::registry::name_context::{collect_class_name_case_refs, resolve_names_in_type};
use crate::types::{self, CallableParam, CallableSig, TemplateParam, Type};

/// Rewrites every `Object(name, _)` in `ty` whose `name` matches a key in
/// `aliases` into that alias's own (already fully-resolved) `Type` -
/// `ClassInfo::from_decl`'s own local `@phpstan-type` support. Mirrors
/// `mark_template_params`'s exact recursive shape (both are a one-time
/// post-processing walk over an already-parsed `Type`), but must run
/// strictly *after* `resolve_names_in_type` has already resolved `ty`
/// itself, with `aliases` keyed by each alias's own *resolved* FQN (the
/// same name a bare, unqualified reference to it would resolve to) -
/// substituting first and re-resolving the (already-resolved) result
/// afterward double-qualifies every name the alias's value already
/// carries. `aliases` itself needs no equivalent care to avoid
/// re-resolving *its own* values further - each entry is inserted once,
/// already fully resolved, and never touched by `resolve_names_in_type`
/// again after that.
pub(crate) fn substitute_type_aliases(ty: Type, aliases: &FxHashMap<Arc<str>, Type>) -> Type {
    match ty {
        Type::Object(name, _) if aliases.contains_key(name.as_str()) => {
            aliases[name.as_str()].clone()
        }
        Type::Object(name, args) => Type::Object(
            name,
            args.into_iter()
                .map(|a| substitute_type_aliases(a, aliases))
                .collect(),
        ),
        Type::Array(key, inner) => Type::Array(
            Box::new(substitute_type_aliases(*key, aliases)),
            Box::new(substitute_type_aliases(*inner, aliases)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(substitute_type_aliases(*inner, aliases)))
        }
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| substitute_type_aliases(m, aliases))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| substitute_type_aliases(m, aliases))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, substitute_type_aliases(t, aliases), opt))
                .collect(),
        ),
        // `object{...}`'s own field types need recursing into as well,
        // the one structural wrapper this function otherwise handles
        // identically to `ArrayShape` above - a `@phpstan-type` alias
        // referenced inside e.g. `object{key: T}` (`T` itself a sibling
        // alias, not just a template) must still be substituted.
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, substitute_type_aliases(t, aliases), opt))
                .collect(),
        ),
        // Mirrors `mark_template_params`'s own new arm above.
        Type::Callable(Some(sig)) => Type::Callable(Some(Arc::new(CallableSig {
            params: sig
                .params
                .iter()
                .map(|p| CallableParam {
                    ty: substitute_type_aliases(p.ty.clone(), aliases),
                    variadic: p.variadic,
                    optional: p.optional,
                    by_ref: p.by_ref,
                })
                .collect(),
            return_type: Box::new(substitute_type_aliases((*sig.return_type).clone(), aliases)),
            templates: sig.templates.clone(),
        }))),
        // Recurses into `target`/`if_true`/`if_false`, same as
        // `mark_template_params`/`resolve_names_in_type`'s own identical
        // arms - a `@phpstan-type` alias referenced inside
        // one of a conditional return type's own branches needs the same
        // substitution as anywhere else.
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
            template_subject,
        } => Type::Conditional {
            subject,
            target: Box::new(substitute_type_aliases(*target, aliases)),
            if_true: Box::new(substitute_type_aliases(*if_true, aliases)),
            if_false: Box::new(substitute_type_aliases(*if_false, aliases)),
            negated,
            template_subject,
        },
        other => other,
    }
}

/// Builds a class's own `@phpstan-type` alias table (`Registry::
/// ClassInfo` doesn't keep a `type_aliases` field of its own - every
/// member's own `ty` is fully expanded once here, at registry-build
/// time, unlike a `@template` placeholder, which stays unresolved until
/// a real call site's own type arguments are known). Each entry is
/// keyed by the alias's own resolved FQN (`ctx.resolve`, the identical
/// resolution a bare reference to that same name anywhere else in this
/// class would go through) mapping to its value, itself run through
/// `mark_template_params`/`resolve_names_in_type` exactly once (a
/// generic class's own alias body may reference its `@template`s, e.g.
/// `@phpstan-type Result array{value: T, error: string|null}`).
///
/// A second pass resolves exactly one level of alias-to-alias nesting
/// (`ErrorList` referencing `Error`, `json-schema`'s own real shape) by
/// substituting the freshly-built map into each of its own entries -
/// deeper chains (three or more aliases referencing each other in turn)
/// aren't attempted; the innermost reference in such a chain would stay
/// an unresolved `Object` reference to the next alias's own resolved
/// name, the same "don't know, don't guess further" leniency this
/// project already accepts elsewhere rather than a full fixed-point
/// solve for a shape not yet confirmed to occur in practice.
/// Also returns `type_alias_case_refs`,
/// keyed the same way the returned alias map itself is (the alias's own
/// resolved local name) - `(raw, resolved)` pairs for every class-name
/// reference the alias's *own* RHS carries, collected during the same
/// `mark_template_params` → `resolve_names_in_type` step every other
/// PHPDoc-typed position uses. Collected from `raw`'s own construction,
/// before `substitute_type_aliases`'s second pass folds in *other*
/// aliases' own values - a sibling alias reference inside this one's RHS
/// isn't itself a class-name reference to case-check (it's resolved
/// separately, when *that* alias's own entry is built).
pub(crate) type TypeAliasCaseRefs = FxHashMap<Arc<str>, Vec<(Arc<str>, Arc<str>)>>;

pub(crate) fn build_type_aliases(
    class_doc: Option<&DocBlock>,
    template_params: &FxHashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> (FxHashMap<Arc<str>, Type>, TypeAliasCaseRefs) {
    let Some(class_doc) = class_doc else {
        return (FxHashMap::default(), FxHashMap::default());
    };
    let mut case_refs: TypeAliasCaseRefs = FxHashMap::default();
    let raw: FxHashMap<Arc<str>, Type> = class_doc
        .type_aliases
        .iter()
        .map(|(name, ty_text)| {
            let ty = types::parse_type_string(ty_text);
            let ty = mark_template_params(ty, template_params);
            let mut refs = Vec::new();
            collect_class_name_case_refs(&ty, ctx, &mut refs);
            let resolved_name = intern(&ctx.resolve(name));
            case_refs.insert(resolved_name.clone(), refs);
            let ty = resolve_names_in_type(ty, ctx);
            (resolved_name, ty)
        })
        .collect();
    let aliases = raw
        .iter()
        .map(|(name, ty)| (name.clone(), substitute_type_aliases(ty.clone(), &raw)))
        .collect();
    (aliases, case_refs)
}

/// Rewrites every `Object(name)` in a type tree whose name is one of this
/// function's own declared `@template` parameters into
/// `TemplateParam(params[name].clone())`. `parse_type_string` has no way to
/// know "T" is a template rather than an ordinary (if oddly-named) class,
/// so that reclassification happens here, where the declaring function's
/// `@template` metadata is known. A name that doesn't itself match still
/// needs its own type arguments walked (e.g. `Collection<T>`'s `T` needs
/// marking even though `Collection` doesn't match anything).
///
/// `params` carries each template's own bound/variance/scope/
/// default (`build_template_params` below), not just its bare name - every
/// occurrence of a given name gets the *same* cloned metadata, since it's
/// one declaration (`@template T of Shape`) applying everywhere `T` is
/// written, not a fresh one per site.
pub(crate) fn mark_template_params(ty: Type, params: &FxHashMap<String, TemplateParam>) -> Type {
    match ty {
        Type::Object(name, _) if params.contains_key(&name) => {
            Type::TemplateParam(params[&name].clone())
        }
        Type::Object(name, args) => Type::Object(
            name,
            args.into_iter()
                .map(|a| mark_template_params(a, params))
                .collect(),
        ),
        Type::Array(key, inner) => Type::Array(
            Box::new(mark_template_params(*key, params)),
            Box::new(mark_template_params(*inner, params)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(mark_template_params(*inner, params)))
        }
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| mark_template_params(m, params))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| mark_template_params(m, params))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, mark_template_params(t, params), opt))
                .collect(),
        ),
        // `object{...}`'s own field types need recursing into too, since
        // real PHPStan's own object-shape syntax is commonly combined
        // with generics (e.g. `@var array<int, object{key: T, priority:
        // int}>`): without this, a class-level `@template T` used inside
        // one would stay a plain, unrecognized `Object("T", [])` instead
        // of becoming `Type::TemplateParam`, which `resolve_names_in_type`
        // would then (correctly, given what it was handed) treat as a
        // genuine, as-written class reference.
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, mark_template_params(t, params), opt))
                .collect(),
        ),
        // A template param inside a `Closure(...)` shape (e.g.
        // `@param Closure(self):T $func`) - without this, `T` inside the
        // shape stays an unresolved `Object("T", [])` instead of becoming
        // a real `TemplateParam`.
        Type::Callable(Some(sig)) => Type::Callable(Some(Arc::new(CallableSig {
            params: sig
                .params
                .iter()
                .map(|p| CallableParam {
                    ty: mark_template_params(p.ty.clone(), params),
                    variadic: p.variadic,
                    optional: p.optional,
                    by_ref: p.by_ref,
                })
                .collect(),
            return_type: Box::new(mark_template_params((*sig.return_type).clone(), params)),
            templates: sig.templates.clone(),
        }))),
        // Recurses into `target`/`if_true`/`if_false` - e.g.
        // `@phpstan-return ($params is array{wrapperClass:
        // class-string<T>} ? T : Connection)`: without this, a
        // class-level/function-own `@template T` referenced inside a
        // conditional return type's own branches would stay an
        // unrecognized `Object("T", [])` instead of becoming a real
        // `TemplateParam`, the same gap this function's own `ObjectShape`/
        // `Callable` arms above each close.
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
            template_subject,
        } => Type::Conditional {
            subject,
            target: Box::new(mark_template_params(*target, params)),
            if_true: Box::new(mark_template_params(*if_true, params)),
            if_false: Box::new(mark_template_params(*if_false, params)),
            negated,
            template_subject,
        },
        other => other,
    }
}

/// Builds this docblock's own `@template` metadata - bound (`T of X`),
/// variance (`@template-covariant`/`-contravariant`), and default
/// (`T = X`) - keyed by name, shared by class-level
/// (`registry::ClassInfo::from_decl`) and function-level (`FunctionSig::
/// from_decl` below) template declarations, which otherwise parse
/// identically. `scope` names whichever class or function declares these
/// templates (`TemplateParam::scope`) - lets `checker::generics::
/// check_variance` later tell a class's own template apart from a
/// same-named one belonging to an unrelated method or ancestor.
pub(crate) fn build_template_params(
    doc: &DocBlock,
    scope: &str,
    ctx: &NameContext,
) -> FxHashMap<String, TemplateParam> {
    // A bound or default can itself reference a sibling template
    // declared on the very same docblock (`@template A` `@template B of
    // A`, or `@template B = A`) - this shallow, metadata-free pass
    // recognizes each of this docblock's own names as *some* template
    // reference (so `mark_template_params` below classifies it as
    // `TemplateParam` instead of an ordinary, nonexistent class) without
    // yet needing its real bound/variance/default, which would otherwise
    // require resolving every template before any single one of them is
    // done - substitution later replaces the whole node by name
    // regardless of what placeholder metadata it carried here.
    let shallow: FxHashMap<String, TemplateParam> = doc
        .templates
        .iter()
        .map(|n| (n.clone(), TemplateParam::unbounded(n.clone())))
        .collect();
    let resolve = |raw: &str| -> Type {
        let ty = types::parse_type_string(raw);
        let ty = mark_template_params(ty, &shallow);
        resolve_names_in_type(ty, ctx)
    };
    doc.templates
        .iter()
        .map(|name| {
            // No `of`/`as` clause at all - explicit mixed, matching
            // `TemplateParam::unbounded`'s own identical reasoning.
            let bound = doc
                .template_bounds
                .get(name)
                .map(|raw| resolve(raw))
                .unwrap_or(Type::Mixed(true));
            let default = doc.template_defaults.get(name).map(|raw| resolve(raw));
            let variance = if doc.covariant_templates.contains(name) {
                types::Variance::Covariant
            } else if doc.contravariant_templates.contains(name) {
                types::Variance::Contravariant
            } else {
                types::Variance::Invariant
            };
            (
                name.clone(),
                TemplateParam {
                    name: name.clone(),
                    bound: Box::new(bound),
                    variance,
                    scope: Some(scope.to_string()),
                    default: default.map(Box::new),
                },
            )
        })
        .collect()
}
