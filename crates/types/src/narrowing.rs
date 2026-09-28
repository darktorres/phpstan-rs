//! Narrowing a type by what a condition proved: to a checked type, away
//! from an excluded one, by key presence, by truthiness, and the array
//! and integer refinements comparisons and counts prove.

use crate::{AccessoryKind, PhpString, Relation, ShapeField, ShapeKey, Type, union_types};
use syntax::ast::BinOp;

/// A comparison's own truth value, canonicalized to "the narrowed side
/// is smaller/smaller-or-equal/greater/greater-or-equal than the bound" -
/// `<`/`<=`/`>`/`>=` all four collapse to Smaller/SmallerOrEqual here (a
/// negated comparison mirrors real PHPStan's own identical `Greater(l,
/// r)` -> `Smaller(r, l)` rewrite, `BinaryOpHandler.php`), so
/// `comparison_narrowed_bounds` only ever has to reason
/// about two shapes, not four.
enum ComparisonOutcome {
    Smaller,
    SmallerOrEqual,
    Greater,
    GreaterOrEqual,
}

impl ComparisonOutcome {
    fn negate(self) -> Self {
        match self {
            ComparisonOutcome::Smaller => ComparisonOutcome::GreaterOrEqual,
            ComparisonOutcome::SmallerOrEqual => ComparisonOutcome::Greater,
            ComparisonOutcome::Greater => ComparisonOutcome::SmallerOrEqual,
            ComparisonOutcome::GreaterOrEqual => ComparisonOutcome::Smaller,
        }
    }
}

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
pub fn narrow_instanceof_true(relation: &Relation, current: &Type, target: &Type) -> Type {
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
    let non_null = narrow_type_excluding(current, &Type::null());
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
pub fn narrow_type_to(relation: &Relation, original: &Type, target: &Type) -> Type {
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
pub fn is_known_single_value(ty: &Type) -> bool {
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

pub fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
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
                    !crate::Relation::structural()
                        .is_super_type_of(excluded, m)
                        .yes()
                })
                .map(|m| narrow_type_excluding(m, excluded))
                .collect();
            match kept.len() {
                0 => Type::never(),
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
            Type::intersection_of(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NonEmptyString),
            ])
        }
        // `$x !== []` on a plain `array`/`list` - the same idiom as the
        // `String` arm above: both a plain `array<T>` and an already-
        // `list<T>` upgrade to their own non-empty form. Shares
        // `upgrade_array_to_non_empty` with `!empty($x)`/`count($x) > 0`
        // so the three
        // "prove this array is non-empty" paths can't drift apart.
        Type::Array(..) | Type::ArrayShape(_) | Type::Intersection(_)
            if excluded.is_empty_array().yes() =>
        {
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
            Type::object("Iterator".into(), Vec::new())
        }
        Type::Object(name, _)
            if name == "Traversable"
                && matches!(excluded, Type::Object(excl, _) if excl == "Iterator") =>
        {
            Type::object("IteratorAggregate".into(), Vec::new())
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
        Type::Bool if *excluded == Type::True => Type::bool_literal(false),
        Type::Bool if *excluded == Type::False => Type::bool_literal(true),
        other if other == excluded => Type::never(),
        other => other.clone(),
    }
}

pub fn upgrade_array_to_non_empty(ty: &Type) -> Type {
    match ty {
        Type::Array(key, inner) => Type::intersection_of(vec![
            Type::array((**key).clone(), (**inner).clone()),
            Type::accessory(AccessoryKind::NonEmptyArray),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray)) =>
        {
            let mut members = members.clone();
            members.push(Type::accessory(AccessoryKind::NonEmptyArray));
            Type::intersection_of(members)
        }
        // A genuinely empty `ArrayShape` is a real contradiction once
        // `count() > 0`/truthy is known - collapses to `Never` (the same
        // "this branch is impossible" signal `narrow_type_excluding_
        // falsy`'s own identical arm already uses) rather than staying
        // unchanged, so a `Union` member that's provably empty gets
        // dropped below instead of surviving as a spurious alternative.
        Type::ArrayShape(shape) if shape.is_empty_array() => Type::never(),
        // A shape whose keys are all optional can't be empty any more.
        Type::ArrayShape(_) if !ty.is_non_empty_array().yes() => crate::intersect_types(vec![
            ty.clone(),
            Type::accessory(AccessoryKind::NonEmptyArray),
        ]),
        // A loop-built array whose own entry might still be its
        // pre-loop `[]` is a real `Union` including that empty-shape
        // alternative, which a `count($x) > 0` guard immediately
        // afterward should disprove entirely (dropped via the `Never`
        // arm just above), not merely leave untouched by falling to
        // `_ => ty.clone()`.
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(upgrade_array_to_non_empty)
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::never(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::union_of(kept),
            }
        }
        _ => ty.clone(),
    }
}

