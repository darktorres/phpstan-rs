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
    // The native, no-docblock-merge type this parameter alone declares -
    // the exact `ty`-vs-`ty_native` split `FunctionSig::return_type`/
    // `return_type_native` already has, and for the identical reason
    // (see that field's own doc comment): real PHP/PHPStan only enforce
    // method-override Liskov-compatibility at this native level by
    // default. `check_one_method_override`'s own param-narrowing check
    // used to compare the merged `ty` instead - real corpus false
    // positive, the extremely common Rector/PHPStan-`Collector<T>` idiom
    // of narrowing a parameter via `@param` alone while leaving a wide
    // native hint (`refactor(Node $node)`, narrowed only via `@param
    // SpecificNode $node`) was wrongly flagged as an LSP violation real
    // PHPStan doesn't, by default, check at all. Every other reader of
    // `ty` correctly wants the merged, docblock-refined type and must
    // keep using it unchanged.
    pub ty_native: Type,
    pub variadic: bool,
    pub has_default: bool,
    pub by_ref: bool,
    // Phase 87 (PHPDoc-inheritance backfill): starts as `true` only when
    // this parameter's own `@param` tag was present on this
    // declaration's own docblock (i.e. `from_decl`'s own `doc_ty` was
    // `Some`) - `false` means `ty` is really just `ty_native` (or a
    // phpDoc-less default), making this position eligible for
    // `Registry`'s own `flatten_phpdoc_inheritance` pass to backfill
    // from an ancestor, mirroring real PHPStan's `ResolvedPhpDocBlock::
    // mergeOneParentParamTags` (`array_key_exists($name, $paramTags)`).
    // That pass also *sets* this to `true` once it successfully
    // backfills `ty` from an ancestor - a position is then "documented"
    // (own or inherited) for the purposes of any *further* descendant's
    // own lookup, which is what makes multi-level transitivity resolve
    // one level per fixed-point pass; see that pass's own doc comment
    // for why an ancestor's own bit being `false` (nothing real to
    // inherit, not even from a bare native hint) must gate the merge
    // rather than the ancestor's `ty` being merely present.
    pub has_own_doc: bool,
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
    // Phase 87 (PHPDoc-inheritance backfill): the `ParamSig::has_own_doc`
    // equivalent for the return position - starts `true` only when this
    // declaration's own docblock carried an `@return` tag (`from_decl`'s
    // own `doc_return` was `Some`), mirroring real PHPStan's
    // `ResolvedPhpDocBlock::mergeOneParentReturnTag`'s own trigger
    // (`$returnTag !== null`). See `ParamSig::has_own_doc`'s own doc
    // comment for why `flatten_phpdoc_inheritance` also sets this to
    // `true` once it backfills `return_type` from an ancestor.
    pub return_has_own_doc: bool,
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
    // Phase 64 follow-up: PHP 8.3's `#[\Override]` marker - `true` when
    // this declaration carries one. Stored here (mirroring `no_discard`
    // exactly, including its "short-name-only, no namespace resolution"
    // matching convention) rather than checked straight off a fresh
    // `MethodDecl`'s own AST, because a method needs this checked in the
    // context of whichever *using class*'s ancestor chain applies - a
    // trait's own method carrying `#[Override]` is only ever a real
    // fatal error relative to a class that later `use`s it (confirmed
    // directly: the same trait method is fine when the using class has a
    // real ancestor to satisfy it, and fatals - attributed to the
    // *using class's* own method name, not the trait's - when it
    // doesn't), so this needs to survive `flatten_traits`' merge the
    // same way `no_discard` already does.
    pub has_override_attr: bool,
    // Phase 39: `@deprecated` on this function/method's own doc comment -
    // see `docblock::DocBlock::deprecated`'s own doc comment for what it
    // means and how it's gated. Deliberately scoped to the function/
    // method's own tag only, unlike `is_internal`'s "declaring class
    // also counts" fallback - whether a class-level `@deprecated`
    // implicitly deprecates every member too isn't something this
    // project could confirm against real PHPStan's own source (the
    // actual deprecation-detection logic lives in `ondrejmirtes/
    // better-reflection`, not vendored here), so it's left unclaimed
    // rather than guessed.
    pub deprecated: Option<String>,
    // Phase 42's own self-contained first slice: `@throws` on this
    // function/method's own doc comment, resolved and unioned across
    // every tag present (`docblock::DocBlock::throws`'s own doc comment
    // explains why it's a `Vec` there) - `None` when no `@throws` tag
    // exists at all, distinct from a real, resolvable-but-empty case
    // (there isn't one - a tag with no type text simply isn't recorded
    // by `docblock::parse` in the first place). Not yet consumed by any
    // checker code - the base model this phase's later stages (a real
    // body walk collecting `throw` statements, transitive union across
    // the call graph, the opt-in "missing checked exception" rule
    // family) build on.
    pub throws: Option<Type>,
    // `@phpstan-assert`-family tags (`docblock::DocBlock::assert_if_true`'s
    // own doc comment has the full picture), resolved into a real `Type`
    // plus a classified `AssertTarget` - a tag whose own target text
    // doesn't match a real parameter of this declaration (typo'd, or a
    // target shape this project doesn't classify at all, e.g. a method-
    // call or array-index target) is silently dropped rather than kept
    // half-resolved, the usual "don't guess" leniency.
    pub assert_if_true: Vec<AssertInfo>,
    pub assert_if_false: Vec<AssertInfo>,
    pub assert: Vec<AssertInfo>,
}

