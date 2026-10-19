//! Walking a type's directly nested types.

use std::sync::Arc;

use crate::{CallableParam, CallableSig, Repr, Shape, ShapeField, TemplateParam, Type};

impl Type {
    /// This type with `f` applied to each directly nested type (generic
    /// arguments, array key/value, shape fields, union/intersection
    /// members, callable parameters and return, conditional branches);
    /// a leaf is returned unchanged. `f` decides whether to recurse.
    pub fn map_children(&self, f: &mut impl FnMut(&Type) -> Type) -> Type {
        stack::guard(|| match self.repr() {
            Repr::Array(k, v) => {
                let (nk, nv) = (f(k), f(v));
                if same(&nk, k) && same(&nv, v) {
                    self.clone()
                } else {
                    Type::array(nk, nv)
                }
            }
            Repr::ClassString(inner) => self.rebuilt_by(inner, f, Type::class_string),
            Repr::ArrayShape(shape) => match map_shape(shape, f) {
                Some(shape) => Type::array_shape(shape),
                None => self.clone(),
            },
            Repr::ObjectShape(shape) => match map_shape(shape, f) {
                Some(shape) => Type::object_shape(shape),
                None => self.clone(),
            },
            Repr::TemplateParam(tp) => match map_template(tp, f) {
                Some(tp) => Type::template_param(tp),
                None => self.clone(),
            },
            Repr::Object(name, args) => match map_all(args, f) {
                Some(args) => Type::object(name.clone(), args),
                None => self.clone(),
            },
            Repr::SelfRef(args) => match map_all(args, f) {
                Some(args) => Type::self_ref(args),
                None => self.clone(),
            },
            Repr::StaticRef(args) => match map_all(args, f) {
                Some(args) => Type::static_ref(args),
                None => self.clone(),
            },
            Repr::Callable(Some(sig)) => {
                let mut changed = false;
                let params: Vec<CallableParam> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let ty = f(&p.ty);
                        changed |= !same(&ty, &p.ty);
                        CallableParam { ty, ..p.clone() }
                    })
                    .collect();
                let return_type = f(&sig.return_type);
                changed |= !same(&return_type, &sig.return_type);
                let templates: Vec<TemplateParam> = sig
                    .templates
                    .iter()
                    .map(|tp| match map_template(tp, f) {
                        Some(mapped) => {
                            changed = true;
                            mapped
                        }
                        None => tp.clone(),
                    })
                    .collect();
                if !changed {
                    return self.clone();
                }
                Type::callable(Some(Arc::new(CallableSig {
                    params,
                    return_type: Box::new(return_type),
                    templates,
                })))
            }
            Repr::VarianceArg(variance, inner) => {
                self.rebuilt_by(inner, f, |inner| Type::variance_arg(*variance, inner))
            }
            // Rebuilt as a union, so members a mapping made equal merge.
            Repr::Union(ms) => Type::union_of(ms.iter().map(&mut *f).collect()),
            Repr::BenevolentUnion(ms) => {
                Type::benevolent_union_of(ms.iter().map(&mut *f).collect())
            }
            Repr::Intersection(ms) => match map_all(ms, f) {
                Some(members) => Type::new(Repr::Intersection(members)),
                None => self.clone(),
            },
            Repr::HasOffsetValue(key, inner) => {
                self.rebuilt_by(inner, f, |inner| Type::has_offset_value(key.clone(), inner))
            }
            Repr::ReflectedMember(owner, member) => self.rebuilt_by(owner, f, |owner| {
                Type::reflected_member(owner, member.clone())
            }),
            Repr::ValueOf(inner) => self.rebuilt_by(inner, f, Type::value_of),
            Repr::KeyOf(inner) => self.rebuilt_by(inner, f, Type::key_of),
            Repr::OffsetAccess(base, key) => {
                let (nb, nk) = (f(base), f(key));
                if same(&nb, base) && same(&nk, key) {
                    self.clone()
                } else {
                    Type::offset_access(nb, nk)
                }
            }
            Repr::ClassConstant { class, pattern } => self.rebuilt_by(class, f, |class| {
                Type::new(Repr::ClassConstant {
                    class,
                    pattern: pattern.clone(),
                })
            }),
            Repr::Conditional {
                subject,
                target,
                if_true,
                if_false,
                negated,
            } => {
                let (nt, nx, ny) = (f(target), f(if_true), f(if_false));
                if same(&nt, target) && same(&nx, if_true) && same(&ny, if_false) {
                    self.clone()
                } else {
                    Type::new(Repr::Conditional {
                        subject: subject.clone(),
                        target: nt,
                        if_true: nx,
                        if_false: ny,
                        negated: *negated,
                    })
                }
            }
            _ => self.clone(),
        })
    }

    /// This type with its one child `inner` mapped by `f`, rebuilt by
    /// `rebuild` from the mapped child, or as it is when `f` left the child
    /// as it was.
    fn rebuilt_by(
        &self,
        inner: &Type,
        f: &mut impl FnMut(&Type) -> Type,
        rebuild: impl FnOnce(Type) -> Type,
    ) -> Type {
        let mapped = f(inner);
        if same(&mapped, inner) {
            self.clone()
        } else {
            rebuild(mapped)
        }
    }

    /// Calls `f` on each directly nested type - the children
    /// `map_children` rebuilds, in the same order.
    pub fn for_each_child<'t>(&'t self, f: &mut impl FnMut(&'t Type)) {
        for_each_repr_child(self.repr(), f);
    }

    /// Whether `f` holds for some directly nested type.
    pub fn any_child(&self, mut f: impl FnMut(&Type) -> bool) -> bool {
        let mut found = false;
        self.for_each_child(&mut |t| found = found || f(t));
        found
    }
}

