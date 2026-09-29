//! Array offsets: whether a type has one, and the type after writing one
//! (PHPStan's `hasOffsetValueType()`/`setOffsetValueType()`).

use crate::{AccessoryKind, AppendKey, Repr, Shape, ShapeField, ShapeKey, TrinaryLogic, Type};

/// The most fields an append grows a shape to; past it, the array is
/// general.
const MAX_SHAPE_FIELDS: usize = 256;

impl Type {
    /// PHPStan's `hasOffsetValueType()`: whether this array surely has
    /// (`Yes`), surely lacks (`No`), or may have the offset `offset`.
    pub fn has_offset_value_type(&self, offset: &Type) -> TrinaryLogic {
        syntax::stack::guard(|| match self {
            Type(Repr::ArrayShape(shape)) => {
                if !shape.is_sealed() {
                    return TrinaryLogic::Maybe;
                }
                if shape.fields.is_empty() && !(offset.is_mixed() || offset.is_never()) {
                    return TrinaryLogic::No;
                }
                let Some(key) = literal_key(offset) else {
                    return TrinaryLogic::Maybe;
                };
                match shape.field(&key) {
                    Some(field) if !field.optional => TrinaryLogic::Yes,
                    Some(_) => TrinaryLogic::Maybe,
                    None => TrinaryLogic::No,
                }
            }
            Type(Repr::Array(key, _)) => general_array_has_offset(key, false, offset),
            // A value of an intersection is each member's: one member
            // proving the offset there (`hasOffset('k')`) or absent settles it.
            Type(Repr::Intersection(members)) => {
                let is_list = members.contains(&Type::accessory(AccessoryKind::ArrayList));
                let key = literal_key(offset);
                let answers: Vec<TrinaryLogic> = members
                    .iter()
                    .map(|m| match m {
                        Type(Repr::Array(k, _)) => general_array_has_offset(k, is_list, offset),
                        Type(Repr::HasOffset(k)) | Type(Repr::HasOffsetValue(k, _))
                            if key.as_ref() == Some(k) =>
                        {
                            TrinaryLogic::Yes
                        }
                        other => other.has_offset_value_type(offset),
                    })
                    .collect();
                if answers.contains(&TrinaryLogic::Yes) {
                    TrinaryLogic::Yes
                } else if answers.contains(&TrinaryLogic::No) {
                    TrinaryLogic::No
                } else {
                    TrinaryLogic::Maybe
                }
            }
            Type(Repr::TemplateParam(param)) => param.bound.has_offset_value_type(offset),
            Type(Repr::Union(members)) => members
                .iter()
                .map(|m| m.has_offset_value_type(offset))
                .reduce(|a, b| if a == b { a } else { TrinaryLogic::Maybe })
                .unwrap_or(TrinaryLogic::Maybe),
            _ => TrinaryLogic::Maybe,
        })
    }

    /// PHPStan's `getOffsetValueType()` for an array: the value at `key`,
    /// `None` for an offset not known literally (every value it could
    /// be; `null` from `array{}`). `None` when this isn't an array, or a
    /// sealed shape surely lacks `key`.
    pub fn offset_value_type(&self, key: Option<&ShapeKey>) -> Option<Type> {
        syntax::stack::guard(|| match self {
            Type(Repr::Array(_, value)) => Some((**value).clone()),
            Type(Repr::ArrayShape(shape)) => match key {
                Some(key) => match shape.field(key) {
                    Some(field) => Some(field.ty.clone()),
                    None => shape.unsealed.as_deref().map(|tail| tail.1.clone()),
                },
                None if shape.is_empty_array() => Some(Type::null()),
                None => self.array_value_type(),
            },
            Type(Repr::Intersection(members)) => {
                let tracked = key.and_then(|key| {
                    members.iter().find_map(|m| match m {
                        Type(Repr::HasOffsetValue(k, v)) if k == key => Some((**v).clone()),
                        _ => None,
                    })
                });
                tracked.or_else(|| members.iter().find_map(|m| m.offset_value_type(key)))
            }
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(|m| m.offset_value_type(key))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(Type::union),
            Type(Repr::TemplateParam(param)) => param.bound.offset_value_type(key),
            _ => None,
        })
    }

