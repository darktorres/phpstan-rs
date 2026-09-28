use crate::checker::narrowing::accessory_range::upgrade_array_to_non_empty;
use types::PhpString;
use types::{AccessoryKind, Relation, Type};

/// Narrows `original` to `target` along the branch where a check for
/// `target` succeeded: if `original` is a union, keep only the member(s)
/// that match; otherwise just trust the more specific `target`.
///
/// A union member is kept when it's a subtype of
/// `target`, not just when it's structurally identical to it - exact
/// equality is the wrong tool for "is this the kind of thing being
/// narrowed": `is_array()`'s registered target is generic
/// `Type::Array(Mixed)`, so a specific-element `Array(String)` member
/// (from a docblock-refined param type) must survive an `is_array()`
/// exclusion, even though `Array(String) != Array(Mixed)`. Deliberately
/// scoped to this `Union(members)` branch only, not the `_ => target.
/// clone()` fallback below it for a non-`Union` `original` - that
/// fallback already unconditionally trusts `target` regardless of
/// `original`'s own content, so there's nothing for `is_super_type_of` to
/// usefully change there.
/// `instanceof`'s own true-branch narrowing - deliberately not
/// `narrow_type_to` above, which unconditionally trusts `target` for a
/// non-`Union` `original`. That's correct for a scalar predicate
/// (`is_int($x)` on `int|string` needs nothing more than plain `int` -
/// mutually exclusive scalar types can't combine), but wrong for
/// `instanceof` against an unrelated sibling interface/class: `$view:
/// View` (an interface), `if ($view instanceof HasPageAssets)` (a
/// second, unrelated interface neither extending nor implementing the
/// other) needs `View&HasPageAssets`, not bare `HasPageAssets` -
/// discarding `View` entirely would lose real information a caller
/// elsewhere still needs (e.g. three independent, sequential
/// `instanceof` checks on the same parameter). Only builds a real
/// `Intersection` when neither
/// side is already a sub/supertype of the other - the common down-cast
/// (`Base` narrowed to a known `Child`) and nullable-narrowing
/// (`?Foo` narrowed to `Foo`) cases both still resolve to a single
/// class/interface exactly as before.
pub(crate) fn narrow_instanceof_true(relation: &Relation, current: &Type, target: &Type) -> Type {
    if relation.is_super_type_of(current, target).yes() {
        return target.clone();
    }
    if relation.is_super_type_of(target, current).yes() {
        return current.clone();
    }
    // A nullable `current` (`?Socket`, i.e. `Union([Object("Socket"),
    // Null])`) can never actually be `null` once a true `instanceof`
    // check has passed - `null instanceof AnyClass` is always `false` in
    // real PHP - so `null` is dropped before deciding whether what's
    // left is object-like enough to intersect with. Without this, a
    // nullable property/variable checked against an unrelated
    // interface (`?Socket` checked via `instanceof ResourceStream`)
    // would fall all the way through to the simpler "just trust the
    // checked class" branch below - discarding `Socket` entirely
    // instead of intersecting down to `Socket&ResourceStream` - so
    // every one of `Socket`'s own members would look confirmed-missing
    // on the resulting bare `ResourceStream`.
    let non_null = narrow_type_excluding(current, &Type::Null);
    // Only combined into a real Intersection when every branch of
    // `current` (once stripped of any nullability, per above) is itself
    // provably object-like - anything else (`Mixed`, or a scalar/array
    // type that could never actually satisfy a true `instanceof` in the
    // first place, a sign this project's own upstream inference already
    // went wrong somewhere else) falls back to the simpler "just trust
    // the checked class" behavior instead of risking compounding an
    // unrelated bug into a nonsensical type like `null&ImageId`.
    // Recurses through `Union`/`Intersection` so a second, independent
    // `instanceof` check against the same variable still combines
    // correctly once the first check's own merge has already turned
    // `current` into a real union of object-like branches.
    if is_object_like(&non_null) {
        relation.intersect(vec![non_null, target.clone()])
    } else {
        target.clone()
    }
}

