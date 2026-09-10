use crate::ast::{Expr, ExprKind, Literal};
use crate::types::{self, Type};

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
pub(crate) fn reconcile_with_native(doc_ty: Type, native_ty: Type) -> Type {
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
// A native `void` return hint admits no value at all - not even a named
// class or callable, both otherwise deliberately trusted (see the
// `Object`/`Callable` arms below) since this function usually can't
// structurally disprove them for a real, non-`void` native hint. `void`
// needs no such leniency: it's never legitimately narrowed by *any* doc
// type, so this is checked before the general `Object`/`Callable`
// permissiveness below ever gets a chance to wrongly keep a doc member
// real PHPStan would always discard in favor of the native `void`
// (`TypehintHelper::decideType`'s own `$type->isSuperTypeOf($phpDocType)`
// is unconditionally `no()` for `VoidType`). Real corpus false positive
// this fixes: `PhpParser\NodeVisitor::enterNode()`'s own `@return
// null|int|Node|Node[]` doc backfilling straight through a native `void`
// override two ancestors down (`NodeVisitorAbstract::enterNode()`'s own
// untyped, undocumented passthrough in between) - the `Node` member
// survived via the `Object => true` rule below with nothing to stop it,
// wrongly demanding a real `Node` return from a function whose own,
// authoritative native signature is `void`.
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
        Type::Object(name, _) if name == "resource" => types::is_subtype(m, native_ty),
        // `"$$valueof"` (`types::parse_type_keyword_or_class`'s own
        // `value-of<T>`-on-an-enum sentinel) is deliberately trusted here
        // exactly like a genuine (if unverifiable) class reference -
        // `Registry::resolve_deferred_value_of`'s own later, whole-
        // registry pass (see its own doc comment) restores the identical
        // "fall back to native when the resolved doc type turns out to be
        // Mixed" behavior this function would otherwise have given for
        // free, once the sentinel's real value is actually known (a
        // narrower value than the generic Object case can safely give
        // here, this early in the pipeline).
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
        // Phase 74 corpus find: an array's own *key* type was checked
        // against `native_ty` via the same full `is_subtype` as every
        // other member, including its key - fine for a concrete key
        // (`array<string, int>`'s `string` really is a subtype of
        // `int|string`), but this function runs *before*
        // `mark_template_params` ever gets a chance to substitute a
        // `@template`-bound key placeholder (`array<TKey, TValue>`,
        // `TKey` still a bare, unresolved `Object("TKey", [])` at this
        // point) - `is_subtype(Object("TKey", []), int|string)` is
        // correctly `false` for an arbitrary class, so the *entire*
        // array member was discarded in favor of the bare native
        // `array`, silently losing `TValue` too even though it would
        // have resolved to a perfectly legitimate template param one
        // step later. A native `array` hint carries no key information
        // to contradict in the first place (PHP's own type system has
        // no generics), so this only ever checks the *value* side
        // against native's own - the key is always trusted as-is.
        Type::Array(_, elem) => match native_ty {
            Type::Array(_, native_elem) => member_possible_given_native(elem, native_elem),
            _ => types::is_subtype(m, native_ty),
        },
        // Same reasoning as the plain `Type::Array` arm just above, for
        // `list<T>`/`non-empty-array<T>`/`non-empty-list<T>`'s own real
        // `Intersection([Array(key, elem), Accessory(_), ...])`
        // representation (Phase 14) - an accessory marker never affects
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
            _ => types::is_subtype(m, native_ty),
        },
        _ => types::is_subtype(m, native_ty),
    }
}
