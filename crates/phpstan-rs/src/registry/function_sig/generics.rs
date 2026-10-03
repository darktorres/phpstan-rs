use rustc_hash::FxHashMap;
use syntax::name::Name;
use syntax::name::NameMap;

use crate::registry::name_context::{collect_class_name_case_refs, resolve_names_in_type};
use crate::registry::type_node::parse_type_node;
use phpdoc::DocType;
use phpdoc::docblock::DocBlock;
use syntax::name_resolution::NameContext;
use types::{self, TemplateParam, Type};

/// Rewrites every `Object(name, _)` in `ty` whose `name` matches a key in
/// `aliases` into that alias's own (already fully-resolved) `Type` -
/// `ClassReflection::from_decl`'s own local `@phpstan-type` support. Mirrors
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
pub(crate) fn substitute_type_aliases(ty: Type, aliases: &NameMap<Type>) -> Type {
    ty.replace_objects(&mut |name| aliases.get(name.as_str()).cloned())
}

/// Builds a class's own `@phpstan-type` alias table (`Registry::
/// ClassReflection` doesn't keep a `type_aliases` field of its own - every
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
/// Aliases used inside an alias expand at any depth (`expand_type_alias`).
/// Also returns `type_alias_case_refs`,
/// keyed the same way the returned alias map itself is (the alias's own
/// resolved local name) - `(raw, resolved)` pairs for every class-name
/// reference the alias's *own* RHS carries, collected during the same
/// `mark_template_params` → `resolve_names_in_type` step every other
/// PHPDoc-typed position uses. Collected from `raw`'s own construction,
/// before `expand_type_alias` folds in *other*
/// aliases' own values - a sibling alias reference inside this one's RHS
/// isn't itself a class-name reference to case-check (it's resolved
/// separately, when *that* alias's own entry is built).
pub(crate) type TypeAliasCaseRefs = NameMap<Vec<(Name, Name)>>;

pub(crate) fn build_type_aliases(
    class_doc: Option<&DocBlock>,
    template_params: &FxHashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> (NameMap<Type>, TypeAliasCaseRefs) {
    let Some(class_doc) = class_doc else {
        return (NameMap::default(), NameMap::default());
    };
    let mut case_refs: TypeAliasCaseRefs = NameMap::default();
    let raw: NameMap<Type> = class_doc
        .type_aliases
        .iter()
        .map(|(name, node)| {
            let ty = parse_type_node(node);
            let ty = mark_template_params(ty, template_params);
            let mut refs = Vec::new();
            collect_class_name_case_refs(&ty, ctx, &mut refs);
            let resolved_name = Name::from(&ctx.resolve(name));
            case_refs.insert(resolved_name.clone(), refs);
            let ty = resolve_names_in_type(ty, ctx);
            (resolved_name, ty)
        })
        .collect();
    let mut aliases = NameMap::default();
    let mut visiting = Vec::new();
    for name in raw.keys() {
        expand_type_alias(name, &raw, &mut aliases, &mut visiting);
    }
    (aliases, case_refs)
}

/// `name`'s value with every alias it uses expanded, at any depth. An alias
/// reached again while it is being expanded is a cycle and reads as
/// `mixed` there.
fn expand_type_alias(
    name: &Name,
    raw: &NameMap<Type>,
    done: &mut NameMap<Type>,
    visiting: &mut Vec<Name>,
) -> Type {
    if let Some(ty) = done.get(name) {
        return ty.clone();
    }
    if visiting.contains(name) {
        return Type::mixed();
    }
    visiting.push(name.clone());
    let expanded = expand_alias_uses(
        raw.get(name)
            .expect("an alias is looked up where it is declared")
            .clone(),
        raw,
        done,
        visiting,
    );
    visiting.pop();
    done.insert(name.clone(), expanded.clone());
    expanded
}

fn expand_alias_uses(
    ty: Type,
    raw: &NameMap<Type>,
    done: &mut NameMap<Type>,
    visiting: &mut Vec<Name>,
) -> Type {
    ty.replace_objects(&mut |name| {
        let (key, _) = raw.get_key_value(name.as_str())?;
        Some(expand_type_alias(&key.clone(), raw, done, visiting))
    })
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
    ty.replace_objects(&mut |name| params.get(name.as_str()).cloned().map(Type::template_param))
}

/// Builds this docblock's own `@template` metadata - bound (`T of X`),
/// variance (`@template-covariant`/`-contravariant`), and default
/// (`T = X`) - keyed by name, shared by class-level
/// (`registry::ClassReflection::from_decl`) and function-level (`FunctionReflection::
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
    let resolve = |node: &DocType| -> Type {
        let ty = parse_type_node(node);
        let ty = mark_template_params(ty, &shallow);
        resolve_names_in_type(ty, ctx)
    };
    let built: FxHashMap<String, TemplateParam> = doc
        .templates
        .iter()
        .map(|name| {
            // No `of`/`as` clause at all - explicit mixed, matching
            // `TemplateParam::unbounded`'s own identical reasoning.
            let bound = doc
                .template_bounds
                .get(name)
                .map(resolve)
                .unwrap_or(Type::explicit_mixed());
            let default = doc.template_defaults.get(name).map(resolve);
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
        .collect();
    // A sibling named in a bound or default carries its own metadata, so
    // `R of Repo<E>` reads `E`'s bound, not the placeholder's `mixed`.
    let with_siblings = |ty: &Type| {
        ty.replace_templates(&mut |tp| {
            (tp.scope.is_none())
                .then(|| built.get(&tp.name).cloned().map(Type::template_param))
                .flatten()
        })
    };
    built
        .iter()
        .map(|(name, tp)| {
            let mut tp = tp.clone();
            tp.bound = Box::new(with_siblings(&tp.bound));
            tp.default = tp.default.as_deref().map(|d| Box::new(with_siblings(d)));
            (name.clone(), tp)
        })
        .collect()
}