fn is_object_like(ty: &Type) -> bool {
    match ty {
        Type::Object(..) | Type::ObjectWithoutClass => true,
        Type::Union(members) | Type::Intersection(members) => members.iter().all(is_object_like),
        _ => false,
    }
}

/// `relation` (the checker's, over the registry's classes) is needed for
/// the `Union` arm's own member filter below - plain `Relation::is_super_type_of`
/// has no class-hierarchy awareness at all, so it only ever matches
/// a union member against `target` by exact structural equality. An
/// interface a concrete class implements only transitively (through an
/// intermediate abstract class) would otherwise narrow the whole union
/// down to zero matching members, falling through to the
/// `target.clone()` case below and discarding every member's own
/// additional properties/methods entirely - `Relation::accepts`
/// is the identical hierarchy-aware check `is_super_type_of` is documented as
/// a non-hierarchy-aware subset of, so using it here can only keep
/// strictly more genuinely-compatible members than before, never fewer.
pub(crate) fn narrow_type_to(relation: &Relation, original: &Type, target: &Type) -> Type {
    match original {
        Type::Union(members) | Type::BenevolentUnion(members) => {
            // A member the target covers stays; one it doesn't gives way
            // to the target's members inside it (`?string` narrowed to
            // `'a'|null` is `'a'|null`).
            let target_members: &[Type] = match target {
                Type::Union(ms) | Type::BenevolentUnion(ms) => ms,
                other => std::slice::from_ref(other),
            };
            let mut kept: Vec<Type> = Vec::new();
            for m in members {
                if relation.accepts(target, m).yes() {
                    if !kept.contains(m) {
                        kept.push(m.clone());
                    }
                    continue;
                }
                for t in target_members {
                    if relation.accepts(m, t).yes() && !kept.contains(t) {
                        kept.push(t.clone());
                    }
                }
            }
            match kept.len() {
                0 => target.clone(),
                1 => kept.into_iter().next().unwrap(),
                _ => original.with_members(kept),
            }
        }
        // Blindly returning `target` would discard
        // `original` even when `original` was already narrower, wrongly
        // widening it: for `$image_id === $this->sessionService->
        // getRefererImageId()`, where `$image_id` is a definite `int`
        // and `getRefererImageId()` returns `int|null`, overwriting
        // `$image_id` with the broader `int|null` inside the `===` true
        // branch would corrupt every downstream use for the rest of the
        // method. If `original`
        // is already at least as narrow as `target`, real equality
        // proves nothing new about it - keep it unchanged; only fall
        // back to `target` when it's the narrower side (the literal-
        // narrowing case this fallback exists for, `$x === 'foo'`).
        // Same hierarchy-aware upgrade as the `Union` arm above: a single
        // concrete class that only transitively implements `target` (an
        // interface inherited from an ancestor, not declared directly)
        // must still keep its own, more specific type here rather than
        // needlessly widening to the bare interface.
        _ if relation.accepts(target, original).yes() => original.clone(),
        _ => target.clone(),
    }
}

/// Narrows `original` by removing `excluded` along the branch where a check
/// for it failed - the "not null" half of a `=== null` guard clause, the
/// most common narrowing pattern in real code.
///
/// A union member is dropped when it's a subtype of
/// `excluded`, not just when it's structurally identical to it - the same
/// reasoning as `narrow_type_to` above, mirrored for exclusion (`is_array
/// (...)`'s false-branch exclusion of `Array(Mixed)` must also drop a
/// specific-element `Array(String)` member, since it's never
/// `==` to the generic target it should match as "an array").
///
/// Deliberately **not** applied to the non-`Union` fallback below
/// (`other if other == excluded => Type::Never`): `is_super_type_of`'s own
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
            // `Suit::Hearts` is exactly as single-valued as any
            // other literal - `$case !== Suit::Hearts` should exclude
            // that one case, the same way `$x !== 'a'` already excludes
            // that one string.
            | Type::EnumCase(_, _)
    ) ||
    // `$x !== []` (the same idiom `$x !== ''` already gets) also
    // excludes the empty shape - an empty array literal is exactly as
    // exhaustively single-valued as `''`/a literal int, just structural
    // rather than a scalar `Type` variant.
    matches!(ty, Type::ArrayShape(shape) if shape.is_empty_array())
}

