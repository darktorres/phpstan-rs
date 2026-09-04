use std::collections::HashMap;

use crate::ast::{Expr, ExprKind, FunctionDecl, Literal, Visibility};
use crate::checker::union_types;
use crate::docblock::{self, DocBlock};
use crate::registry::name_context::{NameContext, resolve_names_in_type};
use crate::types::{self, TemplateParam, Type};

#[derive(Debug, Clone)]
pub struct ParamSig {
    pub name: String,
    pub ty: Type,
    pub variadic: bool,
    pub has_default: bool,
    pub by_ref: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub params: Vec<ParamSig>,
    pub return_type: Type,
    // The return type the *native* signature alone declares, ignoring any
    // `@return` docblock refinement - real PHP only ever enforces
    // override-compatibility (Liskov) at this native level, and by
    // default so does real PHPStan (`checkPhpDocMethodSignatures` is off
    // unless a project opts in, confirmed neither Piwigo's own
    // `phpstan.neon` nor this project's own corpus config does).
    // `check_method_overrides`'s own return-type check is the only
    // reader - every other consumer of a function's return type wants
    // the docblock-refined `return_type` above instead.
    pub return_type_native: Type,
    // Phase 37: PHP 8.5's `#[\NoDiscard]` marker attribute - `true` when
    // this declaration carries one, meaning a bare, unused call to it
    // (`foo();` on its own line, return value neither assigned nor
    // otherwise consumed) is a real diagnostic
    // (`Checker::check_no_discard_statement`), matching real PHPStan's own
    // `NativeFunctionReflection::mustUseReturnValue`/
    // `NativeMethodReflection`'s identical convention: case-insensitive
    // match on the attribute's own short name (no namespace resolution,
    // since real PHP's own `#[NoDiscard]` is always the single, global-
    // namespace one - there's no other class this could plausibly mean).
    pub no_discard: bool,
    // Phase 57: `public`/`private`/`protected` - `Visibility::Public` for
    // every plain top-level function (PHP has no way to declare one
    // otherwise), a method's own declared visibility otherwise. Threaded
    // through so a call site can enforce access legality the same way
    // `PropertySig::visibility` does for properties.
    pub visibility: Visibility,
    // Phase 67: `@internal` on this function/method's own doc comment -
    // see `docblock::DocBlock::internal`'s own doc comment for what it
    // means and how it's gated. A method also counts as internal when
    // its *declaring class* does, but that half is checked separately
    // (against `ClassInfo::is_internal`) by whichever call site already
    // has the declaring class in hand - this field only ever reflects
    // the member's own tag.
    pub is_internal: bool,
}

/// Resolves a param's/property's type: a PHPDoc type (when present) wins
/// over the native hint, since it's usually the more precise of the two
/// (an untyped native param described only in `@param`, or a native `array`
/// refined to `User[]`) - falls back to the native hint, then `Mixed`.
/// A parameter with a plain (non-`?`) native type hint and a bare
/// `= null` default is implicitly nullable in real PHP - `function
/// f(int $x = null)` accepts `null` exactly as if it had been written
/// `function f(?int $x = null)`. This was the only way to write an
/// optional/nullable native parameter before PHP 7.1 introduced `?`, so
/// a large amount of real code (old and new) still uses this style;
/// later PHP versions only added a deprecation notice for it, the
/// behavior itself is unchanged.
pub(crate) fn apply_implicit_nullable_default(ty: Type, default: Option<&Expr>) -> Type {
    let is_bare_null_default = matches!(
        default.map(|e| &e.kind),
        Some(ExprKind::Literal(Literal::Null))
    );
    if !is_bare_null_default {
        return ty;
    }
    let already_nullable = match &ty {
        Type::Null | Type::Mixed => true,
        Type::Union(members) => members.contains(&Type::Null),
        _ => false,
    };
    if already_nullable {
        ty
    } else {
        Type::Union(vec![ty, Type::Null])
    }
}