/// Which real, checkable thing a `@phpstan-assert`-family tag's own
/// `$target` text names - classified once here (registry-build time,
/// when a declaration's own parameter list is available to match a bare
/// name against) rather than carried as raw text into the checker.
/// Deliberately narrow: a method-call target (`$this->parseError()`,
/// a real corpus shape) or any array-index/nested-chain target isn't
/// classified into any of these and the whole tag is dropped - this
/// project's narrowing machinery has no way to key a call's own return
/// value the way it keys a variable/property, and guessing wrong here
/// risks a false assertion rather than just a missed one.
#[derive(Debug, Clone, PartialEq)]
pub enum AssertTarget {
    /// `$name` matching one of this declaration's own parameters.
    Param(String),
    /// Bare `$this` - the assertion narrows the call's own receiver.
    ThisSelf,
    /// `$this->propName` - a single-level property access on the
    /// receiver.
    ThisProp(String),
}

#[derive(Debug, Clone)]
pub struct AssertInfo {
    pub negated: bool,
    pub ty: Type,
    pub target: AssertTarget,
}

/// Classifies `tag.target`'s own raw text into a real `AssertTarget`,
/// `None` for anything this project doesn't model (see `AssertTarget`'s
/// own doc comment) - `param_names` is this declaration's own parameter
/// name list, checked so a target naming a parameter that doesn't
/// actually exist (a stale doc comment, or simply a name this project
/// failed to match for some other reason) isn't silently misread as
/// some other shape instead.
fn classify_assert_target(target: &str, param_names: &[&str]) -> Option<AssertTarget> {
    if target == "$this" {
        return Some(AssertTarget::ThisSelf);
    }
    if let Some(prop) = target.strip_prefix("$this->")
        && !prop.is_empty()
        && prop.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return Some(AssertTarget::ThisProp(prop.to_string()));
    }
    let name = target.strip_prefix('$')?;
    if param_names.contains(&name) {
        return Some(AssertTarget::Param(name.to_string()));
    }
    None
}

