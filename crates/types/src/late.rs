//! Late-resolvable types: `key-of<T>`, `value-of<T>` and `T[K]`, whose
//! answer depends on a `T` that may be a template until a call binds it
//! (PHPStan's `LateResolvableType`).

use crate::{Repr, ShapeKey, Type};

/// What resolving a class reference inside a type needs to know.
pub trait ClassConstants {
    /// `value-of<class>`: an enum's case values; `mixed` for a class
    /// that isn't a known enum.
    fn enum_values(&self, class: &str) -> Type;

    /// The union of the constants of `class` whose names match `pattern`
    /// (`*` matching any run of characters); `mixed` when unknown.
    fn class_constants(&self, class: &Type, pattern: &str) -> Type;
}

impl Type {
    /// This type with each `value-of<Class>` and class constant
    /// reference answered by `consts`, and each late type over them
    /// settled.
    pub fn resolve_class_references(&self, consts: &dyn ClassConstants) -> Type {
        match self {
            Type(Repr::ValueOf(target)) => match target.as_ref() {
                Type(Repr::ClassConstant { .. }) => {
                    value_of(&target.resolve_class_references(consts)).unwrap_or(Type::mixed())
                }
                Type(Repr::Object(class, _)) => consts.enum_values(class),
                _ => Type::mixed(),
            },
            Type(Repr::ClassConstant { class, pattern }) => consts.class_constants(class, pattern),
            Type(Repr::KeyOf(_)) | Type(Repr::OffsetAccess(..)) => {
                settle(self.map_children(&mut |t| t.resolve_class_references(consts)))
            }
            _ => self.map_children(&mut |t| t.resolve_class_references(consts)),
        }
    }

    /// This type with each class constant reference replaced by `f`'s
    /// answer for its class and name pattern, kept where `f` gives none.
    pub fn map_class_constants(&self, f: &mut dyn FnMut(&Type, &str) -> Option<Type>) -> Type {
        match self {
            Type(Repr::ClassConstant { class, pattern }) => {
                f(class, pattern).unwrap_or_else(|| self.clone())
            }
            _ => self.map_children(&mut |t| t.map_class_constants(&mut *f)),
        }
    }
}

/// `key-of<ty>` when `ty` is an array type: its key type. `None` for
/// anything whose keys aren't known from the type alone.
pub(crate) fn key_of(ty: &Type) -> Option<Type> {
    match ty {
        Type(Repr::Mixed(_)) => Some(Type::mixed()),
        Type(Repr::Array(key, _)) => Some((**key).clone()),
        Type(Repr::ArrayShape(shape)) => {
            Some(join(shape.fields.iter().map(|f| f.key.to_type()).collect()))
        }
        Type(Repr::Intersection(members)) => members.iter().find_map(key_of),
        Type(Repr::Union(members)) => members.iter().map(key_of).collect::<Option<_>>().map(join),
        _ => None,
    }
}

/// `value-of<ty>` when `ty` is an array type: its value type. An enum's
/// case values need the registry (`Registry::resolve_deferred_value_of`).
pub(crate) fn value_of(ty: &Type) -> Option<Type> {
    match ty {
        Type(Repr::Mixed(_)) => Some(Type::mixed()),
        Type(Repr::Array(_, value)) => Some((**value).clone()),
        Type(Repr::ArrayShape(shape)) => {
            Some(join(shape.fields.iter().map(|f| f.ty.clone()).collect()))
        }
        Type(Repr::Intersection(members)) => members.iter().find_map(value_of),
        Type(Repr::Union(members)) => members
            .iter()
            .map(value_of)
            .collect::<Option<_>>()
            .map(join),
        _ => None,
    }
}

