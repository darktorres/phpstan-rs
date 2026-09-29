//! A declared type from its PHPDoc and native parts: PHPStan's
//! `TypehintHelper::decideType`.

use crate::{Relation, Repr, Type};

impl Type {
    /// Filters this PHPDoc type's members down to whichever are subtypes of
    /// `native_ty` - see `resolve_type`'s own doc comment for the reasoning.
    /// Falls back to `native_ty` itself when *no* doc member survives (rather
    /// than collapsing to `Mixed`), so a doc type that's entirely stale
    /// (`finfo_file`'s bare `resource` against a modern `finfo` native) still
    /// resolves to the real, useful native type instead of losing all
    /// precision.
    pub fn reconcile_with_native(self, native_ty: Type) -> Type {
        let members: Vec<Type> = match self.into_repr() {
            Repr::Union(members) => members,
            other => vec![Type(other)],
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
        // `Repr::ValueOf` (`value-of<T>` on a still-unbound `@template`):
        // `Registry::resolve_deferred_value_of_with_native_fallback`
        // runs *after* this function, expects to find the `ValueOf` alone
        // (not already unioned with extra native members this function
        // doesn't yet know will turn out unneeded), and does its own,
        // separate "resolved to bare `Mixed`? use the native type instead"
        // swap once its real value is known - unioning native
        // members in early would corrupt that later check's own `resolved ==
        // Repr::Mixed(false)` test (a real regression caught by this project's own
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
        if let Type(Repr::Union(native_members)) = &native_ty
            && !kept.is_empty()
            && !kept.iter().any(Type::contains_deferred)
            && !kept
                .iter()
                .any(|k| matches!(k, Type(Repr::Conditional { .. })))
            && !kept.iter().any(Type::references_template)
        {
            for nm in native_members {
                if native_member_addable(nm)
                    && !kept
                        .iter()
                        .any(|k| Relation::structural().is_super_type_of(nm, k).yes())
                {
                    kept.push(nm.clone());
                }
            }
        }
        match kept.len() {
            0 => native_ty,
            1 => kept.into_iter().next().unwrap(),
            _ => Type(Repr::Union(kept)),
        }
    }
}

/// Whether a native union member is safe for `reconcile_with_native` to
/// add back on its own "the docblock never addressed this alternative"
/// theory - deliberately excludes *every* `Repr::Object` native member
/// and the `self`/`static`/`parent` class references (a doc `static`
/// already covers a native `self`), including the `object`/`resource`
/// pseudo-types: a doc member naming a
/// real *subclass* of that native class, or a plain instance covered by
/// the native `object` pseudo-type's own "accepts any object" leniency,
/// or (for `Closure` specifically) a `Closure(...)`-shaped doc member
/// `refine_callable_member` above deliberately leaves as a bare
/// `Repr::Callable` rather than the native `Object("Closure", _)` itself,
/// all genuinely *did* address that native alternative - but
/// this runs with `Relation::structural()`, which knows no class
/// hierarchy (the registry is still being built - see
/// `member_possible_given_native`), so it can't tell. Wrongly adding a
/// covered class-named member back produces a real, new false positive
/// (confirmed twice over a first pass of this fix: the `object`
/// pseudo-type case broke `a_call_through_a_method_scoped_backfilled_template_binds_t_from_the_real_argument`/
/// `a_class_string_template_parameter_binds_from_a_literal_naming_a_real_class_through_an_ordinary_call`/
/// `an_anonymous_class_inherits_a_method_scoped_template_parameterized_type`
/// with a spurious extra `object` alternative, and a narrower attempt
/// that special-cased only `Closure`-vs-`Callable` broke nothing new but
/// left the `object` case unguarded) - strictly worse than this
/// mechanism's own pre-existing gap of simply not backfilling that one
/// alternative, so no `Repr::Object` native member is ever added back
/// here; only the non-Object cases this function can decide with no
/// hierarchy needed (the actual target shape - a native `array|string`
/// refined by `@param array<int, string>` - is one of these).
fn native_member_addable(native_member: &Type) -> bool {
    !matches!(
        native_member,
        Type(Repr::Object(_, _))
            | Type(Repr::ObjectWithoutClass)
            | Type(Repr::SelfRef(_))
            | Type(Repr::StaticRef(_))
            | Type(Repr::This)
            | Type(Repr::ParentRef)
    )
}

/// A surviving `Repr::Callable` member (kept only because
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
    if !matches!(m, Type(Repr::Callable(None))) {
        return m;
    }
    match native_ty {
        Type(Repr::Object(_, _)) | Type(Repr::ObjectWithoutClass) => native_ty.clone(),
        Type(Repr::Union(members)) => {
            match members
                .iter()
                .filter(|t| **t != Type::null())
                .collect::<Vec<_>>()
                .as_slice()
            {
                [Type(Repr::Object(_, _)) | Type(Repr::ObjectWithoutClass)] => members
                    .iter()
                    .find(|t| **t != Type::null())
                    .cloned()
                    .unwrap_or(m),
                _ => m,
            }
        }
        _ => m,
    }
}