/// `Type::for_each_child` for a representation not yet a type.
pub(crate) fn for_each_repr_child<'t>(repr: &'t Repr, f: &mut impl FnMut(&'t Type)) {
    stack::guard(|| match repr {
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
            for field in shape.fields() {
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

/// Whether `a` and `b` are the same node, so equal.
fn same(a: &Type, b: &Type) -> bool {
    Arc::ptr_eq(&a.0, &b.0)
}

/// `items` with `f` applied to each, in order; `None` when `f` left every
/// one as it was.
fn map_all(items: &[Type], f: &mut impl FnMut(&Type) -> Type) -> Option<Vec<Type>> {
    let mut out: Option<Vec<Type>> = None;
    for (at, item) in items.iter().enumerate() {
        let mapped = f(item);
        match &mut out {
            Some(out) => out.push(mapped),
            None if same(&mapped, item) => {}
            None => {
                let mut changed = Vec::with_capacity(items.len());
                changed.extend(items[..at].iter().cloned());
                changed.push(mapped);
                out = Some(changed);
            }
        }
    }
    out
}

/// A shape with `f` applied to each field's type and to its unsealed
/// tail's key and value types; `None` when `f` left each as it was.
fn map_shape(shape: &Shape, f: &mut impl FnMut(&Type) -> Type) -> Option<Shape> {
    let mut changed = false;
    let fields: Vec<ShapeField> = shape
        .fields()
        .iter()
        .map(|field| {
            let ty = f(&field.ty);
            changed |= !same(&ty, &field.ty);
            ShapeField::new(field.key.clone(), ty, field.optional)
        })
        .collect();
    let unsealed = shape.unsealed.as_deref().map(|(key, value)| {
        let (nk, nv) = (f(key), f(value));
        changed |= !same(&nk, key) || !same(&nv, value);
        Box::new((nk, nv))
    });
    changed.then(|| Shape::from_parts(fields, unsealed, shape.next_index))
}

/// A template with `f` applied to its bound and default; `None` when `f`
/// left both as they were.
fn map_template(tp: &TemplateParam, f: &mut impl FnMut(&Type) -> Type) -> Option<TemplateParam> {
    let bound = f(&tp.bound);
    let default = tp.default.as_deref().map(|d| (f(d), d));
    let changed = !same(&bound, &tp.bound) || default.as_ref().is_some_and(|(n, d)| !same(n, d));
    changed.then(|| TemplateParam {
        bound: Box::new(bound),
        default: default.map(|(mapped, _)| Box::new(mapped)),
        ..tp.clone()
    })
}

/// A template's bound and default.
fn template_children<'t>(tp: &'t TemplateParam, f: &mut impl FnMut(&'t Type)) {
    f(&tp.bound);
    if let Some(default) = tp.default.as_deref() {
        f(default);
    }
}
