//! Array offsets: whether a type has one, and the type after writing one
//! (PHPStan's `hasOffsetValueType()`/`setOffsetValueType()`).

use crate::{AccessoryKind, Repr, Shape, ShapeField, ShapeKey, TrinaryLogic, Type};

impl Type {
    /// PHPStan's `hasOffsetValueType()`: whether this array surely has
    /// (`Yes`), surely lacks (`No`), or may have the offset `offset`.
    pub fn has_offset_value_type(&self, offset: &Type) -> TrinaryLogic {
        match self {
            Type(Repr::ArrayShape(shape)) => {
                if !shape.is_sealed() {
                    return TrinaryLogic::Maybe;
                }
                if shape.fields.is_empty() && !(offset.is_mixed() || offset.is_never()) {
                    return TrinaryLogic::No;
                }
                let Some((key, _)) = literal_key(offset) else {
                    return TrinaryLogic::Maybe;
                };
                match shape.field(&ShapeKey::from_name(&key)) {
                    Some(field) if !field.optional => TrinaryLogic::Yes,
                    Some(_) => TrinaryLogic::Maybe,
                    None => TrinaryLogic::No,
                }
            }
            Type(Repr::Array(key, _)) => general_array_has_offset(key, false, offset),
            Type(Repr::Intersection(members)) => {
                let is_list = members.contains(&Type::accessory(AccessoryKind::ArrayList));
                match members.iter().find_map(|m| match m {
                    Type(Repr::Array(k, _)) => Some(k.as_ref()),
                    _ => None,
                }) {
                    Some(key) => general_array_has_offset(key, is_list, offset),
                    None => TrinaryLogic::Maybe,
                }
            }
            Type(Repr::Union(members)) => members
                .iter()
                .map(|m| m.has_offset_value_type(offset))
                .reduce(|a, b| if a == b { a } else { TrinaryLogic::Maybe })
                .unwrap_or(TrinaryLogic::Maybe),
            _ => TrinaryLogic::Maybe,
        }
    }

    /// PHPStan's `getOffsetValueType()` for an array: the value at `key`,
    /// `None` for an offset not known literally (every value it could
    /// be; `null` from `array{}`). `None` when this isn't an array, or a
    /// sealed shape surely lacks `key`.
    pub fn offset_value_type(&self, key: Option<&ShapeKey>) -> Option<Type> {
        match self {
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
        }
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
        match self {
            // `null` becomes an array on its first write.
            Type(Repr::Null) => Type::array_shape(Shape::sealed(Vec::new())).set_offset_value_type(
                offset,
                value,
                in_range,
                track_literal_key,
            ),
            Type(Repr::ArrayShape(shape)) => match offset.and_then(literal_key_name) {
                Some(key) => {
                    let mut shape = shape.clone();
                    // A written key is present afterwards, even one the
                    // shape only had as optional.
                    shape.set(ShapeField::new(ShapeKey::from_name(&key), value, false));
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
                        Some(Type(Repr::Intersection(vec![
                            Type::array(Type::unknown_array_key(), widened),
                            Type::accessory(AccessoryKind::ArrayList),
                            Type::accessory(AccessoryKind::NonEmptyArray),
                        ])))
                    } else {
                        let key = offset
                            .map(Type::write_key_type)
                            .unwrap_or_else(Type::unknown_array_key);
                        Some(Type::array(key, widened))
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
                let key = offset
                    .map(Type::write_key_type)
                    .unwrap_or_else(Type::unknown_array_key);
                let is_append = offset.is_none();
                if (is_append || in_range)
                    && members.contains(&Type::accessory(AccessoryKind::ArrayList))
                {
                    let mut new_members = vec![
                        Type::array(key, widened),
                        Type::accessory(AccessoryKind::ArrayList),
                    ];
                    if is_append || members.contains(&Type::accessory(AccessoryKind::NonEmptyArray))
                    {
                        new_members.push(Type::accessory(AccessoryKind::NonEmptyArray));
                    }
                    Some(Type(Repr::Intersection(new_members)))
                } else {
                    Some(Type::array(key, widened))
                }
            }
            _ => None,
        }
    }

    /// The key a write at an offset of this type lands on: `int` or
    /// `string` when the offset surely is one, otherwise the benevolent
    /// `int|string` (PHPStan's `MixedType::toArrayKey()`), which a later
    /// read accepts wherever either member would do.
    pub fn write_key_type(&self) -> Type {
        if self.is_int().yes() {
            Type::int()
        } else if self.is_string().yes() {
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
    let key_ty = match offset.map(Type::write_key_type) {
        Some(new_key) => Type::union(key.clone(), new_key),
        None => key.clone(),
    };
    let base = Type::array(key_ty, Type::union(value.clone(), inner.clone()));
    match offset.and_then(literal_key_name) {
        Some(name) if track_literal_key => {
            let key = ShapeKey::from_name(&name);
            let mut members: Vec<Type> = existing_has_offset_values
                .iter()
                .filter(|m| !matches!(m, Type(Repr::HasOffsetValue(k, _)) if *k == key))
                .cloned()
                .collect();
            members.push(Type::has_offset_value(key, value));
            members.insert(0, base);
            Type(Repr::Intersection(members))
        }
        _ => base,
    }
}

/// A string or integer literal offset's text.
fn literal_key_name(offset: &Type) -> Option<String> {
    match offset {
        Type(Repr::StringLiteral(s)) => Some(s.to_string()),
        Type(Repr::IntLiteral(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// An offset's array key as text and whether it's an integer key, for a
/// literal offset (`true`/`false`/`null` included).
fn literal_key(offset: &Type) -> Option<(String, bool)> {
    match offset.to_array_key() {
        Type(Repr::IntLiteral(n)) => Some((n.to_string(), true)),
        Type(Repr::StringLiteral(s)) => Some((s.to_string(), false)),
        _ => None,
    }
}

fn general_array_has_offset(key_ty: &Type, is_list: bool, offset: &Type) -> TrinaryLogic {
    let Some((key, is_int_key)) = literal_key(offset) else {
        return TrinaryLogic::Maybe;
    };
    if is_list {
        return if !is_int_key || key.starts_with('-') {
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