/// `base[key]` (PHPStan's `OffsetAccessType::getResult()`): the value
/// type `base` holds at `key`. A shape without the key, or a non-array
/// `base`, gives `Mixed`; `None` when `base` isn't known yet.
pub(crate) fn offset_access(base: &Type, key: &Type) -> Option<Type> {
    match base {
        Type(Repr::ArrayShape(shape)) | Type(Repr::ObjectShape(shape)) => Some(
            ShapeKey::from_type(key)
                .and_then(|key| shape.field(&key))
                .map(|field| field.ty.clone())
                .unwrap_or(Type::mixed()),
        ),
        Type(Repr::Array(_, value)) => Some((**value).clone()),
        Type(Repr::Intersection(members)) => members.iter().find_map(|m| offset_access(m, key)),
        Type(Repr::Union(members)) => members
            .iter()
            .map(|m| offset_access(m, key))
            .collect::<Option<_>>()
            .map(join),
        Type(Repr::TemplateParam(_)) | Type(Repr::Object(_, _)) => None,
        _ => Some(Type::mixed()),
    }
}

/// A late-resolvable node answered now that its operand may be known -
/// what a substitution applies to each node it rebuilds. A `key-of`/offset
/// access whose operand no longer holds a template is answered, `Mixed`
/// when the operand isn't an array type; a `value-of` is answered only
/// over an array type (an enum's needs the registry).
pub(crate) fn settle(ty: Type) -> Type {
    match &ty {
        Type(Repr::KeyOf(inner)) | Type(Repr::OffsetAccess(inner, _))
            if !inner.references_template() =>
        {
            resolve_node(&ty, &|t| t.clone()).unwrap_or(Type::mixed())
        }
        Type(Repr::ValueOf(_)) => resolve_node(&ty, &|t| t.clone()).unwrap_or(ty),
        _ => ty,
    }
}

impl Type {
    /// Whether `ty` names a template anywhere: a template parameter, or a
    /// conditional whose subject is one.
    pub fn references_template(&self) -> bool {
        match self {
            Type(Repr::TemplateParam(_)) => true,
            Type(Repr::Conditional {
                template_subject: true,
                ..
            }) => true,
            _ => self.any_child(Type::references_template),
        }
    }
}

impl Type {
    /// `ty` with every late-resolvable node answered as far as its
    /// templates' bounds allow - what a template-typed value can be relied
    /// on to be inside the generic code itself. An answer the bound doesn't
    /// give is `Mixed`.
    pub fn resolve_late_to_bounds(&self) -> Type {
        let ty = self.map_children(&mut Type::resolve_late_to_bounds);
        match ty {
            Type(Repr::KeyOf(_)) | Type(Repr::OffsetAccess(..)) => {
                resolve_node(&ty, &upper_bound).unwrap_or(Type::mixed())
            }
            Type(Repr::ValueOf(ref inner)) if matches!(**inner, Type(Repr::TemplateParam(_))) => {
                resolve_node(&ty, &upper_bound).unwrap_or(Type::mixed())
            }
            other => other,
        }
    }
}

impl Type {
    /// Whether `ty` holds a `KeyOf`/`OffsetAccess` anywhere.
    pub fn contains_late(&self) -> bool {
        matches!(self, Type(Repr::KeyOf(_)) | Type(Repr::OffsetAccess(..)))
            || self.any_child(Type::contains_late)
    }
}

/// A template's bound, recursively (`T of U`, `U of array`).
fn upper_bound(ty: &Type) -> Type {
    match ty {
        Type(Repr::TemplateParam(tp)) => upper_bound(&tp.bound),
        other => other.clone(),
    }
}

/// One late-resolvable node's answer, reading its operand through
/// `operand`; `None` when it isn't late-resolvable or can't be answered
/// yet.
fn resolve_node(ty: &Type, operand: &dyn Fn(&Type) -> Type) -> Option<Type> {
    match ty {
        Type(Repr::KeyOf(inner)) => key_of(&operand(inner)),
        Type(Repr::ValueOf(inner)) => value_of(&operand(inner)),
        Type(Repr::OffsetAccess(base, key)) => offset_access(&operand(base), key),
        _ => None,
    }
}

/// The union of `members`; `Never` for none.
fn join(members: Vec<Type>) -> Type {
    members
        .into_iter()
        .reduce(Type::union)
        .unwrap_or(Type::never())
}
