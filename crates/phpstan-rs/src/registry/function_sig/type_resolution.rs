use rustc_hash::FxHashMap;

use super::generics::mark_template_params;
use crate::registry::resolve::type_contains_deferred_value_of;
use crate::registry::type_node::parse_type_node;
use phpdoc::grammar::TypeNode;
use syntax::ast::{Expr, ExprKind, Literal};
use types::{self, TemplateParam, Type};

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
        Type::Null | Type::Mixed(_) => true,
        Type::Union(members) => members.contains(&Type::Null),
        _ => false,
    };
    if already_nullable {
        ty
    } else {
        Type::Union(vec![ty, Type::Null])
    }
}

pub(crate) fn resolve_type(doc_type: Option<&TypeNode>, native_hint: Option<&str>) -> Type {
    let native_ty = native_hint.map(types::parse_native_type_string);
    resolve_type_with_parsed_native(
        doc_type,
        native_hint,
        native_ty.as_ref(),
        &FxHashMap::default(),
    )
}

/// `resolve_type`'s own real body, taking the native hint's already-parsed
/// `Type` instead of re-deriving it from `native_hint` via a second
/// `types::parse_type_string` call - a caller that also needs the
/// native-only type on its own (`FunctionReflection::from_decl`'s `ty`/`ty_native`
/// split) parses `native_hint` once and passes the same `Type` to both,
/// instead of each independently re-parsing the identical string.
/// `native_hint`'s own raw text is still needed alongside `native_ty` for
/// the two checks below that read it as text (nullability prefix, the
/// `object` exclusion) rather than as a parsed `Type`. `template_params`
/// are the templates in scope, marked in the doc type before it is
/// reconciled with the native one.
pub(crate) fn resolve_type_with_parsed_native(
    doc_type: Option<&TypeNode>,
    native_hint: Option<&str>,
    native_ty: Option<&Type>,
    template_params: &FxHashMap<String, TemplateParam>,
) -> Type {
    let Some(doc) = doc_type else {
        return native_ty.cloned().unwrap_or(Type::Mixed(false));
    };
    let ty = mark_template_params(parse_type_node(doc), template_params);
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
    // A conditional return type states its nullability per branch
    // (`($p is non-empty-array ? T : T|null)`), so a native `?` must not
    // be unioned onto every resolution of it.
    let already_nullable = match &ty {
        Type::Null | Type::Mixed(_) | Type::Conditional { .. } => true,
        Type::Union(members) => members.contains(&Type::Null),
        _ => false,
    };
    let ty = if native_is_nullable && !already_nullable {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    };
    // phpstorm-stubs' own `@return`/`@param` doc comments commonly
    // predate a later PHP version's native-signature tightening - e.g.
    // `imagecreatefromstring`'s native return hint was updated to
    // `GdImage|false` for PHP 8's GD object migration, but its `@return
    // resource|GdImage|false` doc comment above it was never revised;
    // `imagesx`/`imagesy`'s native `int` similarly dropped `false`
    // without their `@return int|false` doc ever catching up. PHP
    // enforces a native type hint identically at runtime, so when one is
    // present and real, it's authoritative about what's *possible* - any
    // docblock member incompatible with it can never actually happen,
    // whatever a stale comment claims. `resource` and `false` aren't the
    // only members that can go stale this way, so rather than
    // hardcoding a check per member, `reconcile_with_native` tests
    // "incompatible" precisely via `subtype`.
    //
    // Excludes `object` specifically - real code (this project's own
    // `Logger::$fileHandle`, Symfony `Process::setInput`) deliberately
    // types something `mixed`/`object` *because* PHP has no native
    // `resource` type to declare, leaning on the docblock to carry the
    // only real type information. A `mixed` native doesn't need its own
    // exclusion here: `subtype(_, Type::Mixed(false))` is unconditionally
    // `true`, so `reconcile_with_native` already keeps every doc member
    // when native is `mixed`, for free. `object`'s own subtyping is
    // name-equality-only (see `Relation::subtype`'s own doc comment), so
    // it doesn't get the same automatic fallout and still needs the
    // explicit guard.
    match (native_hint, native_ty) {
        (Some(native), Some(native_ty)) if native.trim() != "object" => {
            reconcile_with_native(ty, native_ty.clone())
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
pub(crate) fn reconcile_with_native(doc_ty: Type, native_ty: Type) -> Type {
    let members: Vec<Type> = match doc_ty {
        Type::Union(members) => members,
        other => vec![other],
    };
    let mut kept: Vec<Type> = members
        .into_iter()
        .filter(|m| member_possible_given_native(m, &native_ty))
        .map(|m| refine_callable_member(m, &native_ty))
        .collect();
    // A doc union that only narrows *one* member of a native union
    // (`array|string` native, `@param array<int, string>` doc) must keep
    // the native's *other*, untouched alternatives alongside the refined
    // one - the filter above only ever narrows `kept`'s own member list
    // down, it never adds one back. Mirrors real PHPStan's own
    // `TypehintHelper::decideType` per-native-member keep-or-refine
    // logic, confirmed via `dumpType()`: a native `string|array` refined
    // by `@param array<int, string>` resolves to `array<int,
    // string>|string`, not bare `array<int, string>`. A native member
    // already covered by a surviving, narrower `kept` entry (`kept`
    // contains a real subtype of it) was the one the docblock refined -
    // only an *uncovered* native member (the docblock said nothing about
    // it at all) survives verbatim.
    //
    // Skipped whenever a kept member still contains an unresolved
    // `Type::ValueOf` (`value-of<T>` on a still-unbound `@template`):
    // `Registry::resolve_deferred_value_of_with_native_fallback`
    // runs *after* this function, expects to find the `ValueOf` alone
    // (not already unioned with extra native members this function
    // doesn't yet know will turn out unneeded), and does its own,
    // separate "resolved to bare `Mixed`? use the native type instead"
    // swap once its real value is known - unioning native
    // members in early would corrupt that later check's own `resolved ==
    // Type::Mixed(false)` test (a real regression caught by this project's own
    // existing `value_of_on_an_unresolvable_template_falls_back_to_the_native_return_type`
    // test).
    //
    // Also skipped when nothing survived the filter at all (the
    // `0 => native_ty` fallback below is the answer then - backfilling
    // here would instead leave only the native union's non-object
    // leftovers, e.g. a bare `null` out of `?Pkg`), and when a kept member
    // is a conditional return type: it resolves per call site, and a
    // backfilled native alternative would survive every resolution,
    // turning a `non-empty-array` call's `T` into `T|null`. A kept member
    // naming a template is skipped the same way: the template's binding
    // is the alternative the native member stands for, so `TKey|null`
    // over a native `string|int|null` stays `TKey|null`.
    if let Type::Union(native_members) = &native_ty
        && !kept.is_empty()
        && !kept.iter().any(type_contains_deferred_value_of)
        && !kept.iter().any(|k| matches!(k, Type::Conditional { .. }))
        && !kept.iter().any(types::references_template)
    {
        for nm in native_members {
            if native_member_addable(nm)
                && !kept
                    .iter()
                    .any(|k| types::Relation::structural().subtype(k, nm))
            {
                kept.push(nm.clone());
            }
        }
    }
    match kept.len() {
        0 => native_ty,
        1 => kept.into_iter().next().unwrap(),
        _ => Type::Union(kept),
    }
}

/// Whether a native union member is safe for `reconcile_with_native` to
/// add back on its own "the docblock never addressed this alternative"
/// theory - deliberately excludes *every* `Type::Object` native member
/// and the `self`/`static`/`parent` class references (a doc `static`
/// already covers a native `self`), including the `object`/`resource`
/// pseudo-types: a doc member naming a
/// real *subclass* of that native class, or a plain instance covered by
/// the native `object` pseudo-type's own "accepts any object" leniency,
/// or (for `Closure` specifically) a `Closure(...)`-shaped doc member
/// `refine_callable_member` above deliberately leaves as a bare
/// `Type::Callable` rather than the native `Object("Closure", _)` itself,
/// all genuinely *did* address that native alternative - but
/// `Relation::subtype`'s own Object handling is name-equality only, with
/// none of those leniencies (this function has no `&Registry` to check
/// real hierarchy or any of `Registry::accepts`'s own object-
/// specific rules with - see `member_possible_given_native`'s own
/// identical "no hierarchy this early" doc comment). Wrongly adding a
/// covered class-named member back produces a real, new false positive
/// (confirmed twice over a first pass of this fix: the `object`
/// pseudo-type case broke `a_call_through_a_method_scoped_backfilled_template_binds_t_from_the_real_argument`/
/// `a_class_string_template_parameter_binds_from_a_literal_naming_a_real_class_through_an_ordinary_call`/
/// `an_anonymous_class_inherits_a_method_scoped_template_parameterized_type`
/// with a spurious extra `object` alternative, and a narrower attempt
/// that special-cased only `Closure`-vs-`Callable` broke nothing new but
/// left the `object` case unguarded) - strictly worse than this
/// mechanism's own pre-existing gap of simply not backfilling that one
/// alternative, so no `Type::Object` native member is ever added back
/// here; only the non-Object cases this function can decide with no
/// hierarchy needed (the actual target shape - a native `array|string`
/// refined by `@param array<int, string>` - is one of these).
fn native_member_addable(native_member: &Type) -> bool {
    !matches!(
        native_member,
        Type::Object(_, _)
            | Type::ObjectWithoutClass
            | Type::SelfRef(_)
            | Type::StaticRef(_)
            | Type::This
            | Type::ParentRef
    )
}

/// A surviving `Type::Callable` member (kept only because
/// `member_possible_given_native` can't disprove it) is itself a coarse
/// marker with strictly less information than a real, present native
/// type already carries - left as `Callable`, it can wrongly *lose* to
/// the native type in an unrelated later comparison that only sees this
/// resolved signature, not the native hint that produced it - e.g. a
/// method's own `@phpstan-param Closure(TKey, T):bool $p` resolving to
/// bare `Callable` instead of the exactly-known native `Closure` would
/// make a subclass's own identically-typed override look like an
/// illegal narrowing purely because the ancestor's resolved type had
/// lost precision the child's never did. Substitutes the
/// native type's own concrete counterpart back in whenever there is
/// one - a plain class-typed native, or (when native itself is
/// nullable) its sole non-null member - leaving `Callable` alone
/// otherwise (an untyped-native `callable $p`, or one this project
/// still can't say anything more precise about).
fn refine_callable_member(m: Type, native_ty: &Type) -> Type {
    // Only the bare marker is "coarse" in the sense this function
    // exists to fix - a real parsed `Closure(...)`/
    // `callable(...)` shape (`Some(sig)`) is already a genuine
    // refinement in its own right and shouldn't be thrown away in favor
    // of a plain native class type.
    if !matches!(m, Type::Callable(None)) {
        return m;
    }
    match native_ty {
        Type::Object(_, _) | Type::ObjectWithoutClass => native_ty.clone(),
        Type::Union(members) => {
            match members
                .iter()
                .filter(|t| **t != Type::Null)
                .collect::<Vec<_>>()
                .as_slice()
            {
                [Type::Object(_, _) | Type::ObjectWithoutClass] => members
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
/// `native_ty`. A bare `subtype(m, native_ty)` call is reliable for
/// anything except a named class/interface (`Type::Object`), where
/// `subtype`'s own Object handling is name-equality only: this
/// project has no class hierarchy to consult at the point `resolve_type`
/// runs (`FunctionReflection::from_decl` is still building the very registry a
/// real lookup would need, long before one exists), so treating a
/// hierarchy mismatch as "incompatible" would wrongly discard a common
/// shape - a doc type naming an actual subclass of a more generic native
/// return hint (e.g. a native `EntityRepository` return with a doc type
/// naming the real concrete repository subclass). Scoped back to exactly
/// the one class-name case this project can prove reliably: a literal
/// `resource` doc member against a native hint that no longer mentions
/// it - any other class name is always kept, since "doc names a real
/// subclass" and "doc is simply stale" can't be told apart without the
/// hierarchy this function doesn't have, and silently guessing wrong
/// here is worse than not generalizing past what's actually provable.
// A native `void` return hint admits no value at all - not even a named
// class or callable, both otherwise deliberately trusted (see the
// `Object`/`Callable` arms below) since this function usually can't
// structurally disprove them for a real, non-`void` native hint. `void`
// needs no such leniency: it's never legitimately narrowed by *any* doc
// type, so this is checked before the general `Object`/`Callable`
// permissiveness below ever gets a chance to wrongly keep a doc member
// real PHPStan would always discard in favor of the native `void`
// (`TypehintHelper::decideType`'s own `$type->isSuperTypeOf($phpDocType)`
// is unconditionally `no()` for `VoidType`). Without this, an inherited
// `@return null|int|Node|Node[]` doc could backfill straight through a
// native `void` override further up the hierarchy - the `Node` member
// would survive via the `Object => true` rule below with nothing to
// stop it, wrongly demanding a real `Node` return from a function whose
// own, authoritative native signature is `void`.
//
// `Type::Never` is exempted from this exclusion - real PHPStan special-
// cases it identically (`TypehintHelper::decideType`'s own `if
// ($phpDocType instanceof NeverType && $phpDocType->isExplicit()) {
// return $phpDocType; }`, checked *before* its general `isSuperTypeOf`
// rule): an explicit `@return never` genuinely means the function always
// diverges (throws/exits/loops forever), which is honestly compatible
// with *any* native return hint, `void` included - a real corpus
// regression caught verifying the `void` exclusion above:
// `Nette\Utils\ObjectHelpers::strictStaticCall(): void` (own `@return
// never`) collapsing to plain `void` lost the one piece of information
// (`Never`) this project's own control-flow-termination analysis needs
// to recognize the call as always-diverging, wrongly flagging every
// `__callStatic` that ends with `ObjectHelpers::strictStaticCall(...);`
// and no further `return` as "should never return, but a return
// statement was reached."
fn member_possible_given_native(m: &Type, native_ty: &Type) -> bool {
    if *native_ty == Type::Void && *m != Type::Void && *m != Type::Never {
        return false;
    }
    match m {
        // A class reference or a template can't be verified this early. A late-
        // resolvable type or a `ClassConstant` is trusted the same way - `Registry::
        // resolve_deferred_value_of`'s own later, whole-registry pass
        // (see its own doc comment) restores the "fall back to native
        // when the resolved doc type turns out to be Mixed" behavior this
        // function would otherwise have given for free, once its real
        // value is actually known.
        Type::Object(_, _)
        | Type::TemplateParam(_)
        | Type::ObjectWithoutClass
        | Type::ValueOf(_)
        | Type::KeyOf(_)
        | Type::OffsetAccess(..)
        | Type::ClassConstant { .. } => true,
        // `@return static` under a native `self`/`static`/class hint: the
        // late-bound class is always an instance of the declaring one.
        Type::StaticRef(_) | Type::This | Type::SelfRef(_) => !matches!(
            native_ty,
            Type::Int | Type::Float | Type::String | Type::Bool | Type::Array(..) | Type::Null
        ),
        // A PHPStan/Psalm callable-shape doc member (`Closure(int):
        // string`) degrades to plain `Type::Callable` at parse time
        // (`types::parse_type_atom`'s own documented simplification - no
        // per-parameter/return signature is tracked) - the same
        // information loss `Object`'s own case just above already has to
        // work around, just one layer earlier. Without this,
        // `subtype(Callable, Closure)` (correctly `false` in general -
        // not every callable is a `Closure` instance) would wrongly read
        // as "the doc member can't be right," discarding it even when
        // the native hint is exactly `Closure` (e.g. a `null|Closure
        // $predicate` parameter documented as `(Closure(Tv): bool)|null`
        // resolving to bare `null` once the `Closure(...)`-shaped member
        // is filtered out).
        Type::Callable(_) => true,
        // A conditional return type (`($p is X ? A : B)`) is possible if
        // either branch is - each call site picks (or unions) its own.
        // Checked branch-wise through this same function, so a branch
        // naming a template or a class is trusted the same way a plain
        // one is, instead of failing `subtype`'s name-equality object
        // check against the native class.
        Type::Conditional {
            if_true, if_false, ..
        } => {
            member_possible_given_native(if_true, native_ty)
                || member_possible_given_native(if_false, native_ty)
        }
        // An array's key is never checked against `native_ty`: a native
        // `array` hint carries no key information to contradict, so only
        // the value side is checked against the native's own.
        Type::Array(_, elem) => match native_ty {
            Type::Array(_, native_elem) => member_possible_given_native(elem, native_elem),
            _ => types::Relation::structural().subtype(m, native_ty),
        },
        // Same reasoning as the plain `Type::Array` arm just above, for
        // `list<T>`/`non-empty-array<T>`/`non-empty-list<T>`'s own real
        // `Intersection([Array(key, elem), Accessory(_), ...])`
        // representation - an accessory marker never affects
        // whether the member is "possible" given a bare native `array`
        // hint (which states nothing about list-ness/non-emptiness
        // either), so this only needs to find the `Array` member inside
        // and re-run the same key-ignoring check on it.
        Type::Intersection(members) => match (
            members.iter().find_map(|inner| match inner {
                Type::Array(_, elem) => Some(elem.as_ref()),
                _ => None,
            }),
            native_ty,
        ) {
            (Some(elem), Type::Array(_, native_elem)) => {
                member_possible_given_native(elem, native_elem)
            }
            (Some(_), _) => types::Relation::structural().subtype(m, native_ty),
            // A `Closure(...)` shape (`\Closure&callable(...)`) or any
            // other object intersection: possible whenever one member is.
            (None, _) => members
                .iter()
                .any(|inner| member_possible_given_native(inner, native_ty)),
        },
        _ => types::Relation::structural().subtype(m, native_ty),
    }
}