/// `int_range_bounds`'s own "isn't range-representable at all" `None`
/// widened to also accept a bare `Type::Int` as the fully-unbounded range
/// `(None, None)` - the common starting point for a plain `int` parameter
/// before any comparison has narrowed it (`int_range_bounds` itself stays
/// strict, since `infer_binary`'s own arithmetic call sites need to tell
/// "unbounded `int`" apart from "not an int at all" for their own,
/// different reasons).
pub fn numeric_bounds_for_narrowing(ty: &Type) -> Option<(Option<i64>, Option<i64>)> {
    match ty {
        Type::Int => Some((None, None)),
        _ => ty.int_range_bounds(),
    }
}

/// The new `(min, max)` bound pair for the narrowed side, once `op`
/// (written as `l op r`, `keyed_on_left` saying which of `l`/`r` is the
/// narrowed side) is known true (`branch_true`) or false - `None` only on
/// `i64` overflow computing an exclusive bound's own off-by-one (`bound -
/// 1`/`bound + 1`), an edge real code never hits (a literal compared
/// against `i64::MIN`/`i64::MAX`), where leaving `locals` untouched is the
/// same "give up gracefully" choice every other range-arithmetic helper
/// in this project already makes rather than risk a wrong bound.
pub fn comparison_narrowed_bounds(
    op: &BinOp,
    keyed_on_left: bool,
    branch_true: bool,
    cur_min: Option<i64>,
    cur_max: Option<i64>,
    bound: i64,
) -> Option<(Option<i64>, Option<i64>)> {
    let outcome = match (op, keyed_on_left) {
        (BinOp::Lt, true) | (BinOp::Gt, false) => ComparisonOutcome::Smaller,
        (BinOp::LtEq, true) | (BinOp::GtEq, false) => ComparisonOutcome::SmallerOrEqual,
        (BinOp::Gt, true) | (BinOp::Lt, false) => ComparisonOutcome::Greater,
        (BinOp::GtEq, true) | (BinOp::LtEq, false) => ComparisonOutcome::GreaterOrEqual,
        _ => return None,
    };
    let outcome = if branch_true {
        outcome
    } else {
        outcome.negate()
    };
    match outcome {
        ComparisonOutcome::Smaller => {
            let candidate = bound.checked_sub(1)?;
            Some((
                cur_min,
                Some(cur_max.map_or(candidate, |m| m.min(candidate))),
            ))
        }
        ComparisonOutcome::SmallerOrEqual => {
            Some((cur_min, Some(cur_max.map_or(bound, |m| m.min(bound)))))
        }
        ComparisonOutcome::Greater => {
            let candidate = bound.checked_add(1)?;
            Some((
                Some(cur_min.map_or(candidate, |m| m.max(candidate))),
                cur_max,
            ))
        }
        ComparisonOutcome::GreaterOrEqual => {
            Some((Some(cur_min.map_or(bound, |m| m.max(bound))), cur_max))
        }
    }
}

/// `array_is_list($x)`'s own true-branch narrowing - mirrors `upgrade_
/// array_to_non_empty` exactly, just adding `ArrayList` instead of
/// `NonEmptyArray`. Doesn't force the key type itself to `Type::Int` -
/// every other list-producing call site in this project (`preg_split`,
/// `array_values`, ...) leaves the stored key as `unknown_array_key()`
/// too, relying on `describe_accessory_intersection`'s own display-layer
/// guard (a list's key is always `int` by definition, so showing it
/// would be redundant) rather than encoding it twice. A docblock-parsed
/// `list<T>` still uses a literal `Type::Int` key instead (needed
/// elsewhere - see `parse_type_string`'s own `"list"` arm), so `crate::
/// subtype`'s dedicated list-vs-list `Intersection` arm and `crate::
/// types_equal_for_union` both compare the two representations ignoring
/// key entirely, rather than this project settling on one encoding
/// everywhere.
pub fn upgrade_array_to_list(ty: &Type) -> Type {
    match ty {
        Type::Array(key, inner) => Type::intersection_of(vec![
            Type::array((**key).clone(), (**inner).clone()),
            Type::accessory(AccessoryKind::ArrayList),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            let mut members = members.clone();
            members.push(Type::accessory(AccessoryKind::ArrayList));
            Type::intersection_of(members)
        }
        Type::Union(members) => Type::union_of(members.iter().map(upgrade_array_to_list).collect()),
        _ => ty.clone(),
    }
}

/// A failing `array_is_list($x)`: no list alternative survives, and what
/// does is non-empty, as an empty array is a list.
pub fn narrow_array_to_non_list(ty: &Type) -> Type {
    match ty {
        Type::Intersection(members)
            if members.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            Type::never()
        }
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(narrow_array_to_non_list)
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::never(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::union_of(kept),
            }
        }
        _ => upgrade_array_to_non_empty(ty),
    }
}