    /// PHPStan's `setOffsetValueType()`: this array after `[offset] =
    /// value`, `offset` `None` for an append (`[] = value`); `None` when
    /// this type doesn't carry enough shape to update precisely. A
    /// literal offset's value is tracked (`hasOffsetValue`) only with
    /// `track_literal_key`, and a list stays a list after a write at an
    /// offset `in_range` of its existing elements.
    pub fn set_offset_value_type(
        &self,
        offset: Option<&Type>,
        value: Type,
        in_range: bool,
        track_literal_key: bool,
    ) -> Option<Type> {
        syntax::stack::guard(|| {
            match self {
                // `null` becomes an array on its first write.
                Type(Repr::Null) => Type::array_shape(Shape::sealed(Vec::new()))
                    .set_offset_value_type(offset, value, in_range, track_literal_key),
                // An append onto a sealed shape takes its next index, when
                // known (`['x' => 1] + [] = 2` is `array{x: 1, 0: 2}`).
                Type(Repr::ArrayShape(shape))
                    if offset.is_none()
                        && track_literal_key
                        && shape.is_sealed()
                        && shape.fields.len() < MAX_SHAPE_FIELDS
                        && let Some(mut next) = shape.next_append_index()
                        && let AppendKey::Key(key) = next.take() =>
                {
                    let mut shape = shape.clone();
                    shape.set(ShapeField::new(ShapeKey::Int(key), value, false));
                    shape.next_index = Some(next);
                    Some(Type::array_shape(shape))
                }
                Type(Repr::ArrayShape(shape)) => match offset.and_then(literal_key) {
                    Some(key) => {
                        let mut shape = shape.clone();
                        // A written key is present afterwards, even one the
                        // shape only had as optional.
                        shape.set(ShapeField::new(key, value, false));
                        Some(Type::array_shape(shape))
                    }
                    None => {
                        let widened = shape
                            .types()
                            .fold(value, |acc, field_ty| Type::union(acc, field_ty.clone()));
                        // An append onto a list-like container takes the next
                        // sequential int key, so the result stays a list, now
                        // surely non-empty. A dynamic key need not be
                        // sequential.
                        if offset.is_none() && self.is_list().yes() {
                            Some(crate::intersect_types(vec![
                                Type::array(Type::unknown_array_key(), widened),
                                Type::accessory(AccessoryKind::ArrayList),
                                Type::accessory(AccessoryKind::NonEmptyArray),
                            ]))
                        } else {
                            // The shape's own keys stay possible beside the
                            // written one.
                            let written = offset
                                .map(Type::write_key_type)
                                .unwrap_or_else(Type::unknown_array_key);
                            let key = Type::union(shape.key_type().generalize(), written);
                            Some(non_empty(Type::array(key, widened)))
                        }
                    }
                },
                // Past a literal shape there's no single key to grow; a
                // literal offset still gets a `hasOffsetValue` member.
                Type(Repr::Array(key, inner)) => Some(write_literal_key_aware(
                    key,
                    inner,
                    offset,
                    value,
                    track_literal_key,
                    &[],
                )),
                // Every member takes the write, or a key written one line
                // earlier would read back as missing through the union.
                Type(Repr::Union(members)) => {
                    let mut updated = Vec::with_capacity(members.len());
                    for m in members {
                        updated.push(m.set_offset_value_type(
                            offset,
                            value.clone(),
                            in_range,
                            track_literal_key,
                        )?);
                    }
                    updated.into_iter().reduce(Type::union)
                }
                // A container carrying `hasOffsetValue` members from earlier
                // literal writes accumulates them.
                Type(Repr::Intersection(members))
                    if members
                        .iter()
                        .any(|m| matches!(m, Type(Repr::HasOffsetValue(..)))) =>
                {
                    let (key, inner) = members.iter().find_map(|m| match m {
                        Type(Repr::Array(k, v)) => Some((k.as_ref(), v.as_ref())),
                        _ => None,
                    })?;
                    let existing: Vec<Type> = members
                        .iter()
                        .filter(|m| matches!(m, Type(Repr::HasOffsetValue(..))))
                        .cloned()
                        .collect();
                    Some(write_literal_key_aware(
                        key,
                        inner,
                        offset,
                        value,
                        track_literal_key,
                        &existing,
                    ))
                }
                // A `list<T>`/`non-empty-array<T>`: a dynamic key drops the
                // markers, an append keeps a list a list (now non-empty), and
                // a write within the list's range keeps it a list, non-empty
                // only if it already was.
                Type(Repr::Intersection(members)) => {
                    let inner = members.iter().find_map(Type::array_value_type)?;
                    let widened = Type::union(value, inner);
                    let written = offset
                        .map(Type::write_key_type)
                        .unwrap_or_else(Type::unknown_array_key);
                    let is_append = offset.is_none();
                    if (is_append || in_range)
                        && members.contains(&Type::accessory(AccessoryKind::ArrayList))
                    {
                        let mut new_members = vec![
                            Type::array(written, widened),
                            Type::accessory(AccessoryKind::ArrayList),
                        ];
                        if is_append
                            || members.contains(&Type::accessory(AccessoryKind::NonEmptyArray))
                        {
                            new_members.push(Type::accessory(AccessoryKind::NonEmptyArray));
                        }
                        Some(crate::intersect_types(new_members))
                    } else {
                        let key = members
                            .iter()
                            .find_map(|m| match m {
                                Type(Repr::Array(k, _)) => {
                                    Some(Type::union((**k).clone(), written.clone()))
                                }
                                _ => None,
                            })
                            .unwrap_or(written);
                        Some(non_empty(Type::array(key, widened)))
                    }
                }
                _ => None,
            }
        })
    }

