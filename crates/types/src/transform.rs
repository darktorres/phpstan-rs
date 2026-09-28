//! Types rewritten into related types: `static` bound, shapes unsealed,
//! a value stepped by `++`/`--`.

use std::sync::Arc;

use crate::{
    AccessoryKind, CallableParam, CallableSig, Name, PhpString, Repr, Shape, ShapeField,
    TemplateParam, Type,
};

impl Type {
    /// This type with `static` and `$this` as `class`, at any depth.
    pub fn bind_static(&self, class: &str) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::StaticRef(args)) => Type::object(
                class.into(),
                args.iter().map(|a| a.bind_static(class)).collect(),
            ),
            Type(Repr::This) => Type::object(class.into(), Vec::new()),
            other => other.map_children(&mut |t| t.bind_static(class)),
        })
    }

    /// This type with `self` as `class`, at any depth.
    pub fn bind_self(&self, class: &str) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::SelfRef(args)) => Type::object(
                class.into(),
                args.iter().map(|a| a.bind_self(class)).collect(),
            ),
            other => other.map_children(&mut |t| t.bind_self(class)),
        })
    }

    /// This type with `parent` as `class`, at any depth.
    pub fn bind_parent(&self, class: &str) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::ParentRef) => Type::object(class.into(), Vec::new()),
            other => other.map_children(&mut |t| t.bind_parent(class)),
        })
    }

    /// This type with each object whose class `replace` answers for
    /// replaced by that answer, generic arguments and all; every other
    /// type's children are walked.
    pub fn replace_objects(&self, replace: &mut dyn FnMut(&Name) -> Option<Type>) -> Type {
        syntax::stack::guard(|| {
            if let Type(Repr::Object(name, _)) = self
                && let Some(replacement) = replace(name)
            {
                return replacement;
            }
            self.map_children(&mut |t| t.replace_objects(replace))
        })
    }

    /// This type with each object's class renamed by `rename`, at any
    /// depth: generic arguments, and callable signatures with their
    /// templates' bounds and defaults. The `Closure` of a `Closure(...)`
    /// shape is PHP's own and keeps its name.
    pub fn map_class_names(&self, rename: &mut dyn FnMut(&Name) -> Name) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::Object(name, args)) => Type::object(
                rename(name),
                args.iter().map(|a| a.map_class_names(rename)).collect(),
            ),
            Type(Repr::Intersection(members))
                if matches!(
                    members.as_slice(),
                    [Type(Repr::Object(name, args)), Type(Repr::Callable(Some(_)))]
                        if name == "Closure" && args.is_empty()
                ) =>
            {
                Type(Repr::Intersection(vec![
                    members[0].clone(),
                    members[1].map_class_names(rename),
                ]))
            }
            Type(Repr::Callable(Some(sig))) => {
                let mut map = |t: &Type| t.map_class_names(rename);
                Type::callable(Some(Arc::new(
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
        })
    }

    /// This type with each sealed array shape unsealed, so a key it
    /// doesn't list isn't proven absent.
    pub fn unsealed_shapes(&self) -> Type {
        match self {
            Type(Repr::ArrayShape(shape)) if shape.is_sealed() => {
                Type::array_shape(Shape::unsealed(shape.fields.clone()))
            }
            Type(Repr::Union(members)) => Type(Repr::Union(
                members.iter().map(Type::unsealed_shapes).collect(),
            )),
            other => other.clone(),
        }
    }

    /// The type after `++`: `null` becomes `1`, a number or numeric
    /// string moves up by one, `''` becomes `'1'` and any other string
    /// counts up alphanumerically (`'a'` to `'b'`, `'Az'` to `'Ba'`).
    pub fn increment_type(&self) -> Type {
        self.step(1)
    }

    /// The type after `--`: a number or numeric string moves down by one
    /// and `''` becomes `-1`; `null` and any other string stay as they
    /// are.
    pub fn decrement_type(&self) -> Type {
        self.step(-1)
    }

    fn step(&self, by: i64) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::Null) if by > 0 => Type::int_literal(1),
            // Stepping past `PHP_INT_MAX` or `PHP_INT_MIN` gives a float.
            Type(Repr::IntLiteral(n)) => match n.checked_add(by) {
                Some(v) => Type::int_literal(v),
                None => Type::float_literal(*n as f64 + by as f64),
            },
            Type(Repr::FloatLiteral(f)) => Type::float_literal(f + by as f64),
            Type(Repr::IntRange(min, max)) => {
                let edge = if by > 0 { i64::MAX } else { i64::MIN };
                let stepped = Type::int_interval(
                    min.map(|v| v.checked_add(by).unwrap_or(edge)),
                    max.map(|v| v.checked_add(by).unwrap_or(edge)),
                );
                if [*min, *max].contains(&Some(edge)) {
                    Type::union(stepped, Type::float_literal(edge as f64 + by as f64))
                } else {
                    stepped
                }
            }
            Type(Repr::StringLiteral(s)) => match crate::numeric_value(s.as_bytes()) {
                Some(crate::PhpNumber::Int(n)) => Type::int_literal(n).step(by),
                Some(crate::PhpNumber::Float(f)) => Type::float_literal(f).step(by),
                None if s.as_bytes().is_empty() => {
                    if by > 0 {
                        Type::string_literal(PhpString::from(b"1".to_vec()))
                    } else {
                        Type::int_literal(-1)
                    }
                }
                None if by > 0 => Type::string_literal(PhpString::from(
                    crate::numeric::increment_string(s.as_bytes()),
                )),
                None => self.clone(),
            },
            Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) => members
                .iter()
                .map(|m| m.step(by))
                .reduce(Type::union)
                .unwrap_or(Type::mixed()),
            // A numeric string steps as the number it is; any other string
            // stays a string - non-empty after `++`, unchanged by `--`
            // (bar `''`, which `--` makes `-1`).
            _ if self.is_string().yes() => {
                let number = Type::union(Type::int(), Type::float());
                if self.is_numeric_string().yes() {
                    number
                } else if by > 0 {
                    Type::union(
                        number,
                        Type::string().with_string_accessory(AccessoryKind::NonEmptyString),
                    )
                } else {
                    Type::union(number, self.clone())
                }
            }
            other => other.clone(),
        })
    }

    /// This type with each array value passed through `f`: a general
    /// array's, and each shape field's and unsealed tail's, in union and
    /// intersection members too. Other types are left as they are.
    pub fn map_array_values(&self, f: &mut dyn FnMut(&Type) -> Type) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::Array(key, value)) => Type(Repr::Array(key.clone(), Box::new(f(value)))),
            Type(Repr::ArrayShape(shape)) => Type::array_shape(shape.map_types(&mut |t| f(t))),
            Type(Repr::Union(_)) | Type(Repr::BenevolentUnion(_)) | Type(Repr::Intersection(_)) => {
                self.map_children(&mut |m| m.map_array_values(&mut *f))
            }
            other => other.clone(),
        })
    }

    /// This type with each general array's value type replaced by
    /// `value`, in union and intersection members too; an array shape
    /// keeps its fields.
    pub fn with_general_array_value(&self, value: &Type) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::Array(key, _)) => Type(Repr::Array(key.clone(), Box::new(value.clone()))),
            Type(Repr::Union(_)) | Type(Repr::BenevolentUnion(_)) | Type(Repr::Intersection(_)) => {
                self.map_children(&mut |m| m.with_general_array_value(value))
            }
            other => other.clone(),
        })
    }

    /// This array with each value passed through `keep`: a shape field
    /// `keep` turns into `never` is dropped, and one it changes may be
    /// dropped (becomes optional). Other types are left as they are.
    pub fn filter_array_values(&self, keep: &dyn Fn(&Type) -> Type) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::Array(key, value)) => Type(Repr::Array(key.clone(), Box::new(keep(value)))),
            Type(Repr::ArrayShape(shape)) => Type::array_shape(Shape {
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
            Type(Repr::Union(members)) => Type(Repr::Union(
                members
                    .iter()
                    .map(|m| m.filter_array_values(keep))
                    .collect(),
            )),
            other => other.clone(),
        })
    }

    /// This type without its accessory refinements: an intersection's
    /// base, in each union member.
    pub fn without_accessories(&self) -> Type {
        match self {
            Type(Repr::Intersection(members)) => members
                .iter()
                .find(|m| !matches!(m, Type(Repr::Accessory(_))))
                .cloned()
                .unwrap_or(Type::mixed()),
            Type(Repr::Union(members)) => members
                .iter()
                .map(Type::without_accessories)
                .reduce(Type::union)
                .unwrap_or(Type::mixed()),
            other => other.clone(),
        }
    }

    /// An array of `value`s keeping what this array is known to be: a
    /// list, non-empty.
    pub fn with_array_value(&self, value: Type) -> Type {
        let mut members = vec![Type::array(Type::unknown_array_key(), value)];
        match self {
            Type(Repr::Intersection(existing)) => members.extend(
                existing
                    .iter()
                    .filter(|m| matches!(m, Type(Repr::Accessory(_))))
                    .cloned(),
            ),
            Type(Repr::Union(_)) => {
                if self.is_list().yes() {
                    members.push(Type::accessory(AccessoryKind::ArrayList));
                }
                if self.is_non_empty_array().yes() {
                    members.push(Type::accessory(AccessoryKind::NonEmptyArray));
                }
            }
            _ => {}
        }
        crate::intersect_types(members)
    }
}