pub(crate) fn resolve_type(doc_type: Option<&str>, native_hint: Option<&str>) -> Type {
    let Some(doc) = doc_type else {
        return native_hint
            .map(types::parse_type_string)
            .unwrap_or(Type::Mixed);
    };
    let ty = types::parse_type_string(doc);
    // A docblock type is trusted as the complete picture and otherwise
    // fully replaces the native hint (deliberate - it's usually more
    // precise) - but real docblocks very commonly aren't fully redundant
    // with the native hint, most often by not bothering to repeat a `?`
    // the native signature already has (`/** @param int $x */` on
    // `function f(?int $x)`, since the signature already fully states
    // the nullability). Silently losing that would make an entirely
    // legitimate `null` argument a false positive - union `Null` in
    // unless the docblock type already says so itself.
    let native_is_nullable = native_hint.is_some_and(|h| h.trim_start().starts_with('?'));
    let already_nullable = match &ty {
        Type::Null | Type::Mixed => true,
        Type::Union(members) => members.contains(&Type::Null),
        _ => false,
    };
    let ty = if native_is_nullable && !already_nullable {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    };
    // Phase 23/Phase 32 Finding I: phpstorm-stubs' own `@return`/`@param`
    // doc comments commonly predate a later PHP version's native-signature
    // tightening - real shape: `imagecreatefromstring`'s native return hint
    // was updated to `GdImage|false` for PHP 8's GD object migration, but
    // its `@return resource|GdImage|false` doc comment above it was never
    // revised (Phase 23's own motivating case); `imagesx`/`imagesy`'s
    // native `int` similarly dropped `false` without their `@return
    // int|false` doc ever catching up. PHP enforces a native type hint
    // identically at runtime, so when one is present and real, it's
    // authoritative about what's *possible* - any docblock member
    // incompatible with it can never actually happen, whatever a stale
    // comment claims. `resource` and `false` aren't the only members that
    // can go stale this way, so rather than hardcoding a check per member
    // (as this project's own first attempt at `false` did, before finding
    // this generalization instead), `reconcile_with_native` tests
    // "incompatible" precisely via `is_subtype`.
    //
    // Excludes `object` specifically - real code (this project's own
    // `Logger::$fileHandle`, Symfony `Process::setInput`) deliberately
    // types something `mixed`/`object` *because* PHP has no native
    // `resource` type to declare, leaning on the docblock to carry the
    // only real type information. A `mixed` native doesn't need its own
    // exclusion here: `is_subtype(_, Type::Mixed)` is unconditionally
    // `true`, so `reconcile_with_native` already keeps every doc member
    // when native is `mixed`, for free. `object`'s own subtyping is
    // name-equality-only (see `types::is_subtype`'s own doc comment), so
    // it doesn't get the same automatic fallout and still needs the
    // explicit guard.
    match native_hint {
        Some(native) if native.trim() != "object" => {
            reconcile_with_native(ty, types::parse_type_string(native))
        }
        _ => ty,
    }
}

/// Filters `doc_ty`'s own members down to whichever are subtypes of
/// `native_ty` - see `resolve_type`'s own doc comment for the reasoning.
/// Falls back to `native_ty` itself when *no* doc member survives (rather
/// than collapsing to `Mixed`), so a doc type that's entirely stale
/// (`finfo_file`'s bare `resource` against a modern `finfo` native) still
/// resolves to the real, useful native type instead of losing all
/// precision.
fn reconcile_with_native(doc_ty: Type, native_ty: Type) -> Type {
    let members: Vec<Type> = match doc_ty {
        Type::Union(members) => members,
        other => vec![other],
    };
    let kept: Vec<Type> = members
        .into_iter()
        .filter(|m| member_possible_given_native(m, &native_ty))
        .map(|m| refine_callable_member(m, &native_ty))
        .collect();
    match kept.len() {
        0 => native_ty,
        1 => kept.into_iter().next().unwrap(),
        _ => Type::Union(kept),
    }
}

