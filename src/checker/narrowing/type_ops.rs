use crate::checker::narrowing::accessory_range::upgrade_array_to_non_empty;
use crate::registry::Registry;
use crate::types::{AccessoryKind, Type, is_subtype};

/// Narrows `original` to `target` along the branch where a check for
/// `target` succeeded: if `original` is a union, keep only the member(s)
/// that match; otherwise just trust the more specific `target`.
///
/// Phase 32 Finding L: a union member is kept when it's a *subtype* of
/// `target`, not just when it's structurally identical to it - exact
/// equality is the wrong tool for "is this the kind of thing being
/// narrowed": `is_array()`'s registered target is generic
/// `Type::Array(Mixed)`, so a specific-element `Array(String)` member
/// (from a docblock-refined param type) used to survive an `is_array()`
/// exclusion untouched, since `Array(String) != Array(Mixed)` even
/// though every value it describes plainly *is* an array. Deliberately
/// scoped to this `Union(members)` branch only, not the `_ => target.
/// clone()` fallback below it for a non-`Union` `original` - that
/// fallback already unconditionally trusts `target` regardless of
/// `original`'s own content, so there's nothing for `is_subtype` to
/// usefully change there.
/// `instanceof`'s own true-branch narrowing - deliberately not
/// `narrow_type_to` above, which unconditionally trusts `target` for a
/// non-`Union` `original`. That's correct for a scalar predicate
/// (`is_int($x)` on `int|string` needs nothing more than plain `int` -
/// mutually exclusive scalar types can't combine), but wrong for
/// `instanceof` against an unrelated *sibling* interface/class: `$view:
/// View` (an interface), `if ($view instanceof HasPageAssets)` (a
/// second, unrelated interface neither extending nor implementing the
/// other) needs `View&HasPageAssets`, not bare `HasPageAssets` -
/// discarding `View` entirely lost real information a caller elsewhere
/// still needed, a real corpus bug (`Template\Renderer::render`'s own
/// three independent, sequential `instanceof` checks on the same `View
/// $view` parameter). Only builds a real `Intersection` when neither
/// side is already a sub/supertype of the other - the common down-cast
/// (`Base` narrowed to a known `Child`) and nullable-narrowing
/// (`?Foo` narrowed to `Foo`) cases both still resolve to a single
/// class/interface exactly as before.
pub(crate) fn narrow_instanceof_true(current: &Type, target: &Type) -> Type {
    if is_subtype(target, current) {
        return target.clone();
    }
    if is_subtype(current, target) {
        return current.clone();
    }
    // A nullable `current` (`?Socket`, i.e. `Union([Object("Socket"),
    // Null])`) can never actually be `null` once a true `instanceof`
    // check has passed - `null instanceof AnyClass` is always `false` in
    // real PHP - so `null` is dropped before deciding whether what's
    // left is object-like enough to intersect with. Without this, a
    // nullable property/variable checked against an *unrelated*
    // interface (real corpus shape: amphp/http-client's own
    // `Http1Connection::watchIdleConnection()`, `?Socket` checked via
    // `instanceof ResourceStream`) fell all the way through to the
    // simpler "just trust the checked class" branch below - discarding
    // `Socket` entirely instead of intersecting down to `Socket&
    // ResourceStream` - so every one of `Socket`'s own members (real
    // shape: a later `$socket?->read()`) looked confirmed-missing on
    // the resulting bare `ResourceStream`.
    let non_null = narrow_type_excluding(current, &Type::Null);
    // Only combined into a real Intersection when every branch of
    // `current` (once stripped of any nullability, per above) is itself
    // provably object-like - anything else (`Mixed`, or a scalar/array
    // type that could never actually satisfy a true `instanceof` in the
    // first place, a sign this project's own upstream inference already
    // went wrong somewhere *else*) falls back to the simpler "just trust
    // the checked class" behavior instead of risking compounding an
    // unrelated bug into a nonsensical type like `null&ImageId`. Found
    // live: `$id = is_array($row) ? ($row['id'] ?? null) : null;` was
    // itself mis-inferred as bare `Null` (a real, separate gap, not
    // narrowed away here) - this guard is what kept this fix's own
    // regression check from compounding that into exactly this
    // nonsense. Recurses through `Union`/`Intersection` so a *second*,
    // independent `instanceof` check against the same variable (real
    // shape: `Template\Renderer::render`'s own three sequential checks)
    // still combines correctly once the first check's own merge has
    // already turned `current` into a real union of object-like
    // branches.
    if is_object_like(&non_null) {
        Type::Intersection(vec![non_null, target.clone()])
    } else {
        target.clone()
    }
}