/// Whether a docblock-derived member `m` could still be real given
/// `native_ty`. `Relation::structural()` answers reliably for anything
/// except a named class/interface (`Repr::Object`), whose subclasses it
/// can't see: there is no class hierarchy to consult at the point this
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
// `Repr::Never` is exempted from this exclusion - real PHPStan special-
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
    syntax::stack::guard(|| {
        if *native_ty == Type::void() && *m != Type::void() && *m != Type::never() {
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
            Type(Repr::Object(_, _))
            | Type(Repr::TemplateParam(_))
            | Type(Repr::ObjectWithoutClass)
            | Type(Repr::ValueOf(_))
            | Type(Repr::KeyOf(_))
            | Type(Repr::OffsetAccess(..))
            | Type(Repr::ClassConstant { .. }) => true,
            // `@return static` under a native `self`/`static`/class hint: the
            // late-bound class is always an instance of the declaring one.
            Type(Repr::StaticRef(_)) | Type(Repr::This) | Type(Repr::SelfRef(_)) => !matches!(
                native_ty,
                Type(Repr::Int)
                    | Type(Repr::Float)
                    | Type(Repr::String)
                    | Type(Repr::Bool)
                    | Type(Repr::Array(..))
                    | Type(Repr::Null)
            ),
            // A callable doc member (`callable(int): string`) may be a
            // `Closure` under a native `Closure` hint, though not every
            // callable is one.
            Type(Repr::Callable(_)) => true,
            // A conditional return type (`($p is X ? A : B)`) is possible if
            // either branch is - each call site picks (or unions) its own.
            // Checked branch-wise through this same function, so a branch
            // naming a template or a class is trusted the same way a plain
            // one is, instead of failing the structural relation's
            // hierarchy-blind object check against the native class.
            Type(Repr::Conditional {
                if_true, if_false, ..
            }) => {
                member_possible_given_native(if_true, native_ty)
                    || member_possible_given_native(if_false, native_ty)
            }
            // An array's key is never checked against `native_ty`: a native
            // `array` hint carries no key information to contradict, so only
            // the value side is checked against the native's own.
            Type(Repr::Array(_, elem)) => match native_ty {
                Type(Repr::Array(_, native_elem)) => {
                    member_possible_given_native(elem, native_elem)
                }
                _ => Relation::structural().is_super_type_of(native_ty, m).yes(),
            },
            // Same reasoning as the plain `Repr::Array` arm just above, for
            // `list<T>`/`non-empty-array<T>`/`non-empty-list<T>`'s own real
            // `Intersection([Array(key, elem), Accessory(_), ...])`
            // representation - an accessory marker never affects
            // whether the member is "possible" given a bare native `array`
            // hint (which states nothing about list-ness/non-emptiness
            // either), so this only needs to find the `Array` member inside
            // and re-run the same key-ignoring check on it.
            Type(Repr::Intersection(members)) => match (
                members.iter().find_map(|inner| match inner {
                    Type(Repr::Array(_, elem)) => Some(elem.as_ref()),
                    _ => None,
                }),
                native_ty,
            ) {
                (Some(elem), Type(Repr::Array(_, native_elem))) => {
                    member_possible_given_native(elem, native_elem)
                }
                (Some(_), _) => Relation::structural().is_super_type_of(native_ty, m).yes(),
                // A `Closure(...)` shape (`\Closure&callable(...)`) or any
                // other object intersection: possible whenever one member is.
                (None, _) => members
                    .iter()
                    .any(|inner| member_possible_given_native(inner, native_ty)),
            },
            _ => Relation::structural().is_super_type_of(native_ty, m).yes(),
        }
    })
}