/// A surviving `Type::Callable` member (kept only because
/// `member_possible_given_native` can't disprove it) is itself a coarse
/// marker with strictly less information than a real, present native
/// type already carries - left as `Callable`, it can wrongly *lose* to
/// the native type in an unrelated later comparison that only sees this
/// resolved signature, not the native hint that produced it (real
/// corpus regression: `Doctrine\Common\Collections\ReadableCollection::
/// exists(Closure $p)`'s own `@phpstan-param Closure(TKey, T):bool $p`
/// resolved to bare `Callable` instead of the exactly-known native
/// `Closure`, making a subclass's own identically-typed override look
/// like an illegal narrowing purely because the *ancestor's* resolved
/// type had lost precision the child's never did). Substitutes the
/// native type's own concrete counterpart back in whenever there is
/// one - a plain class-typed native, or (when native itself is
/// nullable) its sole non-null member - leaving `Callable` alone
/// otherwise (an untyped-native `callable $p`, or one this project
/// still can't say anything more precise about).
fn refine_callable_member(m: Type, native_ty: &Type) -> Type {
    if m != Type::Callable {
        return m;
    }
    match native_ty {
        Type::Object(_, _) => native_ty.clone(),
        Type::Union(members) => {
            match members
                .iter()
                .filter(|t| **t != Type::Null)
                .collect::<Vec<_>>()
                .as_slice()
            {
                [Type::Object(_, _)] => members
                    .iter()
                    .find(|t| **t != Type::Null)
                    .cloned()
                    .unwrap_or(m),
                _ => m,
            }
        }
        _ => m,
    }
}

/// Whether a docblock-derived member `m` could still be real given
/// `native_ty` - found necessary the hard way, via a full corpus re-check
/// after first shipping this as a bare `is_subtype(m, native_ty)` call:
/// for anything except a named class/interface (`Type::Object`),
/// `is_subtype` gives a reliable, hierarchy-independent answer (scalar/
/// array/... subtyping never depends on an unverified class
/// relationship). For an `Object`, this project has no class hierarchy
/// to consult at the point `resolve_type` runs (`FunctionSig::from_decl`
/// is still building the very registry a real lookup would need,
/// long before one exists) - `is_subtype`'s own Object handling is
/// name-equality only, so treating that as "incompatible" wrongly
/// discarded a real, common shape: a doc type naming an actual subclass
/// of a more generic native return hint (real corpus regression:
/// Doctrine-repository-typed services declaring a native
/// `EntityRepository` return with a doc type naming the real concrete
/// repository subclass, e.g. `ActivityRepository` - is_subtype couldn't
/// see the inheritance, so the doc's own more-precise type was wrongly
/// dropped in favor of the generic base class). Scoped back to exactly
/// the one class-name case this project *can* prove reliably (Phase 23's
/// own original, narrower finding: a literal `resource` doc member
/// against a native hint that no longer mentions it) - any other class
/// name is always kept, since "doc names a real subclass" and "doc is
/// simply stale" can't be told apart without the hierarchy this function
/// doesn't have, and silently guessing wrong here is worse than not
/// generalizing past what's actually provable.
fn member_possible_given_native(m: &Type, native_ty: &Type) -> bool {
    match m {
        Type::Object(name, _) if name == "resource" => types::is_subtype(m, native_ty),
        Type::Object(_, _) => true,
        // Phase 60 corpus find: a PHPStan/Psalm callable-shape doc member
        // (`Closure(int): string`) degrades to plain `Type::Callable` at
        // parse time (`types::parse_type_atom`'s own documented
        // simplification - no per-parameter/return signature is tracked)
        // - the same information loss `Object`'s own case just above
        // already has to work around, just one layer earlier. Without
        // this, `is_subtype(Callable, Closure)` (correctly `false` in
        // general - not every callable is a `Closure` instance) wrongly
        // read as "the doc member can't be right," discarding it even
        // when the native hint is exactly `Closure` - a real corpus
        // false positive: `Psl\Dict\filter()`'s own `null|Closure
        // $predicate` parameter, documented as `(Closure(Tv): bool)|null`,
        // resolved to bare `null` once the `Closure(...)`-shaped member
        // was filtered out, in turn making Phase 60's `nullCoalesce`
        // check wrongly flag `$predicate ??= ...` as "always null."
        Type::Callable => true,
        _ => types::is_subtype(m, native_ty),
    }
}

