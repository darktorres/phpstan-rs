//! Types rewritten into related types: `static` bound, shapes unsealed,
//! a value stepped by `++`/`--`.

use std::sync::Arc;

use crate::{
    AccessoryKind, CallableParam, CallableSig, Name, Shape, ShapeField, TemplateParam, Type,
    union_types,
};

impl Type {
    /// This type with `static` and `$this` as `class`, at any depth.
    pub fn bind_static(&self, class: &str) -> Type {
        match self {
            Type::StaticRef(args) => Type::Object(
                class.into(),
                args.iter().map(|a| a.bind_static(class)).collect(),
            ),
            Type::This => Type::Object(class.into(), Vec::new()),
            other => other.map_children(&mut |t| t.bind_static(class)),
        }
    }

    /// This type with `self` as `class`, at any depth.
    pub fn bind_self(&self, class: &str) -> Type {
        match self {
            Type::SelfRef(args) => Type::Object(
                class.into(),
                args.iter().map(|a| a.bind_self(class)).collect(),
            ),
            other => other.map_children(&mut |t| t.bind_self(class)),
        }
    }

    /// This type with `parent` as `class`, at any depth.
    pub fn bind_parent(&self, class: &str) -> Type {
        match self {
            Type::ParentRef => Type::Object(class.into(), Vec::new()),
            other => other.map_children(&mut |t| t.bind_parent(class)),
        }
    }

    /// This type with each object whose class `replace` answers for
    /// replaced by that answer, generic arguments and all; every other
    /// type's children are walked.
    pub fn replace_objects(&self, replace: &mut dyn FnMut(&Name) -> Option<Type>) -> Type {
        if let Type::Object(name, _) = self
            && let Some(replacement) = replace(name)
        {
            return replacement;
        }
        self.map_children(&mut |t| t.replace_objects(replace))
    }

    /// This type with each object's class renamed by `rename`, at any
    /// depth: generic arguments, and callable signatures with their
    /// templates' bounds and defaults. The `Closure` of a `Closure(...)`
    /// shape is PHP's own and keeps its name.
    pub fn map_class_names(&self, rename: &mut dyn FnMut(&Name) -> Name) -> Type {
        match self {
            Type::Object(name, args) => Type::Object(
                rename(name),
                args.iter().map(|a| a.map_class_names(rename)).collect(),
            ),
            Type::Intersection(members)
                if matches!(
                    members.as_slice(),
                    [Type::Object(name, args), Type::Callable(Some(_))]
                        if name == "Closure" && args.is_empty()
                ) =>
            {
                Type::Intersection(vec![members[0].clone(), members[1].map_class_names(rename)])
            }
            Type::Callable(Some(sig)) => {
                let mut map = |t: &Type| t.map_class_names(rename);
                Type::Callable(Some(Arc::new(
                    CallableSig {
                        params: sig
                            .params
                            .iter()
                            .map(|p| CallableParam {
                                ty: map(&p.ty),
                                ..p.clone()
                            })
                            .collect(),
                        return_type: Box::new(map(&sig.return_type)),
                        templates: sig
                            .templates
                            .iter()
                            .map(|t| TemplateParam {
                                bound: Box::new(map(&t.bound)),
                                default: t.default.as_deref().map(|d| Box::new(map(d))),
                                ..t.clone()
                            })
                            .collect(),
                    }
                    .rebind_templates(),
                )))
            }
            _ => self.map_children(&mut |t| t.map_class_names(rename)),
        }
    }

    /// This type with each sealed array shape unsealed, so a key it
    /// doesn't list isn't proven absent.
    pub fn unsealed_shapes(&self) -> Type {
        match self {
            Type::ArrayShape(shape) if shape.is_sealed() => {
                Type::ArrayShape(Shape::unsealed(shape.fields.clone()))
            }
            Type::Union(members) => {
                Type::Union(members.iter().map(Type::unsealed_shapes).collect())
            }
            other => other.clone(),
        }
    }

    /// The type after `++`: `null` becomes `1` (an `int`), a literal or
    /// range moves up by one.
    pub fn increment_type(&self) -> Type {
        self.step(1)
    }