    /// PHPStan's `unsetOffset()`: this array after `unset($a[offset])`.
    /// One literal key leaves the shape; one of several becomes
    /// optional, as does every field an arbitrary key could name. A
    /// general array keeps its key and value types, but is no longer
    /// surely non-empty, a list (its keys stay non-negative), or sure of
    /// the offsets the key could name. Anything else is unchanged.
    pub fn unset_offset(&self, offset: &Type) -> Type {
        syntax::stack::guard(|| {
            let key = offset.to_array_key();
            let literal_keys: Option<Vec<ShapeKey>> = {
                let finite = key.finite_types();
                (!finite.is_empty())
                    .then(|| finite.iter().map(ShapeKey::from_type).collect())
                    .flatten()
            };
            let may_name = |field: &ShapeKey| match &literal_keys {
                Some(keys) => keys.contains(field),
                None => match field {
                    ShapeKey::Int(_) => !key.is_int().no(),
                    ShapeKey::Str(_) => !key.is_string().no(),
                },
            };
            match self {
                Type(Repr::ArrayShape(shape)) => {
                    let mut shape = shape.clone();
                    // Removing the highest int key doesn't move the next
                    // index back.
                    shape.next_index = shape.next_append_index();
                    match literal_keys.as_deref() {
                        Some([only]) => shape.fields.retain(|f| f.key != *only),
                        _ => {
                            for field in &mut shape.fields {
                                if may_name(&field.key) {
                                    field.optional = true;
                                }
                            }
                        }
                    }
                    Type::array_shape(shape)
                }
                Type(Repr::Intersection(members))
                    if members.iter().any(|m| matches!(m, Type(Repr::Array(..)))) =>
                {
                    let was_list = members.contains(&Type::accessory(AccessoryKind::ArrayList));
                    let kept = members
                        .iter()
                        .filter_map(|m| match m {
                            Type(Repr::Array(_, v)) if was_list => {
                                Some(Type::array(Type::int_range(Some(0), None), Type::clone(v)))
                            }
                            Type(Repr::Accessory(
                                AccessoryKind::ArrayList | AccessoryKind::NonEmptyArray,
                            )) => None,
                            Type(Repr::HasOffset(k)) | Type(Repr::HasOffsetValue(k, _))
                                if may_name(k) =>
                            {
                                None
                            }
                            other => Some(other.clone()),
                        })
                        .collect();
                    crate::intersect_types(kept)
                }
                Type(Repr::Union(members)) => members
                    .iter()
                    .map(|m| m.unset_offset(offset))
                    .reduce(Type::union)
                    .unwrap_or_else(|| self.clone()),
                Type(Repr::BenevolentUnion(members)) => Type::benevolent_union_of(
                    members.iter().map(|m| m.unset_offset(offset)).collect(),
                ),
                _ => self.clone(),
            }
        })
    }