/// Narrows `ty` by whether `key` is (`present = true`) or isn't
/// (`present = false`) provably a real offset of it -
/// `array_key_exists($key, $ty)`'s own true/false branches. A member
/// declaring `key` required can't make the call `false`, and a sealed
/// member omitting it can't make it `true`; an optional `key` becomes
/// required when present and is dropped when absent, and a general array
/// has it when present. Recurses through `Union`; anything else is left
/// untouched.
pub fn narrow_array_shape_by_key_presence(ty: &Type, key: &str, present: bool) -> Type {
    match ty {
        Type::ArrayShape(shape) => match shape.field(&ShapeKey::from_name(key)) {
            Some(field) if !field.optional => {
                if present {
                    ty.clone()
                } else {
                    Type::never()
                }
            }
            Some(field) => {
                let mut shape = shape.clone();
                if present {
                    shape.set(ShapeField::new(field.key.clone(), field.ty.clone(), false));
                } else {
                    shape.fields.retain(|f| f.key != field.key);
                }
                Type::array_shape(shape)
            }
            // An *unsealed* shape (`array{a: int, ...}`) can carry keys
            // beyond the ones it lists - `key` simply not
            // being one of the declared fields is never proof it's
            // absent, unlike a sealed shape's identical situation just
            // below. Only the `present` direction is affected: "is `key`
            // known to be *present*" still correctly stays unprovable
            // either way (an unsealed shape doesn't promise anything
            // about a specific unlisted key being there, only that it
            // *could* be).
            None if !shape.is_sealed() => ty.clone(),
            None => {
                if present {
                    Type::never()
                } else {
                    ty.clone()
                }
            }
        },
        // A general array now has that key.
        Type::Array(..) if present => {
            crate::intersect_types(vec![ty.clone(), Type::has_offset(ShapeKey::from_name(key))])
        }
        Type::Intersection(members)
            if present && members.iter().any(|m| matches!(m, Type::Array(..))) =>
        {
            crate::intersect_types(vec![ty.clone(), Type::has_offset(ShapeKey::from_name(key))])
        }
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(|m| narrow_array_shape_by_key_presence(m, key, present))
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::never(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::union_of(kept),
            }
        }
        _ => ty.clone(),
    }
}

/// The container half of a true `isset($c[key])`: `$c` held an offset
/// whose value isn't `null`, so it's none of the scalar/null types an
/// offset read always yields `null` for, and a shape with `key` listed
/// has it present and non-null. `key` is `None` for a non-literal
/// subscript.
pub fn narrow_isset_container(ty: &Type, key: Option<&str>) -> Type {
    match ty {
        Type::Null
        | Type::Void
        | Type::Bool
        | Type::True
        | Type::False
        | Type::Int
        | Type::IntLiteral(_)
        | Type::IntRange(..)
        | Type::Float
        | Type::FloatLiteral(_) => Type::never(),
        Type::ArrayShape(_) => {
            let Some(key) = key else {
                return ty.clone();
            };
            match narrow_array_shape_by_key_presence(ty, key, true) {
                Type::ArrayShape(mut shape) => {
                    let key = ShapeKey::from_name(key);
                    // An unsealed shape's tail held it when it isn't
                    // listed.
                    let held = match shape.field(&key) {
                        Some(field) => field.ty.clone(),
                        None => shape
                            .unsealed
                            .as_deref()
                            .map(|tail| tail.1.clone())
                            .unwrap_or(Type::mixed()),
                    };
                    let value = narrow_type_excluding(&held, &Type::null());
                    shape.set(ShapeField::new(key, value, false));
                    Type::array_shape(shape)
                }
                other => other,
            }
        }
        // A general array now has that key, with a non-null value.
        Type::Array(_, value) => match key {
            Some(key) => Type::intersection_of(vec![
                ty.clone(),
                Type::has_offset_value(
                    ShapeKey::from_name(key),
                    narrow_type_excluding(value, &Type::null()),
                ),
            ]),
            None => ty.clone(),
        },
        // `list<T>`/`non-empty-array<T>` - an accessory-marked array.
        Type::Intersection(members) => {
            let element = members.iter().find_map(|m| match m {
                Type::Array(_, value) => Some(value),
                _ => None,
            });
            match (key, element) {
                (Some(key), Some(value)) => {
                    let mut members = members.clone();
                    members.push(Type::has_offset_value(
                        ShapeKey::from_name(key),
                        narrow_type_excluding(value, &Type::null()),
                    ));
                    Type::intersection_of(members)
                }
                _ => ty.clone(),
            }
        }
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(|m| narrow_isset_container(m, key))
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::never(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::union_of(kept),
            }
        }
        _ => ty.clone(),
    }
}