fn is_object_like(ty: &Type) -> bool {
    match ty {
        Type::Object(..) => true,
        Type::Union(members) | Type::Intersection(members) => members.iter().all(is_object_like),
        _ => false,
    }
}

/// `registry` is needed for the `Union` arm's own member filter below -
/// plain `types::is_subtype` has no class-hierarchy awareness at all
/// (`types.rs` doesn't depend on `registry.rs`), so it only ever matched
/// a union member against `target` by exact structural equality. A real
/// corpus regression this missed: Doctrine ORM's own `@phpstan-assert-
/// if-true ToOneAssociationMapping $this` (an interface every concrete
/// mapping class implements only *transitively*, through an intermediate
/// abstract class) narrowed the full six-member union down to *zero*
/// matching members, falling through to the `target.clone()` case below
/// and discarding every member's own additional properties/methods
/// entirely - `Registry::is_assignable` is the identical hierarchy-aware
/// check `is_subtype` is documented as a non-hierarchy-aware subset of,
/// so using it here can only keep strictly more genuinely-compatible
/// members than before, never fewer.
pub(crate) fn narrow_type_to(registry: &Registry, original: &Type, target: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| registry.is_assignable(m, target))
                .cloned()
                .collect();
            match kept.len() {
                0 => target.clone(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        // Round 14 of the Piwigo corpus cleanup: a real, general
        // unsoundness bug - blindly returning `target` discarded
        // `original` even when `original` was already narrower, wrongly
        // *widening* it. Real corpus shape: `$image_id === $this->
        // sessionService->getRefererImageId()`, where `$image_id` is a
        // definite `int` and `getRefererImageId()` returns `int|null` -
        // the pre-existing code overwrote `$image_id` itself with the
        // broader `int|null` inside the `===` true branch, corrupting
        // every downstream use for the rest of the method. If `original`
        // is already at least as narrow as `target`, real equality
        // proves nothing new about it - keep it unchanged; only fall
        // back to `target` when it's the narrower side (the literal-
        // narrowing case this fallback exists for, `$x === 'foo'`).
        // Same hierarchy-aware upgrade as the `Union` arm above: a single
        // concrete class that only transitively implements `target` (an
        // interface inherited from an ancestor, not declared directly)
        // must still keep its own, more specific type here rather than
        // needlessly widening to the bare interface.
        _ if registry.is_assignable(original, target) => original.clone(),
        _ => target.clone(),
    }
}

/// Narrows `original` by removing `excluded` along the branch where a check
/// for it failed - the "not null" half of a `=== null` guard clause, the
/// most common narrowing pattern in real code.
///
/// Phase 32 Finding L: a union member is dropped when it's a *subtype* of
/// `excluded`, not just when it's structurally identical to it - the same
/// reasoning as `narrow_type_to` above, mirrored for exclusion (`is_array
/// (...)`'s *false*-branch exclusion of `Array(Mixed)` used to leave a
/// specific-element `Array(String)` member behind, since it was never
/// `==` to the generic target it should have matched as "an array").
///
/// Deliberately **not** applied to the non-`Union` fallback below
/// (`other if other == excluded => Type::Never`): `is_subtype`'s own
/// int-to-float argument-coercion rule (`Type::Int` is a subtype of
/// `Type::Float`) would make `narrow_type_excluding(Type::Int,
/// Type::Float)` wrongly collapse a plain, correctly-known `int` to
/// `Type::Never` - the value genuinely is an int, and its coercibility to
/// `float` for parameter-passing purposes has nothing to do with whether
/// it should be excluded here. Every existing caller of this branch
/// (`isset`'s `Null` exclusion, and similar plain-`Null` exclusions
/// elsewhere) is unaffected either way, since no other `Type` variant is
/// ever a subtype of `Null`.
/// Whether `ty` is proven to hold exactly one possible value - the only
/// kind of type it's sound to *exclude* from another type based on a
/// runtime `===`/`!==` comparison (`apply_equality_narrowing`'s own use).
/// `Null`/`True`/`False` and a literal are each their own single value;
/// everything else (`Bool`, `Int`, `String`, any other general/composite
/// type) covers multiple possible values, so knowing only "the other side
/// has this general type" - not which of its values it actually holds -
/// proves nothing about which value to rule out.
pub(crate) fn is_known_single_value(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Null
            | Type::True
            | Type::False
            | Type::IntLiteral(_)
            | Type::StringLiteral(_)
            // Phase 79: `Suit::Hearts` is exactly as single-valued as any
            // other literal - `$case !== Suit::Hearts` should exclude
            // that one case, the same way `$x !== 'a'` already excludes
            // that one string.
            | Type::EnumCase(_, _)
    ) ||
    // Found by a round of Piwigo-corpus bucketing: `$x !== []` (the same
    // idiom `$x !== ''` already gets, Phase 14) never excluded the empty
    // shape at all - an empty array literal is exactly as exhaustively
    // single-valued as `''`/a literal int, just structural rather than a
    // scalar `Type` variant.
    matches!(ty, Type::ArrayShape(fields) if fields.is_empty())
}