/// A class constant's value is almost always a literal in real code - this
/// covers the common cases the same way `checker.rs`'s own literal
/// inference does, without pulling in the full expression-inference engine
/// (which lives in `checker.rs` and depends on `Registry` itself, so it
/// can't be called from here). Anything more complex than a bare literal
/// (an array, a concatenation, another constant) falls back to `Mixed` -
/// undefined-constant detection only needs the name to be known, not a
/// precise type.
pub(crate) fn infer_const_type(value: &crate::ast::Expr) -> Type {
    use crate::ast::Literal;
    match &value.kind {
        ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(*n),
        ExprKind::Literal(Literal::Float(_)) => Type::Float,
        ExprKind::Literal(Literal::String(s)) => Type::StringLiteral(s.clone()),
        ExprKind::Literal(Literal::Bool(true)) => Type::True,
        ExprKind::Literal(Literal::Bool(false)) => Type::False,
        ExprKind::Literal(Literal::Null) => Type::Null,
        // Phase 23: `const int OFF = -1;` parses as a literal `1` wrapped
        // in a unary negation, not a single negative-literal token - a
        // negative-valued typed constant (real shape: `Piwigo\Core\
        // Logger::OFF`, the one negative severity level alongside seven
        // non-negative ones) otherwise silently fell to `Mixed` below.
        ExprKind::Unary(crate::ast::UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(-n),
            ExprKind::Literal(Literal::Float(_)) => Type::Float,
            _ => Type::Mixed,
        },
        // A round of Piwigo-corpus bucketing found this real, common
        // shape (`AppInfo::URL = 'https://' . self::DOMAIN;`) - a
        // const's own value referencing *another* const via `self::`
        // can't be resolved here at all (this function has no class
        // context, and no registry to look the other const up in even
        // if it did), but real PHP string concatenation always
        // produces a `string` regardless of either operand's own type,
        // so this is sound information even without resolving anything
        // - unlike falling back to `Mixed` below, which silently
        // widened every array literal/argument this const then fed
        // into.
        ExprKind::Binary(crate::ast::BinOp::Concat, ..) => Type::String,
        _ => Type::Mixed,
    }
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
/// Phase 54: `params` carries each template's own bound/variance/scope/
/// default (`build_template_params` below), not just its bare name - every
/// occurrence of a given name gets the *same* cloned metadata, since it's
/// one declaration (`@template T of Shape`) applying everywhere `T` is
/// written, not a fresh one per site.
pub(crate) fn mark_template_params(ty: Type, params: &HashMap<String, TemplateParam>) -> Type {
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
        other => other,
    }
}

/// Phase 37: whether an `Attribute`'s own raw (possibly namespaced) name
/// names the real PHP 8.5 `#[\NoDiscard]` marker - real PHPStan's own
/// `NativeFunctionReflection::mustUseReturnValue` matches by short name
/// only, case-insensitively (`strtolower($attrib->getName()) ===
/// 'nodiscard'`), not a fully-resolved, namespace-aware class reference -
/// mirrored exactly here rather than resolving `name` against `use`
/// imports the way an ordinary attribute-class reference would, since
/// real PHP's own attribute is always the single global-namespace one and
/// there's no other plausible class this could mean.
pub(crate) fn is_no_discard_attribute_name(name: &str) -> bool {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("NoDiscard")
}

