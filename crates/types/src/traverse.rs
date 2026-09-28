//! Walking a type's directly nested types.

use std::sync::Arc;

use crate::{CallableParam, CallableSig, Repr, Type};

impl Type {
    /// This type with `f` applied to each directly nested type (generic
    /// arguments, array key/value, shape fields, union/intersection
    /// members, callable parameters and return, conditional branches);
    /// a leaf is returned unchanged. `f` decides whether to recurse.
    pub fn map_children(&self, f: &mut impl FnMut(&Type) -> Type) -> Type {
        match self {
            Type(Repr::Array(k, v)) => Type::array(f(k), f(v)),
            Type(Repr::ClassString(inner)) => Type::class_string(f(inner)),
            Type(Repr::ArrayShape(shape)) => Type::array_shape(shape.map_types(f)),
            Type(Repr::ObjectShape(shape)) => Type::object_shape(shape.map_types(f)),
            Type(Repr::Object(name, args)) => {
                Type::object(name.clone(), args.iter().map(&mut *f).collect())
            }
            Type(Repr::SelfRef(args)) => Type::self_ref(args.iter().map(&mut *f).collect()),
            Type(Repr::StaticRef(args)) => Type::static_ref(args.iter().map(&mut *f).collect()),
            Type(Repr::Callable(Some(sig))) => Type::callable(Some(Arc::new(CallableSig {
                params: sig
                    .params
                    .iter()
                    .map(|p| CallableParam {
                        ty: f(&p.ty),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(f(&sig.return_type)),
                templates: sig.templates.clone(),
            }))),
            Type(Repr::VarianceArg(variance, inner)) => Type::variance_arg(*variance, f(inner)),
            Type(Repr::Union(ms)) => Type(Repr::Union(ms.iter().map(&mut *f).collect())),
            Type(Repr::BenevolentUnion(ms)) => {
                Type(Repr::BenevolentUnion(ms.iter().map(&mut *f).collect()))
            }
            Type(Repr::Intersection(ms)) => {
                Type(Repr::Intersection(ms.iter().map(&mut *f).collect()))
            }
            Type(Repr::HasOffsetValue(key, inner)) => Type::has_offset_value(key.clone(), f(inner)),
            Type(Repr::ReflectedMember(owner, member)) => {
                Type::reflected_member(f(owner), member.clone())
            }
            Type(Repr::ValueOf(inner)) => Type::value_of(f(inner)),
            Type(Repr::KeyOf(inner)) => Type::key_of(f(inner)),
            Type(Repr::OffsetAccess(base, key)) => Type::offset_access(f(base), f(key)),
            Type(Repr::ClassConstant { class, pattern }) => Type(Repr::ClassConstant {
                class: Box::new(f(class)),
                pattern: pattern.clone(),
            }),
            Type(Repr::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
                template_subject,
            }) => Type(Repr::Conditional {
                subject: subject.clone(),
                target: Box::new(f(target)),
                if_true: Box::new(f(if_true)),
                if_false: Box::new(f(if_false)),
                negated: *negated,
                template_subject: *template_subject,
            }),
            leaf => leaf.clone(),
        }
    }

    /// Calls `f` on each directly nested type - the children
    /// `map_children` rebuilds, in the same order.
    pub fn for_each_child<'t>(&'t self, f: &mut impl FnMut(&'t Type)) {
        match self {
            Type(Repr::Array(k, v)) => {
                f(k);
                f(v);
            }
            Type(Repr::ClassString(inner))
            | Type(Repr::VarianceArg(_, inner))
            | Type(Repr::HasOffsetValue(_, inner))
            | Type(Repr::ReflectedMember(inner, _))
            | Type(Repr::ValueOf(inner))
            | Type(Repr::KeyOf(inner))
            | Type(Repr::ClassConstant { class: inner, .. }) => f(inner),
            Type(Repr::OffsetAccess(base, key)) => {
                f(base);
                f(key);
            }
            Type(Repr::ArrayShape(shape)) | Type(Repr::ObjectShape(shape)) => {
                shape.types().for_each(f)
            }
            Type(Repr::Object(_, args))
            | Type(Repr::SelfRef(args))
            | Type(Repr::StaticRef(args)) => args.iter().for_each(f),
            Type(Repr::Callable(Some(sig))) => {
                for p in &sig.params {
                    f(&p.ty);
                }
                f(&sig.return_type);
            }
            Type(Repr::Union(ms))
            | Type(Repr::BenevolentUnion(ms))
            | Type(Repr::Intersection(ms)) => ms.iter().for_each(f),
            Type(Repr::Conditional {
                target,
                if_true,
                if_false,
                ..
            }) => {
                f(target);
                f(if_true);
                f(if_false);
            }
            _ => {}
        }
    }

    /// Whether `f` holds for some directly nested type.
    pub fn any_child(&self, mut f: impl FnMut(&Type) -> bool) -> bool {
        let mut found = false;
        self.for_each_child(&mut |t| found = found || f(t));
        found
    }
}
