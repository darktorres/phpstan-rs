//! Array offsets: whether a type has one, and the type after writing one
//! (PHPStan's `hasOffsetValueType()`/`setOffsetValueType()`).

use crate::{AccessoryKind, Shape, ShapeField, ShapeKey, TrinaryLogic, Type, union_types};

impl Type {
    /// PHPStan's `hasOffsetValueType()`: whether this array surely has
    /// (`Yes`), surely lacks (`No`), or may have the offset `offset`.
    pub fn has_offset_value_type(&self, offset: &Type) -> TrinaryLogic {
        match self {
            Type::ArrayShape(shape) => {
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
            Type::Array(key, _) => general_array_has_offset(key, false, offset),
            Type::Intersection(members) => {
                let is_list = members.contains(&Type::Accessory(AccessoryKind::ArrayList));
                match members.iter().find_map(|m| match m {
                    Type::Array(k, _) => Some(k.as_ref()),
                    _ => None,
                }) {
                    Some(key) => general_array_has_offset(key, is_list, offset),
                    None => TrinaryLogic::Maybe,
                }
            }
            Type::Union(members) => members
                .iter()
                .map(|m| m.has_offset_value_type(offset))
                .reduce(|a, b| if a == b { a } else { TrinaryLogic::Maybe })
                .unwrap_or(TrinaryLogic::Maybe),
            _ => TrinaryLogic::Maybe,
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
            Type::Null => Type::ArrayShape(Shape::sealed(Vec::new())).set_offset_value_type(
                offset,
                value,
                in_range,
                track_literal_key,
            ),
            Type::ArrayShape(shape) => match offset.and_then(literal_key_name) {
                Some(key) => {
                    let mut shape = shape.clone();
                    // A written key is present afterwards, even one the
                    // shape only had as optional.
                    shape.set(ShapeField::new(ShapeKey::from_name(&key), value, false));
                    Some(Type::ArrayShape(shape))
                }
                None => {
                    let widened = shape
                        .types()
                        .fold(value, |acc, field_ty| union_types(acc, field_ty.clone()));
                    // An append onto a list-like container takes the next
                    // sequential int key, so the result stays a list, now
                    // surely non-empty. A dynamic key need not be
                    // sequential.
                    if offset.is_none() && self.is_list().yes() {
                        Some(Type::Intersection(vec![
                            Type::Array(Box::new(Type::unknown_array_key()), Box::new(widened)),
                            Type::Accessory(AccessoryKind::ArrayList),
                            Type::Accessory(AccessoryKind::NonEmptyArray),
                        ]))
                    } else {
                        let key = offset
                            .map(Type::write_key_type)
                            .unwrap_or_else(Type::unknown_array_key);
                        Some(Type::Array(Box::new(key), Box::new(widened)))
                    }
                }
            },
            // Past a literal shape there's no single key to grow; a
            // literal offset still gets a `hasOffsetValue` member.
            Type::Array(key, inner) => Some(write_literal_key_aware(
                key,
                inner,
                offset,
                value,
                track_literal_key,
                &[],
            )),
            // Every member takes the write, or a key written one line
            // earlier would read back as missing through the union.
            Type::Union(members) => {
                let mut updated = Vec::with_capacity(members.len());
                for m in members {
                    updated.push(m.set_offset_value_type(
                        offset,
                        value.clone(),
                        in_range,
                        track_literal_key,
                    )?);
                }
                updated.into_iter().reduce(union_types)
            }
            // A container carrying `hasOffsetValue` members from earlier
            // literal writes accumulates them.
            Type::Intersection(members)
                if members
                    .iter()
                    .any(|m| matches!(m, Type::HasOffsetValue(..))) =>
            {
                let (key, inner) = members.iter().find_map(|m| match m {
                    Type::Array(k, v) => Some((k.as_ref(), v.as_ref())),
                    _ => None,
                })?;
                let existing: Vec<Type> = members
                    .iter()
                    .filter(|m| matches!(m, Type::HasOffsetValue(..)))
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
            Type::Intersection(members) => {
                let inner = members.iter().find_map(Type::array_value_type)?;
                let widened = union_types(value, inner);
                let key = offset
                    .map(Type::write_key_type)
                    .unwrap_or_else(Type::unknown_array_key);
                let is_append = offset.is_none();
                if (is_append || in_range)
                    && members.contains(&Type::Accessory(AccessoryKind::ArrayList))
                {
                    let mut new_members = vec![
                        Type::Array(Box::new(key), Box::new(widened)),
                        Type::Accessory(AccessoryKind::ArrayList),
                    ];
                    if is_append || members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray))
                    {
                        new_members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
                    }
                    Some(Type::Intersection(new_members))
                } else {
                    Some(Type::Array(Box::new(key), Box::new(widened)))
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
            Type::Int
        } else if self.is_string().yes() {
            Type::String
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
        Some(new_key) => union_types(key.clone(), new_key),
        None => key.clone(),
    };
    let base = Type::Array(
        Box::new(key_ty),
        Box::new(union_types(value.clone(), inner.clone())),
    );
    match offset.and_then(literal_key_name) {
        Some(name) if track_literal_key => {
            let key = ShapeKey::from_name(&name);
            let mut members: Vec<Type> = existing_has_offset_values
                .iter()
                .filter(|m| !matches!(m, Type::HasOffsetValue(k, _) if *k == key))
                .cloned()
                .collect();
            members.push(Type::HasOffsetValue(key, Box::new(value)));
            members.insert(0, base);
            Type::Intersection(members)
        }
        _ => base,
    }
}

/// A string or integer literal offset's text.
fn literal_key_name(offset: &Type) -> Option<String> {
    match offset {
        Type::StringLiteral(s) => Some(s.to_string()),
        Type::IntLiteral(n) => Some(n.to_string()),
        _ => None,
    }
}

/// An offset's array key as text and whether it's an integer key, for a
/// literal offset (`true`/`false`/`null` included).
fn literal_key(offset: &Type) -> Option<(String, bool)> {
    match offset.to_array_key() {
        Type::IntLiteral(n) => Some((n.to_string(), true)),
        Type::StringLiteral(s) => Some((s.to_string(), false)),
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