/// Builds this docblock's own `@template` metadata - bound (`T of X`),
/// variance (`@template-covariant`/`-contravariant`), and default
/// (`T = X`, Phase 54) - keyed by name, shared by class-level
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
) -> HashMap<String, TemplateParam> {
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
    let shallow: HashMap<String, TemplateParam> = doc
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
            let bound = doc
                .template_bounds
                .get(name)
                .map(|raw| resolve(raw))
                .unwrap_or(Type::Mixed);
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

impl FunctionSig {
    /// `class_templates` is the enclosing class's own `@template` metadata
    /// (empty for a plain function, or a method on a non-generic class) -
    /// merged in alongside the function/method's own, so a method body
    /// referencing a template its *class* declares (not itself) still
    /// gets it correctly recognized as `TemplateParam` rather than an
    /// ordinary (nonexistent) class name. This function/method's own
    /// same-named template (rare, but not invalid PHP) wins on a
    /// collision, matching a nested scope shadowing an outer one.
    pub(crate) fn from_decl(
        f: &FunctionDecl,
        ctx: &NameContext,
        class_templates: &HashMap<String, TemplateParam>,
        visibility: Visibility,
    ) -> FunctionSig {
        let doc: Option<DocBlock> = f.doc_comment.as_deref().map(docblock::parse);
        let mut template_params: HashMap<String, TemplateParam> = doc
            .as_ref()
            .map(|d| build_template_params(d, &f.name, ctx))
            .unwrap_or_default();
        for (name, tp) in class_templates {
            template_params
                .entry(name.clone())
                .or_insert_with(|| tp.clone());
        }

        let params = f
            .params
            .iter()
            .map(|p| {
                let doc_ty = doc.as_ref().and_then(|d| d.params.get(&p.name));
                let ty = resolve_type(doc_ty.map(String::as_str), p.type_hint.as_deref());
                let ty = mark_template_params(ty, &template_params);
                let ty = resolve_names_in_type(ty, ctx);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                // A real `= value` default always wins; failing that, a
                // `[optional]` marker on this param's own `@param` tag is
                // trusted too (see `DocBlock::optional_params` - phpstorm-
                // stubs models several functions, e.g. `mt_rand`/
                // `array_keys`, only as their highest-arity overload, with
                // trailing params marked optional in the docblock but
                // carrying no native default at all).
                let has_default = p.default.is_some()
                    || doc
                        .as_ref()
                        .is_some_and(|d| d.optional_params.contains(&p.name));
                ParamSig {
                    name: p.name.clone(),
                    ty,
                    variadic: p.variadic,
                    has_default,
                    by_ref: p.by_ref,
                }
            })
            .collect();
        let params = dedup_versioned_params(params);

        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_deref());
        let return_type = resolve_type(doc_return, f.return_type.as_deref());
        let return_type = mark_template_params(return_type, &template_params);
        let return_type = resolve_names_in_type(return_type, ctx);

        let return_type_native = resolve_type(None, f.return_type.as_deref());
        let return_type_native = mark_template_params(return_type_native, &template_params);
        let return_type_native = resolve_names_in_type(return_type_native, ctx);

        let no_discard = f
            .attrs
            .iter()
            .any(|a| is_no_discard_attribute_name(&a.name));

        let is_internal = doc.as_ref().is_some_and(|d| d.internal);

        FunctionSig {
            params,
            return_type,
            return_type_native,
            no_discard,
            visibility,
            is_internal,
        }
    }

    /// Phase 32 Finding C: merges two *unconditional* declarations of the
    /// same function name - the real-overload shape phpstorm-stubs uses
    /// for `strtr` (a 3-string-argument form and a 2-argument, `array
    /// $replace_pairs` form, as two separate top-level declarations
    /// sharing one name), mirroring `ClassInfo::merge`'s own "later
    /// declaration wins on conflict" convention.
    pub(crate) fn merge(self, other: FunctionSig) -> FunctionSig {
        let return_type = union_types(self.return_type, other.return_type);
        let return_type_native = union_types(self.return_type_native, other.return_type_native);
        // Permissive, same direction as `variadic`/`has_default` below:
        // if either real overload actually carries the marker, the merged
        // signature should too - a caller ignoring the return value must
        // not silently escape the obligation just because *one* overload
        // (of what's really the same underlying declaration, PHP-version-
        // split) happened not to repeat the attribute.
        let no_discard = self.no_discard || other.no_discard;
        let len = self.params.len().max(other.params.len());
        let mut self_params = self.params.into_iter();
        let mut other_params = other.params.into_iter();
        let mut params = Vec::with_capacity(len);
        for _ in 0..len {
            let a = self_params.next();
            let b = other_params.next();
            let merged = match (a, b) {
                (Some(a), Some(b)) => ParamSig {
                    // Later declaration wins on conflict, matching
                    // `ClassInfo::merge`'s own convention.
                    name: b.name,
                    ty: union_types(a.ty, b.ty),
                    // Permissive: accepting a variadic-style tail call
                    // when *either* real overload allows it costs nothing
                    // a stricter rule would have caught.
                    variadic: a.variadic || b.variadic,
                    // A position required by only one real overload can't
                    // be required in a callable meant to accept either.
                    has_default: a.has_default || b.has_default,
                    // Unlike the above, NOT permissive: `by_ref` is a
                    // restriction placed on the *caller* (an lvalue is
                    // required), not something being accepted - OR-ing it
                    // would wrongly flag a call matching the non-by-ref
                    // overload's own shape just because the *other*
                    // overload needs a reference there. AND keeps the
                    // merged signature only as strict as every real
                    // overload agrees it should be.
                    by_ref: a.by_ref && b.by_ref,
                },
                // A position present in only one real overload can't be
                // required in the merged callable either.
                (Some(mut only), None) | (None, Some(mut only)) => {
                    only.has_default = true;
                    only
                }
                (None, None) => unreachable!("loop bound is the longer side's own length"),
            };
            params.push(merged);
        }
        FunctionSig {
            params,
            return_type,
            return_type_native,
            no_discard,
            // Both real overloads share one declaration (a stub-modeled
            // PHP-version split of the same underlying function/method,
            // never two independently-declared visibilities) - either
            // side's own value is equally correct.
            visibility: self.visibility,
            // Same permissive "either overload's own marker wins" as
            // `no_discard` just above.
            is_internal: self.is_internal || other.is_internal,
        }
    }
}