/// Resolves every `@phpstan-assert`-family tag on `doc` into a real
/// `AssertInfo` list, dropping any tag whose own target doesn't classify
/// (`classify_assert_target`) or whose own type text doesn't parse into
/// anything meaningful. Shares the exact same type-resolution pipeline
/// (`mark_template_params` then `resolve_names_in_type`) every other
/// declared type on this same declaration already goes through.
fn resolve_assert_tags(
    tags: &[docblock::AssertTag],
    param_names: &[&str],
    template_params: &HashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> Vec<AssertInfo> {
    tags.iter()
        .filter_map(|tag| {
            let target = classify_assert_target(&tag.target, param_names)?;
            let ty = types::parse_type_string(&tag.ty);
            let ty = mark_template_params(ty, template_params);
            let ty = resolve_names_in_type(ty, ctx);
            Some(AssertInfo {
                negated: tag.negated,
                ty,
                target,
            })
        })
        .collect()
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
/// Rewrites every `Object(name, _)` in `ty` whose `name` matches a key in
/// `aliases` into that alias's own (already fully-resolved) `Type` -
/// `ClassInfo::from_decl`'s own local `@phpstan-type` support. Mirrors
/// `mark_template_params`'s exact recursive shape (both are a one-time
/// post-processing walk over an already-parsed `Type`), but must run
/// strictly *after* `resolve_names_in_type` has already resolved `ty`
/// itself, with `aliases` keyed by each alias's own *resolved* FQN (the
/// same name a bare, unqualified reference to it would resolve to) -
/// substituting first and re-resolving the (already-resolved) result
/// afterward double-qualifies every name the alias's own value already
/// carries (a real bug caught during this feature's own first attempt:
/// `json-schema`'s own unparsed `int-mask-of<...>` pseudo-type tripled
/// into `JsonSchema\Entity\JsonSchema\Entity\JsonSchema\Entity\int-mask-of`
/// this way). `aliases` itself needs no equivalent care to avoid
/// re-resolving *its own* values further - each entry is inserted once,
/// already fully resolved, and never touched by `resolve_names_in_type`
/// again after that.
pub(crate) fn substitute_type_aliases(ty: Type, aliases: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Object(name, _) if aliases.contains_key(&name) => aliases[&name].clone(),
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
pub(crate) fn build_type_aliases(
    class_doc: Option<&DocBlock>,
    template_params: &HashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> HashMap<String, Type> {
    let Some(class_doc) = class_doc else {
        return HashMap::new();
    };
    let raw: HashMap<String, Type> = class_doc
        .type_aliases
        .iter()
        .map(|(name, ty_text)| {
            let ty = types::parse_type_string(ty_text);
            let ty = mark_template_params(ty, template_params);
            let ty = resolve_names_in_type(ty, ctx);
            (ctx.resolve(name), ty)
        })
        .collect();
    raw.iter()
        .map(|(name, ty)| (name.clone(), substitute_type_aliases(ty.clone(), &raw)))
        .collect()
}

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

/// Phase 64 follow-up: same idea as `is_no_discard_attribute_name` just
/// above, for PHP 8.3's `#[\Override]` marker - real PHPStan's own
/// `OverridingMethodRule::hasOverrideAttribute` matches the identical way
/// (`$attr->name->toLowerString() === 'override'` - short-name-only,
/// case-insensitive, no fully-resolved namespace check), deliberately
/// *not* the stricter engine-level namespace resolution the real PHP
/// runtime itself performs for this same attribute (confirmed directly:
/// a userland `App\Override` class doesn't trigger the real fatal error
/// at all) - this project follows real PHPStan's own rule here, not the
/// engine.
pub(crate) fn is_override_attribute_name(name: &str) -> bool {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("Override")
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
    // `class_type_aliases` is this method's own declaring class's
    // `@phpstan-type` table (`ClassInfo::from_decl`'s own `build_type_
    // aliases`) - empty for a plain top-level function or an enum's own
    // synthesized method (neither has a class-level docblock these could
    // meaningfully come from). A method's *own* docblock declaring
    // further `@phpstan-type`s of its own isn't supported - real
    // PHPStan allows this too, but every real corpus shape found so far
    // (`json-schema`'s own `ErrorBag`) declares aliases once, on the
    // class, referenced from many members - not attempted here.
    pub(crate) fn from_decl(
        f: &FunctionDecl,
        ctx: &NameContext,
        class_templates: &HashMap<String, TemplateParam>,
        class_type_aliases: &HashMap<String, Type>,
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
                let ty = substitute_type_aliases(ty, class_type_aliases);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                // Mirrors `ty` just above exactly, but with `doc_ty`
                // forced to `None` - the same "native signature alone,
                // no docblock refinement" split `return_type_native`
                // already has. Still passes through `apply_implicit_
                // nullable_default`, unlike `return_type_native` (which
                // has no equivalent default-value-implies-nullable rule
                // to apply) - that transformation reflects a genuinely
                // native-level PHP fact (an untyped-`?` native hint with
                // a `null` default is implicitly nullable), independent
                // of any docblock merge.
                let ty_native = resolve_type(None, p.type_hint.as_deref());
                let ty_native = mark_template_params(ty_native, &template_params);
                let ty_native = resolve_names_in_type(ty_native, ctx);
                let ty_native = apply_implicit_nullable_default(ty_native, p.default.as_ref());
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
                    ty_native,
                    variadic: p.variadic,
                    has_default,
                    by_ref: p.by_ref,
                    has_own_doc: doc_ty.is_some(),
                }
            })
            .collect();
        let params = dedup_versioned_params(params);

        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_deref());
        let return_type = resolve_type(doc_return, f.return_type.as_deref());
        let return_type = mark_template_params(return_type, &template_params);
        let return_type = resolve_names_in_type(return_type, ctx);
        let return_type = substitute_type_aliases(return_type, class_type_aliases);

        let return_type_native = resolve_type(None, f.return_type.as_deref());
        let return_type_native = mark_template_params(return_type_native, &template_params);
        let return_type_native = resolve_names_in_type(return_type_native, ctx);

        let no_discard = f
            .attrs
            .iter()
            .any(|a| is_no_discard_attribute_name(&a.name));
        let has_override_attr = f.attrs.iter().any(|a| is_override_attribute_name(&a.name));

        let is_internal = doc.as_ref().is_some_and(|d| d.internal);
        let deprecated = doc.as_ref().and_then(|d| d.deprecated.clone());

        let throws =
            doc.as_ref()
                .into_iter()
                .flat_map(|d| &d.throws)
                .fold(None::<Type>, |acc, raw| {
                    let ty = types::parse_type_string(raw);
                    let ty = mark_template_params(ty, &template_params);
                    let ty = resolve_names_in_type(ty, ctx);
                    Some(match acc {
                        Some(prev) => union_types(prev, ty),
                        None => ty,
                    })
                });

        let param_names: Vec<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
        let (assert_if_true, assert_if_false, assert) = doc
            .as_ref()
            .map(|d| {
                (
                    resolve_assert_tags(&d.assert_if_true, &param_names, &template_params, ctx),
                    resolve_assert_tags(&d.assert_if_false, &param_names, &template_params, ctx),
                    resolve_assert_tags(&d.assert, &param_names, &template_params, ctx),
                )
            })
            .unwrap_or_default();

        FunctionSig {
            params,
            return_type,
            return_type_native,
            return_has_own_doc: doc_return.is_some(),
            no_discard,
            visibility,
            is_internal,
            has_override_attr,
            deprecated,
            throws,
            assert_if_true,
            assert_if_false,
            assert,
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
        let has_override_attr = self.has_override_attr || other.has_override_attr;
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
                    ty_native: union_types(a.ty_native, b.ty_native),
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
                    // Permissive, same direction as `has_default` above:
                    // if either real overload's own docblock already
                    // documents this position, the merged signature
                    // shouldn't look like a further-inheritance candidate.
                    has_own_doc: a.has_own_doc || b.has_own_doc,
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
            // Same permissive "either overload's own marker wins" as
            // `no_discard`/`is_internal` below.
            return_has_own_doc: self.return_has_own_doc || other.return_has_own_doc,
            no_discard,
            // Both real overloads share one declaration (a stub-modeled
            // PHP-version split of the same underlying function/method,
            // never two independently-declared visibilities) - either
            // side's own value is equally correct.
            visibility: self.visibility,
            // Same permissive "either overload's own marker wins" as
            // `no_discard` just above.
            is_internal: self.is_internal || other.is_internal,
            has_override_attr,
            // Same permissive "either overload's own marker wins" as
            // `no_discard`/`is_internal` above.
            deprecated: self.deprecated.or(other.deprecated),
            // Both real overloads can genuinely throw whatever either
            // one's own `@throws` documents - a caller has to be
            // prepared for either, whichever one actually runs.
            throws: match (self.throws, other.throws) {
                (Some(a), Some(b)) => Some(union_types(a, b)),
                (Some(t), None) | (None, Some(t)) => Some(t),
                (None, None) => None,
            },
            // Both real overloads' own assertions still hold regardless
            // of which one actually ran - simple concatenation, not a
            // type union, since each entry is an independent claim about
            // a target, not a competing type for the same one.
            assert_if_true: [self.assert_if_true, other.assert_if_true].concat(),
            assert_if_false: [self.assert_if_false, other.assert_if_false].concat(),
            assert: [self.assert, other.assert].concat(),
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
