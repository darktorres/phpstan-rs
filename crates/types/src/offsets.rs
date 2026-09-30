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
        syntax::stack::guard(|| match self.repr() {
            Repr::ArrayShape(shape) => {
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
            Repr::Array(key, _) => general_array_has_offset(key, false, offset),
            // A value of an intersection is each member's: one member
            // proving the offset there (`hasOffset('k')`) or absent settles it.
            Repr::Intersection(members) => {
                let is_list = members.contains(&Type::accessory(AccessoryKind::ArrayList));
                let key = literal_key(offset);
                let answers: Vec<TrinaryLogic> = members
                    .iter()
                    .map(|m| match m.repr() {
                        Repr::Array(k, _) => general_array_has_offset(k, is_list, offset),
                        Repr::HasOffset(k) | Repr::HasOffsetValue(k, _)
                            if key.as_ref() == Some(k) =>
                        {
                            TrinaryLogic::Yes
                        }
                        _ => m.has_offset_value_type(offset),
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
            Repr::TemplateParam(param) => param.bound.has_offset_value_type(offset),
            Repr::Union(members) => members
                .iter()
                .map(|m| m.has_offset_value_type(offset))
                .reduce(|a, b| if a == b { a } else { TrinaryLogic::Maybe })
                .unwrap_or(TrinaryLogic::Maybe),
            _ => TrinaryLogic::Maybe,
        })
    }

    /// The value at an offset of type `offset`: when every value it may
    /// be is a literal key, the values at those keys joined (`'sql'|'dql'`
    /// reads those two fields); otherwise the value at any key.
    pub fn offset_value_type_at(&self, offset: &Type) -> Option<Type> {
        let keys: Option<Vec<ShapeKey>> = offset
            .union_members()
            .iter()
            .map(ShapeKey::from_type)
            .collect();
        match keys {
            Some(keys) if !keys.is_empty() => keys
                .iter()
                .map(|key| self.offset_value_type(Some(key)))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(Type::union),
            _ => self.offset_value_type(None),
        }
    }

    /// What a nested write (`$a[offset][…] = v`) writes into: the value at
    /// `offset`, and `array{}` where it is `null` or where a shape may lack
    /// the key, since PHP creates the array there. A general array's key,
    /// or a shape tail's, is taken as present, as a read of it is.
    /// `offset` `None` is an append, which always starts from `array{}`.
    /// `None` when this isn't an array or `null`.
    pub fn nested_write_container(&self, offset: Option<&Type>) -> Option<Type> {
        let empty = || Type::array_shape(Shape::sealed(Vec::new()));
        let Some(offset) = offset else {
            return Some(empty());
        };
        // The keys it may be, when there are few enough to list
        // (`int<0, 2>` is `0|1|2`).
        let keys: Option<Vec<ShapeKey>> = offset.finite_types().iter().map(literal_key).collect();
        let mut containers = Vec::new();
        for member in self.union_members() {
            if member.is_null().yes() {
                containers.push(empty());
                continue;
            }
            if !member.is_array().yes() {
                return None;
            }
            let reads: Vec<(Type, Option<Type>)> = match &keys {
                Some(keys) if !keys.is_empty() => keys
                    .iter()
                    .map(|key| (key.to_type(), member.offset_value_type(Some(key))))
                    .collect(),
                _ => vec![(offset.clone(), member.offset_value_type(None))],
            };
            let shape = crate::union::shape_parts(member).map(|(shape, _)| shape);
            for (key, value) in reads {
                let presence = match shape {
                    Some(shape) => match literal_key(&key) {
                        Some(k) => match shape.field(&k) {
                            Some(field) if !field.optional => TrinaryLogic::Yes,
                            Some(_) => TrinaryLogic::Maybe,
                            None if shape.is_sealed() => TrinaryLogic::No,
                            None => TrinaryLogic::Yes,
                        },
                        None if shape.is_sealed() => member.has_offset_value_type(&key),
                        None => TrinaryLogic::Yes,
                    },
                    None => TrinaryLogic::Yes,
                };
                if !presence.yes() {
                    containers.push(empty());
                }
                if presence.no() {
                    continue;
                }
                for v in value.unwrap_or_else(Type::mixed).union_members() {
                    containers.push(if v.is_null().yes() {
                        empty()
                    } else {
                        v.clone()
                    });
                }
            }
        }
        Some(Type::union_of(containers))
    }

    /// PHPStan's `getOffsetValueType()` for an array: the value at `key`,
    /// `None` for an offset not known literally (every value it could
    /// be; `null` from `array{}`). `None` when this isn't an array, or a
    /// sealed shape surely lacks `key`.
    pub fn offset_value_type(&self, key: Option<&ShapeKey>) -> Option<Type> {
        syntax::stack::guard(|| match self.repr() {
            Repr::Array(_, value) => Some((*value).clone()),
            Repr::ArrayShape(shape) => match key {
                Some(key) => match shape.field(key) {
                    Some(field) => Some(field.ty.clone()),
                    None => shape.unsealed.as_deref().map(|tail| tail.1.clone()),
                },
                None if shape.is_empty_array() => Some(Type::null()),
                None => self.array_value_type(),
            },
            Repr::Intersection(members) => {
                let tracked = key.and_then(|key| {
                    members.iter().find_map(|m| match m.repr() {
                        Repr::HasOffsetValue(k, v) if k == key => Some((*v).clone()),
                        _ => None,
                    })
                });
                tracked.or_else(|| members.iter().find_map(|m| m.offset_value_type(key)))
            }
            Repr::Union(members) | Repr::BenevolentUnion(members) => members
                .iter()
                .map(|m| m.offset_value_type(key))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .reduce(Type::union),
            Repr::TemplateParam(param) => param.bound.offset_value_type(key),
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
            match self.repr() {
                // `null` becomes an array on its first write.
                Repr::Null => Type::array_shape(Shape::sealed(Vec::new())).set_offset_value_type(
                    offset,
                    value,
                    in_range,
                    track_literal_key,
                ),
                // An append onto a sealed shape takes its next index, when
                // known (`['x' => 1] + [] = 2` is `array{x: 1, 0: 2}`).
                Repr::ArrayShape(shape)
                    if offset.is_none()
                        && track_literal_key
                        && shape.is_sealed()
                        && shape.fields.len() < MAX_SHAPE_FIELDS
                        && let Some(mut next) = shape.next_append_index()
                        && let AppendKey::Key(key) = next.take() =>
                {
                    let mut shape = Shape::clone(shape);
                    shape.set(ShapeField::new(ShapeKey::Int(key), value, false));
                    shape.next_index = Some(next);
                    Some(Type::array_shape(shape))
                }
                Repr::ArrayShape(shape) => match offset.and_then(literal_key) {
                    Some(key) => {
                        let mut shape = Shape::clone(shape);
                        // A written key is present afterwards, even one the
                        // shape only had as optional.
                        shape.set(ShapeField::new(key, value, false));
                        Some(Type::array_shape(shape))
                    }
                    // A key that is one of several literal keys, or one of
                    // few values the shape already has as keys (`int<0, 2>`
                    // on a three-element list), writes one of them: each
                    // keeps its value or takes the written one, one the
                    // shape lacks may now exist, and the array is non-empty.
                    None if let Some(written) = offset
                        && let Some(keys) = written
                            .union_members()
                            .iter()
                            .map(literal_key)
                            .collect::<Option<Vec<_>>>()
                            .or_else(|| {
                                let keys = written
                                    .finite_types()
                                    .iter()
                                    .map(literal_key)
                                    .collect::<Option<Vec<_>>>()?;
                                (!keys.is_empty() && keys.iter().all(|k| shape.field(k).is_some()))
                                    .then_some(keys)
                            }) =>
                    {
                        let mut shape = Shape::clone(shape);
                        for key in keys {
                            let field = match shape.field(&key) {
                                Some(field) => ShapeField::new(
                                    key,
                                    Type::union(field.ty.clone(), value.clone()),
                                    field.optional,
                                ),
                                None => ShapeField::new(key, value.clone(), true),
                            };
                            shape.set(field);
                        }
                        Some(crate::union::shape_type(shape, true))
                    }
                    // Any other key may write a field it could equal, or
                    // another key: the shape stays, open to that key. Only
                    // a shape a required field keeps non-empty stays one;
                    // an all-optional one is the general array below.
                    None if let Some(written) = offset
                        && shape.fields.iter().any(|f| !f.optional) =>
                    {
                        Some(Type::array_shape(open_to_key(shape, written, value)))
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
                Repr::Array(key, inner) => Some(write_literal_key_aware(
                    key,
                    inner,
                    offset,
                    value,
                    track_literal_key,
                    &[],
                )),
                // Every member takes the write, or a key written one line
                // earlier would read back as missing through the union.
                Repr::Union(members) => {
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
                Repr::Intersection(members)
                    if members
                        .iter()
                        .any(|m| matches!(m.repr(), Repr::HasOffsetValue(..))) =>
                {
                    let (key, inner) = members.iter().find_map(|m| match m.repr() {
                        Repr::Array(k, v) => Some((k, v)),
                        _ => None,
                    })?;
                    let existing: Vec<Type> = members
                        .iter()
                        .filter(|m| matches!(m.repr(), Repr::HasOffsetValue(..)))
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
                // A shape with accessories (`non-empty-array{a?: int}`)
                // takes the write on its shape. A write never empties an
                // array, so non-emptiness stays; the other accessories may
                // name the written key and are dropped.
                Repr::Intersection(members)
                    if let Some(shape) = members
                        .iter()
                        .find(|m| matches!(m.repr(), Repr::ArrayShape(_))) =>
                {
                    let non_empty_before =
                        members.contains(&Type::accessory(AccessoryKind::NonEmptyArray));
                    // An all-optional shape known non-empty stays a shape,
                    // open to a key that isn't literal.
                    if non_empty_before
                        && let Repr::ArrayShape(inner) = shape.repr()
                        && let Some(written) = offset
                        && written
                            .union_members()
                            .iter()
                            .any(|m| literal_key(m).is_none())
                    {
                        return Some(non_empty(Type::array_shape(open_to_key(
                            inner, written, value,
                        ))));
                    }
                    let written =
                        shape.set_offset_value_type(offset, value, in_range, track_literal_key)?;
                    if non_empty_before {
                        Some(non_empty(written))
                    } else {
                        Some(written)
                    }
                }
                // A `list<T>`/`non-empty-array<T>`: a dynamic key drops the
                // markers, an append keeps a list a list (now non-empty), and
                // a write within the list's range keeps it a list, non-empty
                // only if it already was.
                Repr::Intersection(members) => {
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
                            .find_map(|m| match m.repr() {
                                Repr::Array(k, _) => {
                                    Some(Type::union((*k).clone(), written.clone()))
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
            match self.repr() {
                Repr::ArrayShape(shape) => {
                    let mut shape = Shape::clone(shape);
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
                Repr::Intersection(members)
                    if members.iter().any(|m| matches!(m.repr(), Repr::Array(..))) =>
                {
                    let was_list = members.contains(&Type::accessory(AccessoryKind::ArrayList));
                    let kept = members
                        .iter()
                        .filter_map(|m| match m.repr() {
                            Repr::Array(_, v) if was_list => {
                                Some(Type::array(Type::int_range(Some(0), None), Type::clone(v)))
                            }
                            Repr::Accessory(
                                AccessoryKind::ArrayList | AccessoryKind::NonEmptyArray,
                            ) => None,
                            Repr::HasOffset(k) | Repr::HasOffsetValue(k, _) if may_name(k) => None,
                            _ => Some(m.clone()),
                        })
                        .collect();
                    crate::intersect_types(kept)
                }
                Repr::Union(members) => members
                    .iter()
                    .map(|m| m.unset_offset(offset))
                    .reduce(Type::union)
                    .unwrap_or_else(|| self.clone()),
                Repr::BenevolentUnion(members) => Type::benevolent_union_of(
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
                .filter(|m| !matches!(m.repr(), Repr::HasOffsetValue(k, _) if *k == key))
                .cloned()
                .collect();
            members.push(Type::has_offset_value(key, value));
            members.insert(0, base);
            Type::new(Repr::Intersection(members))
        }
        _ => non_empty(base),
    }
}

/// `shape` after a write of `value` at `written`, a key that isn't one
/// literal: each field the key may name may take the value, and the shape
/// opens to the key's other values.
fn open_to_key(shape: &Shape, written: &Type, value: Type) -> Shape {
    let written = written.write_key_type();
    let relation = crate::Relation::structural();
    let mut shape = Shape::clone(shape);
    for field in &mut shape.fields {
        if !relation
            .is_super_type_of(&written, &field.key.to_type())
            .no()
        {
            field.ty = Type::union(field.ty.clone(), value.clone());
        }
    }
    shape.unsealed = Some(Box::new(match shape.unsealed.take() {
        Some(tail) => (Type::union(tail.0, written), Type::union(tail.1, value)),
        None => (written, value),
    }));
    shape.next_index = None;
    shape
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