/// phpstorm-stubs represents "this parameter's type/default changed
/// across PHP versions" by declaring the *same conceptual parameter
/// twice*, once per era, each gated by its own
/// `#[PhpStormStubsElementAvailable(from: X, to: Y)]` attribute (real
/// shape: `pg_query`'s `$connection`, `fsockopen`'s `$port`,
/// `SQLite3::open`'s `$flags`/`$encryptionKey`) - but real PHP cannot
/// declare two parameters with the same name in one function (a fatal
/// error), so any duplicate name reaching here is unconditionally this
/// versioning artifact, never a legitimate shape, regardless of which
/// attribute produced it or whether it has a default. The lexer's own
/// `skip_stub_availability_attribute` heuristic already drops some of
/// these (a closed-range duplicate with no default), but keeps one that
/// does have a default - correct for a different real shape
/// (`imagefilter`'s uniquely-named `$arg1`..`$arg4` before a
/// differently-named variadic) but wrong when the kept duplicate's name
/// is reused by a later parameter, producing a function with one extra
/// position and every real argument after it binding to the wrong
/// parameter. Collapsing here, after the lexer, needs no knowledge of
/// that versioning idiom at all: keep each name's *last* occurrence
/// (phpstorm-stubs always writes the legacy era first, the modern one
/// last), at the *position* of its *first* occurrence, so later
/// uniquely-named parameters aren't shifted.
fn dedup_versioned_params(params: Vec<ParamSig>) -> Vec<ParamSig> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, ParamSig> = HashMap::new();
    for p in params {
        if !by_name.contains_key(&p.name) {
            order.push(p.name.clone());
        }
        by_name.insert(p.name.clone(), p);
    }
    order
        .into_iter()
        .map(|name| by_name.remove(&name).expect("just inserted for this name"))
        .collect()
}