    /// The key a write at an offset of this type lands on: `int` or
    /// `string` when the offset surely becomes one (`'5'` becomes `5`, an
    /// arbitrary string may too), otherwise the benevolent `int|string`
    /// (PHPStan's `MixedType::toArrayKey()`), which a later read accepts
    /// wherever either member would do.
    pub fn write_key_type(&self) -> Type {
        let key = self.to_array_key();
        if key.is_int().yes() {
            Type::int()
        } else if key.is_string().yes() {
            Type::string()
        } else {
            Type::benevolent_array_key()
        }
    }
}

/// `Array(key, inner)[offset] = value`, with a `hasOffsetValue` member
/// for a literal offset when `track_literal_key`, replacing any earlier
/// one for the same key. A dynamic offset, or one not tracked, drops
/// every earlier `hasOffsetValue` member: an arbitrary key may have
/// overwritten them.
fn write_literal_key_aware(
    key: &Type,
    inner: &Type,
    offset: Option<&Type>,
    value: Type,
    track_literal_key: bool,
    existing_has_offset_values: &[Type],
) -> Type {
    // An append takes the next int key.
    let key_ty = Type::union(
        key.clone(),
        offset.map_or_else(Type::int, Type::write_key_type),
    );
    let base = Type::array(key_ty, Type::union(value.clone(), inner.clone()));
    match offset.and_then(literal_key) {
        // A `hasOffsetValue` member says the array isn't empty.
        Some(key) if track_literal_key => {
            let mut members: Vec<Type> = existing_has_offset_values
                .iter()
                .filter(|m| !matches!(m, Type(Repr::HasOffsetValue(k, _)) if *k == key))
                .cloned()
                .collect();
            members.push(Type::has_offset_value(key, value));
            members.insert(0, base);
            Type(Repr::Intersection(members))
        }
        _ => non_empty(base),
    }
}

/// `array` known to hold an element, as it does after a write.
fn non_empty(array: Type) -> Type {
    crate::intersect_types(vec![array, Type::accessory(AccessoryKind::NonEmptyArray)])
}

/// A literal offset's array key (`true` is `1`, `null` is `''`).
fn literal_key(offset: &Type) -> Option<ShapeKey> {
    ShapeKey::from_type(&offset.to_array_key())
}

fn general_array_has_offset(key_ty: &Type, is_list: bool, offset: &Type) -> TrinaryLogic {
    let Some(key) = literal_key(offset) else {
        return TrinaryLogic::Maybe;
    };
    let is_int_key = matches!(key, ShapeKey::Int(_));
    if is_list {
        return if !matches!(key, ShapeKey::Int(n) if n >= 0) {
            TrinaryLogic::No
        } else {
            TrinaryLogic::Maybe
        };
    }
    if (is_int_key && key_ty.is_int().no()) || (!is_int_key && key_ty.is_string().no()) {
        return TrinaryLogic::No;
    }
    TrinaryLogic::Maybe
}
