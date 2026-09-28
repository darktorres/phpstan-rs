//! Building an intersection type: nested intersections flattened, every
//! array member folded into one array or shape, and a member dropped when
//! another member is within it.

use crate::{AccessoryKind, Relation, Shape, ShapeField, ShapeKey, Type, union_types};

/// The intersection of `members`, with no class knowledge. `list<int>&
/// non-empty-array` is one array (`int` values, `int` keys) carrying both
/// refinements, i.e. `non-empty-list<int>`.
pub fn intersect_types(members: Vec<Type>) -> Type {
    Relation::structural().intersect(members)
}

impl Relation<'_> {
    /// The intersection of `members`, as PHPStan's `TypeCombinator::
    /// intersect` reduces it: a member another member is within is
    /// dropped (`Expr&Dim` is `Dim`), two arrays or shapes fold into one,
    /// an offset or accessory a shape can hold is applied to it, and
    /// members sharing no value make `never`.
    pub fn intersect(&self, members: Vec<Type>) -> Type {
        let mut flat: Vec<Type> = Vec::new();
        for member in members {
            match member {
                Type::Intersection(inner) => flat.extend(inner),
                other => flat.push(other),
            }
        }
        if flat.contains(&Type::Never) {
            return Type::Never;
        }
        let mut array: Option<(Type, Type)> = None;
        let mut shape: Option<Shape> = None;
        let mut rest: Vec<Type> = Vec::new();
        for member in flat {
            match member {
                Type::Array(key, value) => {
                    array = Some(match array {
                        None => (*key, *value),
                        Some((k, v)) => (self.narrower(k, *key), self.narrower(v, *value)),
                    });
                }
                Type::ArrayShape(s) => {
                    shape = match shape {
                        None => Some(s),
                        Some(acc) => match self.intersect_shapes(&acc, &s) {
                            Some(merged) => Some(merged),
                            None => return Type::Never,
                        },
                    };
                }
                other if !rest.contains(&other) => rest.push(other),
                _ => {}
            }
        }
        let mut out: Vec<Type> = Vec::new();
        match (shape, array) {
            (Some(s), array) => {
                let s = match array {
                    Some((key, value)) => match self.shape_within_array(&s, &key, &value) {
                        Some(s) => s,
                        None => return Type::Never,
                    },
                    None => s,
                };
                let Some(s) = self.apply_offsets(s, &mut rest) else {
                    return Type::Never;
                };
                out.push(Type::ArrayShape(s));
            }
            (None, Some((key, value))) => out.push(Type::Array(Box::new(key), Box::new(value))),
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
                return Type::Never;
            }
        }
        match kept.len() {
            0 => Type::Mixed(false),
            1 => kept.pop().expect("one member"),
            _ => Type::Intersection(kept),
        }
    }

    /// Real PHPStan's `MutatingScope::intersectButNotNever`: a hinted
    /// closure's return is its native type narrowed by what the body
    /// actually returns. An inferred type the native one can't hold at all
    /// (a fall-off-the-end `void` under `: array`) leaves the native type
    /// as-is, and a nullable native type keeps its `null`. An `int` returned
    /// under `: float` is coerced at runtime, so it reads as the `float`.
    pub fn intersect_but_not_never(&self, native: &Type, inferred: Type) -> Type {
        let native_members: Vec<&Type> = match native {
            Type::Union(ms) => ms.iter().collect(),
            other => vec![other],
        };
        let inferred_members = match inferred {
            Type::Union(ms) => ms,
            other => vec![other],
        };
        let mut kept: Vec<Type> = Vec::new();
        for m in inferred_members {
            if m.is_mixed() {
                return native.clone();
            }
            let int_like = matches!(m, Type::Int | Type::IntLiteral(_) | Type::IntRange(..));
            let fits = !matches!(m, Type::Void)
                && native_members
                    .iter()
                    .any(|n| !(int_like && matches!(n, Type::Float)) && self.accepts(n, &m).yes());
            if fits {
                kept.push(m);
                continue;
            }
            kept.extend(
                native_members
                    .iter()
                    .filter(|n| !matches!(n, Type::Null) && self.accepts(&m, n).yes())
                    .map(|n| (*n).clone()),
            );
        }
        if kept.is_empty() {
            return native.clone();
        }
        if native_members.iter().any(|n| matches!(n, Type::Null)) {
            kept.push(Type::Null);
        }
        kept.sort_by_key(|m| *m == Type::Null);
        kept.into_iter()
            .reduce(union_types)
            .unwrap_or_else(|| native.clone())
    }

    /// The narrower of two types when one contains the other, `never`
    /// when they share no value, both otherwise.
    fn narrower(&self, a: Type, b: Type) -> Type {
        if matches!(b, Type::Mixed(_)) || self.is_super_type_of(&b, &a).yes() {
            a
        } else if matches!(a, Type::Mixed(_)) || self.is_super_type_of(&a, &b).yes() {
            b
        } else if self.disjoint(&a, &b) {
            Type::Never
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
                if field.ty == Type::Never {
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
        Some(Shape { fields, unsealed })
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
        };
        self.intersect_shapes(shape, &tail)
    }

    /// `shape` with the offsets and array accessories among `rest`
    /// applied, each one it absorbs removed from `rest`. `None` when
    /// `shape` can't have one of them.
    fn apply_offsets(&self, mut shape: Shape, rest: &mut Vec<Type>) -> Option<Shape> {
        let mut unapplied = Vec::new();
        for member in rest.drain(..) {
            match member {
                Type::HasOffset(ref key) => match shape.field(key).cloned() {
                    Some(f) => shape.set(ShapeField::new(f.key, f.ty, false)),
                    None if shape.is_sealed() => return None,
                    None => unapplied.push(member),
                },
                Type::HasOffsetValue(ref key, ref value) => match shape.field(key).cloned() {
                    Some(f) => {
                        let ty = self.narrower(f.ty, (**value).clone());
                        if ty == Type::Never {
                            return None;
                        }
                        shape.set(ShapeField::new(f.key, ty, false));
                    }
                    None if shape.is_sealed() => return None,
                    None => unapplied.push(member),
                },
                Type::Accessory(AccessoryKind::NonEmptyArray)
                    if shape.is_sealed() && shape.fields.is_empty() =>
                {
                    return None;
                }
                other => unapplied.push(other),
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
        ty,
        Type::Accessory(_)
            | Type::HasOffset(_)
            | Type::HasOffsetValue(..)
            | Type::HasMethod(_)
            | Type::HasProperty(_)
            | Type::ReflectedMember(..)
    )
}
