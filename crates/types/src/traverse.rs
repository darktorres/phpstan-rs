//! Walking a type's directly nested types.

use std::sync::Arc;

use crate::{CallableParam, CallableSig, Repr, Shape, ShapeField, TemplateParam, Type};

impl Type {
    /// This type with `f` applied to each directly nested type (generic
    /// arguments, array key/value, shape fields, union/intersection
    /// members, callable parameters and return, conditional branches);
    /// a leaf is returned unchanged. `f` decides whether to recurse.
    pub fn map_children(&self, f: &mut impl FnMut(&Type) -> Type) -> Type {
        syntax::stack::guard(|| match self.repr() {
            Repr::Array(k, v) => Type::array(f(k), f(v)),
            Repr::ClassString(inner) => Type::class_string(f(inner)),
            Repr::ArrayShape(shape) => Type::array_shape(map_shape(shape, f)),
            Repr::ObjectShape(shape) => Type::object_shape(map_shape(shape, f)),
            Repr::TemplateParam(tp) => Type::template_param(map_template(tp, f)),
            Repr::Object(name, args) => {
                Type::object(name.clone(), args.iter().map(&mut *f).collect())
            }
            Repr::SelfRef(args) => Type::self_ref(args.iter().map(&mut *f).collect()),
            Repr::StaticRef(args) => Type::static_ref(args.iter().map(&mut *f).collect()),
            Repr::Callable(Some(sig)) => Type::callable(Some(Arc::new(CallableSig {
                params: sig
                    .params
                    .iter()
                    .map(|p| CallableParam {
                        ty: f(&p.ty),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(f(&sig.return_type)),
                templates: sig.templates.iter().map(|tp| map_template(tp, f)).collect(),
            }))),
            Repr::VarianceArg(variance, inner) => Type::variance_arg(*variance, f(inner)),
            // Rebuilt as a union, so members a mapping made equal merge.
            Repr::Union(ms) => Type::union_of(ms.iter().map(&mut *f).collect()),
            Repr::BenevolentUnion(ms) => {
                Type::benevolent_union_of(ms.iter().map(&mut *f).collect())
            }
            Repr::Intersection(ms) => {
                Type::new(Repr::Intersection(ms.iter().map(&mut *f).collect()))
            }
            Repr::HasOffsetValue(key, inner) => Type::has_offset_value(key.clone(), f(inner)),
            Repr::ReflectedMember(owner, member) => {
                Type::reflected_member(f(owner), member.clone())
            }
            Repr::ValueOf(inner) => Type::value_of(f(inner)),
            Repr::KeyOf(inner) => Type::key_of(f(inner)),
            Repr::OffsetAccess(base, key) => Type::offset_access(f(base), f(key)),
            Repr::ClassConstant { class, pattern } => Type::new(Repr::ClassConstant {
                class: Arc::new(f(class)),
                pattern: pattern.clone(),
            }),
            Repr::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
            } => Type::new(Repr::Conditional {
                subject: subject.clone(),
                target: Arc::new(f(target)),
                if_true: Arc::new(f(if_true)),
                if_false: Arc::new(f(if_false)),
                negated: *negated,
            }),
            _ => self.clone(),
        })
    }

    /// Calls `f` on each directly nested type - the children
    /// `map_children` rebuilds, in the same order.
    pub fn for_each_child<'t>(&'t self, f: &mut impl FnMut(&'t Type)) {
        syntax::stack::guard(|| match self.repr() {
            Repr::Array(k, v) => {
                f(k);
                f(v);
            }
            Repr::ClassString(inner)
            | Repr::VarianceArg(_, inner)
            | Repr::HasOffsetValue(_, inner)
            | Repr::ReflectedMember(inner, _)
            | Repr::ValueOf(inner)
            | Repr::KeyOf(inner)
            | Repr::ClassConstant { class: inner, .. } => f(inner),
            Repr::OffsetAccess(base, key) => {
                f(base);
                f(key);
            }
            Repr::ArrayShape(shape) | Repr::ObjectShape(shape) => {
                for field in &shape.fields {
                    f(&field.ty);
                }
                if let Some((key, value)) = shape.unsealed.as_deref() {
                    f(key);
                    f(value);
                }
            }
            Repr::TemplateParam(tp) => template_children(tp, f),
            Repr::Object(_, args) | Repr::SelfRef(args) | Repr::StaticRef(args) => {
                args.iter().for_each(f)
            }
            Repr::Callable(Some(sig)) => {
                for p in &sig.params {
                    f(&p.ty);
                }
                f(&sig.return_type);
                for tp in &sig.templates {
                    template_children(tp, f);
                }
            }
            Repr::Union(ms) | Repr::BenevolentUnion(ms) | Repr::Intersection(ms) => {
                ms.iter().for_each(f)
            }
            Repr::Conditional {
                target,
                if_true,
                if_false,
                ..
            } => {
                f(target);
                f(if_true);
                f(if_false);
            }
            _ => {}
        })
    }

    /// Whether `f` holds for some directly nested type.
    pub fn any_child(&self, mut f: impl FnMut(&Type) -> bool) -> bool {
        let mut found = false;
        self.for_each_child(&mut |t| found = found || f(t));
        found
    }
}

/// A shape with `f` applied to each field's type and to its unsealed
/// tail's key and value types.
fn map_shape(shape: &Shape, f: &mut impl FnMut(&Type) -> Type) -> Shape {
    Shape {
        fields: shape
            .fields
            .iter()
            .map(|field| ShapeField::new(field.key.clone(), f(&field.ty), field.optional))
            .collect(),
        unsealed: shape
            .unsealed
            .as_deref()
            .map(|(key, value)| Box::new((f(key), f(value)))),
        next_index: shape.next_index,
    }
}

/// A template with `f` applied to its bound and default.
fn map_template(tp: &TemplateParam, f: &mut impl FnMut(&Type) -> Type) -> TemplateParam {
    TemplateParam {
        bound: Box::new(f(&tp.bound)),
        default: tp.default.as_deref().map(|d| Box::new(f(d))),
        ..tp.clone()
    }
}

/// A template's bound and default.
fn template_children<'t>(tp: &'t TemplateParam, f: &mut impl FnMut(&'t Type)) {
    f(&tp.bound);
    if let Some(default) = tp.default.as_deref() {
        f(default);
    }
}