pub(crate) fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
    match original {
        // Recurses into `narrow_type_excluding` for each kept member,
        // rather than just cloning it, so a `String`/`Array` member
        // found inside a `Union` gets the same accessory upgrade a
        // bare one does below - a member that doesn't hit a new arm
        // still falls through to the same `other.clone()` it reached
        // directly before.
        Type::Union(members) | Type::BenevolentUnion(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| {
                    !types::Relation::structural()
                        .is_super_type_of(excluded, m)
                        .yes()
                })
                .map(|m| narrow_type_excluding(m, excluded))
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => original.with_members(kept),
            }
        }
        // `$x !== ''` on a plain `string`. Deliberately not extended to an
        // already-Intersection-refined value (e.g. upgrading an existing
        // `non-empty-string` to `non-falsy-string` inside a nested
        // `!empty($x)` check) - a real gap, but a rare, redundant
        // double-guard pattern this project doesn't attempt yet.
        Type::String if *excluded == Type::StringLiteral(PhpString::default()) => {
            Type::Intersection(vec![
                Type::String,
                Type::Accessory(AccessoryKind::NonEmptyString),
            ])
        }
        // `$x !== []` on a plain `array`/`list` - the same idiom as the
        // `String` arm above: both a plain `array<T>` and an already-
        // `list<T>` upgrade to their own non-empty form. Shares
        // `upgrade_array_to_non_empty` with `!empty($x)`/`count($x) > 0`
        // so the three
        // "prove this array is non-empty" paths can't drift apart.
        Type::Array(..) | Type::Intersection(_) if matches!(excluded, Type::ArrayShape(shape) if shape.is_empty_array()) => {
            upgrade_array_to_non_empty(original)
        }
        // Real PHP's own `Traversable` is a marker interface userland
        // code can never implement directly (`class Foo implements
        // Traversable {}` is a real fatal error, confirmed via `php -r`)
        // - only through one of its exactly two direct subinterfaces,
        // `Iterator` and `IteratorAggregate`. Excluding one therefore
        // structurally proves the other, unlike the general "we can't
        // disprove anything more precise than the type itself" fallback
        // below - real PHPStan special-cases this identical relationship.
        // Without it, e.g. `while ($x instanceof IteratorAggregate) {
        // $x = $x->getIterator(); }`'s loop-exit type stays bare
        // `Traversable` (missing `rewind()`/`valid()`/`current()`)
        // instead of narrowing to the only other structurally possible
        // answer, `Iterator`.
        Type::Object(name, _)
            if name == "Traversable"
                && matches!(excluded, Type::Object(excl, _) if excl == "IteratorAggregate") =>
        {
            Type::Object("Iterator".into(), Vec::new())
        }
        Type::Object(name, _)
            if name == "Traversable"
                && matches!(excluded, Type::Object(excl, _) if excl == "Iterator") =>
        {
            Type::Object("IteratorAggregate".into(), Vec::new())
        }
        // Plain `bool` is exactly the two-element set `{true, false}` -
        // unlike the general "a type covering multiple distinct values
        // proves nothing when excluded" caution this function's own
        // callers already apply (`apply_equality_narrowing`'s doc
        // comment), excluding *from* `Bool` is always sound: `True`/
        // `False` are each already `is_known_single_value`, so knowing
        // one is ruled out leaves exactly the other, unconditionally -
        // no case this could ever get wrong. Without this, `match ($b)
        // { true => ..., false => ... }` (or a bare `if ($b !== true)`)
        // never narrows `$b` down to `false` at all, since `Type::Bool`
        // isn't itself represented as `Union([True, False])`.
        Type::Bool if *excluded == Type::True => Type::False,
        Type::Bool if *excluded == Type::False => Type::True,
        other if other == excluded => Type::Never,
        other => other.clone(),
    }
}