pub(crate) fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
    match original {
        // Recurses into `narrow_type_excluding` for each kept member,
        // rather than just cloning it, so a `String`/`Array` member
        // found *inside* a `Union` gets the same Phase 14 accessory
        // upgrade a bare one does below - behavior-preserving for every
        // other existing caller, since a member that doesn't hit a new
        // arm still falls through to the same `other.clone()` it
        // reached directly before.
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| !is_subtype(m, excluded))
                .map(|m| narrow_type_excluding(m, excluded))
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        // Phase 14: `$x !== ''` on a plain `string` - verified against
        // real PHPStan's own dumpType(). Deliberately not extended to an
        // already-Intersection-refined value (e.g. upgrading an existing
        // `non-empty-string` to `non-falsy-string` inside a nested
        // `!empty($x)` check) - a real gap, but a rare, redundant
        // double-guard pattern with no corpus evidence.
        Type::String if *excluded == Type::StringLiteral(String::new()) => {
            Type::Intersection(vec![
                Type::String,
                Type::Accessory(AccessoryKind::NonEmptyString),
            ])
        }
        // `$x !== []` on a plain `array`/`list` - the same idiom as the
        // `String` arm above, verified against real PHPStan's own
        // dumpType(): both a plain `array<T>` and an already-`list<T>`
        // upgrade to their own non-empty form. Shares `upgrade_array_to_
        // non_empty` with `!empty($x)`/`count($x) > 0` so the three
        // "prove this array is non-empty" paths can't drift apart.
        Type::Array(..) | Type::Intersection(_) if matches!(excluded, Type::ArrayShape(fields) if fields.is_empty()) => {
            upgrade_array_to_non_empty(original)
        }
        // Real PHP's own `Traversable` is a marker interface userland
        // code can never implement directly (`class Foo implements
        // Traversable {}` is a real fatal error, confirmed via `php -r`)
        // - only through one of its exactly two direct subinterfaces,
        // `Iterator` and `IteratorAggregate`. Excluding one therefore
        // structurally *proves* the other, unlike the general "we can't
        // disprove anything more precise than the type itself" fallback
        // below - real PHPStan special-cases this identical relationship.
        // Real corpus false positive this fixes: `Symfony\Component\
        // Messenger\MessageBus::dispatch()`'s own `while ($x instanceof
        // IteratorAggregate) { $x = $x->getIterator(); }` loop-exit type
        // staying bare `Traversable` (missing `rewind()`/`valid()`/
        // `current()`) instead of narrowing to the only other
        // structurally possible answer, `Iterator`.
        Type::Object(name, _)
            if name == "Traversable"
                && matches!(excluded, Type::Object(excl, _) if excl == "IteratorAggregate") =>
        {
            Type::Object("Iterator".to_string(), Vec::new())
        }
        Type::Object(name, _)
            if name == "Traversable"
                && matches!(excluded, Type::Object(excl, _) if excl == "Iterator") =>
        {
            Type::Object("IteratorAggregate".to_string(), Vec::new())
        }
        other if other == excluded => Type::Never,
        other => other.clone(),
    }
}
