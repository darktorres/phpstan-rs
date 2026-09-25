use crate::checker::narrowing::accessory_range::{
    upgrade_array_to_non_empty, upgrade_string_with_accessory,
};
use crate::checker::union_types;
use crate::types::{AccessoryKind, ShapeField, ShapeKey, Type};

/// Narrows `ty` by whether `key` is (`present = true`) or isn't
/// (`present = false`) provably a real offset of it -
/// `array_key_exists($key, $ty)`'s own true/false branches. An
/// `ArrayShape` member declaring `key` (required or optional) can always
/// make the call `true`; only a member declaring it *required* can be
/// proven to make the call `false` (an optional field's absence is
/// exactly what "optional" means, so its being false there is not
/// impossible for that member specifically). A member omitting `key`
/// altogether can never make the call `true`. Recurses through `Union`
/// the same way `narrow_type_to`/`narrow_type_excluding` do; anything
/// that isn't an `ArrayShape` (or a `Union` of them) is left untouched,
/// this project's usual "don't guess" precedent.
pub(crate) fn narrow_array_shape_by_key_presence(ty: &Type, key: &str, present: bool) -> Type {
    match ty {
        Type::ArrayShape(shape) => match shape.field(&ShapeKey::from_name(key)) {
            Some(field) => {
                if !present && !field.optional {
                    Type::Never
                } else {
                    ty.clone()
                }
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
                    Type::Never
                } else {
                    ty.clone()
                }
            }
        },
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(|m| narrow_array_shape_by_key_presence(m, key, present))
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
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
pub(crate) fn narrow_isset_container(ty: &Type, key: Option<&str>) -> Type {
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
        | Type::FloatLiteral(_) => Type::Never,
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
                            .unwrap_or(Type::Mixed(false)),
                    };
                    let value = crate::checker::narrow_type_excluding(&held, &Type::Null);
                    shape.set(ShapeField::new(key, value, false));
                    Type::ArrayShape(shape)
                }
                other => other,
            }
        }
        // A general array now has that key, with a non-null value.
        Type::Array(_, value) => match key {
            Some(key) => Type::Intersection(vec![
                ty.clone(),
                Type::HasOffsetValue(
                    ShapeKey::from_name(key),
                    Box::new(crate::checker::narrow_type_excluding(value, &Type::Null)),
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
                    members.push(Type::HasOffsetValue(
                        ShapeKey::from_name(key),
                        Box::new(crate::checker::narrow_type_excluding(value, &Type::Null)),
                    ));
                    Type::Intersection(members)
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
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
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
pub(crate) fn array_value_type_for_key(ty: &Type, key: &str) -> Option<Type> {
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
pub(crate) fn narrow_type_excluding_falsy(original: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(narrow_type_excluding_falsy)
                .filter(|t| *t != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Null | Type::False | Type::IntLiteral(0) => Type::Never,
        Type::StringLiteral(s) if s.is_empty() || s == "0" => Type::Never,
        Type::ArrayShape(shape) if shape.is_empty_array() => Type::Never,
        Type::Array(_, _) => upgrade_array_to_non_empty(original),
        // Delegates to the same general-purpose "add one more accessory
        // fact onto a string, bare or already-refined" helper the
        // `is_numeric()` upgrade already uses - a bare `String` gets the
        // identical `Intersection([String, NonFalsyString])` the old
        // hand-rolled arm built directly, but an already-refined value
        // (e.g. a known `non-empty-string`) now also gets upgraded to
        // the strictly narrower `non-falsy-string`, instead of falling
        // through unchanged.
        other => upgrade_string_with_accessory(other, AccessoryKind::NonFalsyString),
    }
}