    /// The type after `--`: a literal or range moves down by one; `null`
    /// stays `null`.
    pub fn decrement_type(&self) -> Type {
        self.step(-1)
    }

    fn step(&self, by: i64) -> Type {
        match self {
            Type::Null if by > 0 => Type::Int,
            Type::IntLiteral(n) => Type::IntLiteral(n + by),
            Type::FloatLiteral(f) => Type::FloatLiteral(f + by as f64),
            Type::IntRange(min, max) => Type::IntRange(min.map(|v| v + by), max.map(|v| v + by)),
            Type::Union(members) => members
                .iter()
                .map(|m| m.step(by))
                .reduce(union_types)
                .unwrap_or(Type::Mixed(false)),
            other => other.clone(),
        }
    }

    /// This type with each array value passed through `f`: a general
    /// array's, and each shape field's and unsealed tail's, in union and
    /// intersection members too. Other types are left as they are.
    pub fn map_array_values(&self, f: &mut dyn FnMut(&Type) -> Type) -> Type {
        match self {
            Type::Array(key, value) => Type::Array(key.clone(), Box::new(f(value))),
            Type::ArrayShape(shape) => Type::ArrayShape(shape.map_types(&mut |t| f(t))),
            Type::Union(_) | Type::BenevolentUnion(_) | Type::Intersection(_) => {
                self.map_children(&mut |m| m.map_array_values(&mut *f))
            }
            other => other.clone(),
        }
    }

    /// This type with each general array's value type replaced by
    /// `value`, in union and intersection members too; an array shape
    /// keeps its fields.
    pub fn with_general_array_value(&self, value: &Type) -> Type {
        match self {
            Type::Array(key, _) => Type::Array(key.clone(), Box::new(value.clone())),
            Type::Union(_) | Type::BenevolentUnion(_) | Type::Intersection(_) => {
                self.map_children(&mut |m| m.with_general_array_value(value))
            }
            other => other.clone(),
        }
    }

    /// This array with each value passed through `keep`: a shape field
    /// `keep` turns into `never` is dropped, and one it changes may be
    /// dropped (becomes optional). Other types are left as they are.
    pub fn filter_array_values(&self, keep: &dyn Fn(&Type) -> Type) -> Type {
        match self {
            Type::Array(key, value) => Type::Array(key.clone(), Box::new(keep(value))),
            Type::ArrayShape(shape) => Type::ArrayShape(Shape {
                fields: shape
                    .fields
                    .iter()
                    .filter_map(|field| {
                        let kept = keep(&field.ty);
                        if kept.is_never() {
                            return None;
                        }
                        let maybe_dropped = kept != field.ty;
                        Some(ShapeField::new(
                            field.key.clone(),
                            kept,
                            field.optional || maybe_dropped,
                        ))
                    })
                    .collect(),
                unsealed: shape.unsealed.clone(),
            }),
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|m| m.filter_array_values(keep))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// This type without its accessory refinements: an intersection's
    /// base, in each union member.
    pub fn without_accessories(&self) -> Type {
        match self {
            Type::Intersection(members) => members
                .iter()
                .find(|m| !matches!(m, Type::Accessory(_)))
                .cloned()
                .unwrap_or(Type::Mixed(false)),
            Type::Union(members) => members
                .iter()
                .map(Type::without_accessories)
                .reduce(union_types)
                .unwrap_or(Type::Mixed(false)),
            other => other.clone(),
        }
    }

    /// An array of `value`s keeping what this array is known to be: a
    /// list, non-empty.
    pub fn with_array_value(&self, value: Type) -> Type {
        let mut members = vec![Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(value),
        )];
        match self {
            Type::Intersection(existing) => members.extend(
                existing
                    .iter()
                    .filter(|m| matches!(m, Type::Accessory(_)))
                    .cloned(),
            ),
            Type::Union(_) => {
                if self.is_list().yes() {
                    members.push(Type::Accessory(AccessoryKind::ArrayList));
                }
                if self.is_non_empty_array().yes() {
                    members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
                }
            }
            _ => {}
        }
        if members.len() == 1 {
            members.pop().expect("one member")
        } else {
            Type::Intersection(members)
        }
    }
}