/// `HasOffsetValueType` equivalent, adapted to this
/// project's own type system rather than replicating real PHPStan's -
/// real PHPStan represents "definitely has offset `key`, value type `V`"
/// as a whole new `CompoundType`/`AccessoryType` (`Type::Accessory::
/// HasOffsetType`/`HasOffsetValueType`, ~40 polymorphic methods,
/// `TypeCombinator`-intersected onto the base array type) because its
/// own `ConstantArrayType` (the `ArrayShape` equivalent) is reserved for
/// a *fully* known literal shape - a general `array<K, V>` needs a
/// *separate* vehicle to carry "and I also know this one extra key
/// exists" without claiming to know every other key too. This project
/// has no such restriction (`Type::ArrayShape` was never scoped to
/// "fully known" in the first place), and already has the exact right
/// mechanism for "I know more about this one specific access than the
/// container's own general type says" - a composite `narrowing_key`
/// entry (`"arr[key]"`), the same one `is_string($info['code'])`-style
/// per-element narrowing already writes and `infer_expr`'s own `Index`
/// arm already reads back first. So instead of building a new
/// accessory-type hierarchy, `array_key_exists`'s own true-branch
/// narrowing (below) writes `"arr[key]" => this function's own result`
/// directly, alongside its existing `$arr`-wide `narrow_array_shape_by_
/// key_presence` call - no new `Type` variant, no `TypeCombinator`
/// equivalent needed. Derived from `ty` *after* shape-based narrowing
/// (not the pre-narrowing type), so a `Union` member `narrow_array_
/// shape_by_key_presence` already proved impossible doesn't also
/// contribute a bogus value type here. `None` when nothing in `ty`
/// carries a real value type for `key` at all (an unrelated type, or an
/// `ArrayShape` genuinely missing the key - only reachable when
/// `present` was `false`, so the composite key isn't written for that
/// branch anyway).
pub fn array_value_type_for_key(ty: &Type, key: &str) -> Option<Type> {
    match ty {
        Type::ArrayShape(shape) => shape.field(&ShapeKey::from_name(key)).map(|f| f.ty.clone()),
        Type::Array(_, value_ty) => Some((**value_ty).clone()),
        Type::Union(members) => members
            .iter()
            .map(|m| array_value_type_for_key(m, key))
            .collect::<Option<Vec<Type>>>()?
            .into_iter()
            .reduce(union_types),
        _ => None,
    }
}

/// Narrows `original` by excluding every falsy member this project can
/// represent - used for a bare truthiness check (`if ($x)`) and
/// `empty($x)`'s own truthy branch. The falsy set covers `Null`/`False`,
/// `IntLiteral(0)`, `StringLiteral("")`/`StringLiteral("0")`, and an
/// empty `ArrayShape`, and upgrades a kept bare `String`/`Array` to its
/// own accessory-refined form (`non-falsy-string`/`non-empty-array` -
/// `empty()` is false for `'0'` too, not merely non-empty). Recurses
/// through `Union` members and drops any that
/// resolve to `Never`, rather than pre-filtering and cloning separately -
/// simpler than maintaining two parallel "is this falsy" checks.
pub fn narrow_type_excluding_falsy(original: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(narrow_type_excluding_falsy)
                .filter(|t| *t != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::never(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::union_of(kept),
            }
        }
        Type::Null | Type::False | Type::IntLiteral(0) => Type::never(),
        Type::StringLiteral(s) if s.is_empty() || s == "0" => Type::never(),
        Type::ArrayShape(shape) if shape.is_empty_array() => Type::never(),
        Type::Array(_, _) => upgrade_array_to_non_empty(original),
        // Delegates to the same general-purpose "add one more accessory
        // fact onto a string, bare or already-refined" helper the
        // `is_numeric()` upgrade already uses - a bare `String` gets the
        // identical `Intersection([String, NonFalsyString])` the old
        // hand-rolled arm built directly, but an already-refined value
        // (e.g. a known `non-empty-string`) now also gets upgraded to
        // the strictly narrower `non-falsy-string`, instead of falling
        // through unchanged.
        other => other.with_string_accessory(AccessoryKind::NonFalsyString),
    }
}
