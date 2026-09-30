//! Building an intersection type: nested intersections flattened, every
//! array member folded into one array or shape, and a member dropped when
//! another member is within it.

use crate::{AccessoryKind, Relation, Repr, Shape, ShapeField, ShapeKey, Type};

/// The intersection of `members`, with no class knowledge. `list<int>&
/// non-empty-array` is one array (`int` values, `int` keys) carrying both
/// refinements, i.e. `non-empty-list<int>`.
pub(crate) fn intersect_types(members: Vec<Type>) -> Type {
    let arguments: Vec<&Type> = members.iter().collect();
    crate::intern::memoized(crate::intern::Operation::Intersect, &arguments, || {
        Relation::structural().intersect(members.clone())
    })
}

impl Relation<'_> {
    /// The intersection of `members`, as PHPStan's `TypeCombinator::
    /// intersect` reduces it: a member another member is within is
    /// dropped (`Expr&Dim` is `Dim`), two arrays or shapes fold into one,
    /// an offset or accessory a shape can hold is applied to it, and
    /// members sharing no value make `never`.
    pub fn intersect(&self, members: Vec<Type>) -> Type {
        syntax::stack::guard(|| {
            let mut flat: Vec<Type> = Vec::new();
            for member in members {
                match member.into_repr() {
                    Repr::Intersection(inner) => flat.extend(inner),
                    other => flat.push(Type::new(other)),
                }
            }
            if flat.contains(&Type::never()) {
                return Type::never();
            }
            // `(A|B)&C` is `(A&C)|(B&C)`, for a union small enough to
            // spell out.
            const DISTRIBUTE_LIMIT: usize = 16;
            if let Some(at) = flat
                .iter()
                .position(|m| matches!(m.repr(), Repr::Union(ms) if ms.len() <= DISTRIBUTE_LIMIT))
            {
                let Repr::Union(alternatives) = flat.remove(at).into_repr() else {
                    unreachable!("matched a union above")
                };
                return alternatives
                    .into_iter()
                    .map(|alternative| {
                        let mut members = flat.clone();
                        members.push(alternative);
                        self.intersect(members)
                    })
                    .reduce(|a, b| self.union(a, b))
                    .unwrap_or_else(Type::never);
            }
            let mut array: Option<(Type, Type)> = None;
            let mut shape: Option<Shape> = None;
            let mut rest: Vec<Type> = Vec::new();
            for member in flat {
                match member.into_repr() {
                    Repr::Array(key, value) => {
                        array = Some(match array {
                            None => (Type::clone(&key), Type::clone(&value)),
                            Some((k, v)) => (
                                self.narrower(k, Type::clone(&key)),
                                self.narrower(v, Type::clone(&value)),
                            ),
                        });
                    }
                    Repr::ArrayShape(s) => {
                        let s = std::sync::Arc::unwrap_or_clone(s);
                        shape = match shape {
                            None => Some(s),
                            Some(acc) => match self.intersect_shapes(&acc, &s) {
                                Some(merged) => Some(merged),
                                None => return Type::never(),
                            },
                        };
                    }
                    other => {
                        let other = Type::new(other);
                        if !rest.contains(&other) {
                            rest.push(other);
                        }
                    }
                }
            }
            let mut out: Vec<Type> = Vec::new();
            match (shape, array) {
                (Some(s), array) => {
                    let s = match array {
                        Some((key, value)) => match self.shape_within_array(&s, &key, &value) {
                            Some(s) => s,
                            None => return Type::never(),
                        },
                        None => s,
                    };
                    let Some(s) = self.apply_offsets(s, &mut rest) else {
                        return Type::never();
                    };
                    out.push(Type::array_shape(s));
                }
                // A list keys on `int<0, max>`, whatever key its array member
                // was given.
                (None, Some((key, value))) => {
                    let key = if rest.contains(&Type::accessory(AccessoryKind::ArrayList)) {
                        self.narrower(key, Type::int_range(Some(0), None))
                    } else {
                        key
                    };
                    out.push(Type::array(key, value));
                }
                (None, None) => {}
            }
            out.extend(rest);
            let mut kept: Vec<Type> = Vec::new();
            for (i, member) in out.iter().enumerate() {
                let within_another = out.iter().enumerate().any(|(j, other)| {
                    i != j
                        && !is_refinement(other)
                        && self.is_super_type_of(member, other).yes()
                        && (j < i || !self.is_super_type_of(other, member).yes())
                });
                if !within_another {
                    kept.push(member.clone());
                }
            }
            for (i, a) in kept.iter().enumerate() {
                if kept[i + 1..].iter().any(|b| self.disjoint(a, b)) {
                    return Type::never();
                }
            }
            match kept.len() {
                0 => Type::mixed(),
                1 => kept.pop().expect("one member"),
                _ => Type::new(Repr::Intersection(kept)),
            }
        })
    }

    /// Real PHPStan's `MutatingScope::intersectButNotNever`: a hinted
    /// closure's return is its native type narrowed by what the body
    /// actually returns. An inferred type the native one can't hold at all
    /// (a fall-off-the-end `void` under `: array`) leaves the native type
    /// as-is, and a nullable native type keeps its `null`. An `int` returned
    /// under a native type with `float` and not `int` (`: float`,
    /// `: float|string`) is coerced at runtime, so it reads as the `float`.
    pub fn intersect_but_not_never(&self, native: &Type, inferred: Type) -> Type {
        let native_members: Vec<&Type> = match native.repr() {
            Repr::Union(ms) => ms.iter().collect(),
            _ => vec![native],
        };
        let inferred_members = match inferred.into_repr() {
            Repr::Union(ms) => ms,
            other => vec![Type::new(other)],
        };
        let mut kept: Vec<Type> = Vec::new();
        for m in inferred_members {
            if m.is_mixed() {
                return native.clone();
            }
            let int_like = matches!(
                m.repr(),
                Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(..)
            );
            let fits = !matches!(m.repr(), Repr::Void)
                && native_members.iter().any(|n| {
                    !(int_like && matches!(n.repr(), Repr::Float)) && self.accepts(n, &m).yes()
                });
            if fits {
                kept.push(m);
                continue;
            }
            // An int where the native type takes a float and not an int is
            // coerced to that float, as PHP's union coercion prefers it.
            if int_like
                && native_members
                    .iter()
                    .any(|n| matches!(n.repr(), Repr::Float))
            {
                kept.push(match m.repr() {
                    Repr::IntLiteral(n) => Type::float_literal(*n as f64),
                    _ => Type::float(),
                });
                continue;
            }
            kept.extend(
                native_members
                    .iter()
                    .filter(|n| !matches!(n.repr(), Repr::Null) && self.accepts(&m, n).yes())
                    .map(|n| (*n).clone()),
            );
        }
        if kept.is_empty() {
            return native.clone();
        }
        if native_members
            .iter()
            .any(|n| matches!(n.repr(), Repr::Null))
        {
            kept.push(Type::null());
        }
        kept.sort_by_key(|m| *m == Type::null());
        kept.into_iter()
            .reduce(|a, b| self.union(a, b))
            .unwrap_or_else(|| native.clone())
    }

    /// The narrower of two types when one contains the other, `never`
    /// when they share no value, both otherwise.
    fn narrower(&self, a: Type, b: Type) -> Type {
        // `mixed~S` narrows the other side to it without `S`.
        if let Repr::Mixed(_, lacks) = b.repr() {
            return match lacks {
                Some(lacks) => a.without(lacks),
                None => a,
            };
        }
        if let Repr::Mixed(_, lacks) = a.repr() {
            return match lacks {
                Some(lacks) => b.without(lacks),
                None => b,
            };
        }
        if self.is_super_type_of(&b, &a).yes() {
            a
        } else if self.is_super_type_of(&a, &b).yes() {
            b
        } else if self.disjoint(&a, &b) {
            Type::never()
        } else {
            self.intersect(vec![a, b])
        }
    }

    /// Two shapes as one: a key both have takes both values and is
    /// optional only when both have it optional; a key one lacks is
    /// dropped when optional, or held by the other's unsealed tail.
    /// `None` when no array is both.
    fn intersect_shapes(&self, a: &Shape, b: &Shape) -> Option<Shape> {
        let mut keys: Vec<&ShapeKey> = a.fields.iter().map(|f| &f.key).collect();
        for f in &b.fields {
            if !keys.contains(&&f.key) {
                keys.push(&f.key);
            }
        }
        let mut fields = Vec::new();
        for key in keys {
            let field = match (a.field(key), b.field(key)) {
                (Some(fa), Some(fb)) => Some(ShapeField::new(
                    key.clone(),
                    self.narrower(fa.ty.clone(), fb.ty.clone()),
                    fa.optional && fb.optional,
                )),
                (Some(f), None) => self.field_within_tail(f, b)?,
                (None, Some(f)) => self.field_within_tail(f, a)?,
                (None, None) => unreachable!("key came from one of the two shapes"),
            };
            if let Some(field) = field {
                if field.ty == Type::never() {
                    if !field.optional {
                        return None;
                    }
                    continue;
                }
                fields.push(field);
            }
        }
        let unsealed = match (a.unsealed.as_deref(), b.unsealed.as_deref()) {
            (Some((ka, va)), Some((kb, vb))) => Some(Box::new((
                self.narrower(ka.clone(), kb.clone()),
                self.narrower(va.clone(), vb.clone()),
            ))),
            _ => None,
        };
        Some(Shape {
            fields,
            unsealed,
            next_index: None,
        })
    }

    /// `field`, from one shape, as the other shape `other` holds it: its
    /// value narrowed by `other`'s tail, or dropped when `other` can't
    /// hold the key. The outer `None` is a required key `other` can't
    /// hold.
    fn field_within_tail(&self, field: &ShapeField, other: &Shape) -> Option<Option<ShapeField>> {
        match other.unsealed.as_deref() {
            Some((key, value)) if !self.disjoint(key, &field.key.to_type()) => {
                Some(Some(ShapeField::new(
                    field.key.clone(),
                    self.narrower(field.ty.clone(), value.clone()),
                    field.optional,
                )))
            }
            _ if field.optional => Some(None),
            _ => None,
        }
    }

    /// `shape` as an `array<key, value>` holds it. `None` when a required
    /// key can't be one of `key`.
    fn shape_within_array(&self, shape: &Shape, key: &Type, value: &Type) -> Option<Shape> {
        let tail = Shape {
            fields: Vec::new(),
            unsealed: Some(Box::new((key.clone(), value.clone()))),
            next_index: None,
        };
        self.intersect_shapes(shape, &tail)
    }

    /// `shape` with the offsets and array accessories among `rest`
    /// applied, each one it absorbs removed from `rest`. `None` when
    /// `shape` can't have one of them.
    fn apply_offsets(&self, mut shape: Shape, rest: &mut Vec<Type>) -> Option<Shape> {
        let mut unapplied = Vec::new();
        for member in rest.drain(..) {
            match member.repr() {
                Repr::HasOffset(key) => match shape.field(key).cloned() {
                    Some(f) => shape.set(ShapeField::new(f.key, f.ty, false)),
                    None if shape.is_sealed() => return None,
                    None => unapplied.push(member),
                },
                Repr::HasOffsetValue(key, value) => match shape.field(key).cloned() {
                    Some(f) => {
                        let ty = self.narrower(f.ty, (*value).clone());
                        if ty == Type::never() {
                            return None;
                        }
                        shape.set(ShapeField::new(f.key, ty, false));
                    }
                    None if shape.is_sealed() => return None,
                    None => unapplied.push(member),
                },
                Repr::Accessory(AccessoryKind::NonEmptyArray)
                    if shape.is_sealed() && shape.fields.is_empty() =>
                {
                    return None;
                }
                // A list has only the keys 0, 1, ...: a shape requiring any
                // other key can't be one, and a shape that is a list already
                // says so.
                Repr::Accessory(AccessoryKind::ArrayList) => {
                    let list_keys_only = shape
                        .fields
                        .iter()
                        .enumerate()
                        .all(|(i, f)| f.optional || f.key == crate::ShapeKey::Int(i as i64));
                    if shape.is_sealed() && !list_keys_only {
                        return None;
                    }
                    if !shape.is_list() {
                        unapplied.push(member);
                    }
                }
                _ => unapplied.push(member),
            }
        }
        *rest = unapplied;
        Some(shape)
    }
}

/// A member that only refines the base it's intersected with, never a
/// type on its own.
fn is_refinement(ty: &Type) -> bool {
    matches!(
        ty.repr(),
        Repr::Accessory(_)
            | Repr::HasOffset(_)
            | Repr::HasOffsetValue(..)
            | Repr::HasMethod(_)
            | Repr::HasProperty(_)
            | Repr::ReflectedMember(..)
    )
}
