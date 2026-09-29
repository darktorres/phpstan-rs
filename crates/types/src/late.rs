//! Late-resolvable types: `key-of<T>`, `value-of<T>` and `T[K]`, whose
//! answer depends on a `T` that may be a template until a call binds it
//! (PHPStan's `LateResolvableType`).

use crate::{ConditionalSubject, Repr, ShapeKey, Type};

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
        syntax::stack::guard(|| match self.repr() {
            // An enum's case values need `consts`; any other target settles
            // like `key-of` does, a template staying until a call binds it.
            Repr::ValueOf(target) => {
                let target = target.resolve_class_references(consts);
                match target.repr() {
                    Repr::Object(class, _) => consts.enum_values(class),
                    _ => settle(Type::value_of(target)),
                }
            }
            Repr::ClassConstant { class, pattern } => consts.class_constants(class, pattern),
            Repr::KeyOf(_) | Repr::OffsetAccess(..) => {
                settle(self.map_children(&mut |t| t.resolve_class_references(consts)))
            }
            _ => self.map_children(&mut |t| t.resolve_class_references(consts)),
        })
    }

    /// This type with each class constant reference replaced by `f`'s
    /// answer for its class and name pattern, kept where `f` gives none.
    pub fn map_class_constants(&self, f: &mut dyn FnMut(&Type, &str) -> Option<Type>) -> Type {
        syntax::stack::guard(|| match self.repr() {
            Repr::ClassConstant { class, pattern } => {
                f(class, pattern).unwrap_or_else(|| self.clone())
            }
            _ => self.map_children(&mut |t| t.map_class_constants(&mut *f)),
        })
    }
}

/// `key-of<ty>` when `ty` is an array type: its key type. `None` for
/// anything whose keys aren't known from the type alone.
pub(crate) fn key_of(ty: &Type) -> Option<Type> {
    match ty.repr() {
        Repr::Mixed(..) => Some(Type::mixed()),
        Repr::Array(key, _) => Some((**key).clone()),
        Repr::ArrayShape(shape) => Some(join(
            shape
                .fields
                .iter()
                .map(|f| f.key.to_type())
                .chain(shape.unsealed.as_deref().map(|tail| tail.0.clone()))
                .collect(),
        )),
        Repr::Intersection(members) => members.iter().find_map(key_of),
        Repr::Union(members) => members.iter().map(key_of).collect::<Option<_>>().map(join),
        _ => None,
    }
}

/// `value-of<ty>` when `ty` is an array type: its value type. An enum's
/// case values need the registry (`Registry::resolve_deferred_value_of`).
pub(crate) fn value_of(ty: &Type) -> Option<Type> {
    match ty.repr() {
        Repr::Mixed(..) => Some(Type::mixed()),
        Repr::Array(_, value) => Some((**value).clone()),
        Repr::ArrayShape(shape) => Some(join(shape.types().cloned().collect())),
        Repr::Intersection(members) => members.iter().find_map(value_of),
        Repr::Union(members) => members
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
    syntax::stack::guard(|| match base.repr() {
        // A key the shape doesn't list is its unsealed tail's.
        Repr::ArrayShape(shape) | Repr::ObjectShape(shape) => Some(
            ShapeKey::from_type(key)
                .and_then(|key| shape.field(&key))
                .map(|field| field.ty.clone())
                .or_else(|| shape.unsealed.as_deref().map(|tail| tail.1.clone()))
                .unwrap_or(Type::mixed()),
        ),
        Repr::Array(_, value) => Some((**value).clone()),
        Repr::Intersection(members) => members.iter().find_map(|m| offset_access(m, key)),
        Repr::Union(members) => members
            .iter()
            .map(|m| offset_access(m, key))
            .collect::<Option<_>>()
            .map(join),
        Repr::TemplateParam(_) | Repr::Object(_, _) => None,
        _ => Some(Type::mixed()),
    })
}

/// A late-resolvable node answered now that its operand may be known -
/// what a substitution applies to each node it rebuilds. A `key-of`/offset
/// access whose operand no longer holds a template is answered, `Mixed`
/// when the operand isn't an array type; a `value-of` is answered only
/// over an array type (an enum's needs the registry).
pub(crate) fn settle(ty: Type) -> Type {
    match ty.repr() {
        Repr::KeyOf(inner) | Repr::OffsetAccess(inner, _) if !inner.references_template() => {
            resolve_node(&ty, &|t| t.clone()).unwrap_or(Type::mixed())
        }
        Repr::ValueOf(_) => resolve_node(&ty, &|t| t.clone()).unwrap_or(ty),
        _ => ty,
    }
}

impl Type {
    /// Whether this type names a template anywhere: a template parameter, or a
    /// conditional whose subject is one.
    pub fn references_template(&self) -> bool {
        match self.repr() {
            Repr::TemplateParam(_) => true,
            Repr::Conditional {
                subject: ConditionalSubject::Template(_),
                ..
            } => true,
            _ => self.any_child(Type::references_template),
        }
    }
}

impl Type {
    /// This type with every late-resolvable node answered as far as its
    /// templates' bounds allow - what a template-typed value can be relied
    /// on to be inside the generic code itself. An answer the bound doesn't
    /// give is `Mixed`.
    pub fn resolve_late_to_bounds(&self) -> Type {
        let ty = self.map_children(&mut Type::resolve_late_to_bounds);
        match ty.repr() {
            Repr::KeyOf(_) | Repr::OffsetAccess(..) => {
                resolve_node(&ty, &upper_bound).unwrap_or(Type::mixed())
            }
            Repr::ValueOf(inner) if matches!(inner.repr(), Repr::TemplateParam(_)) => {
                resolve_node(&ty, &upper_bound).unwrap_or(Type::mixed())
            }
            _ => ty,
        }
    }
}

impl Type {
    /// Whether this type holds a `key-of` or offset access anywhere.
    pub fn contains_late(&self) -> bool {
        matches!(self.repr(), Repr::KeyOf(_) | Repr::OffsetAccess(..))
            || self.any_child(Type::contains_late)
    }
}

/// A template's bound, recursively (`T of U`, `U of array`).
pub(crate) fn upper_bound(ty: &Type) -> Type {
    syntax::stack::guard(|| match ty.repr() {
        Repr::TemplateParam(tp) => upper_bound(&tp.bound),
        _ => ty.clone(),
    })
}

/// One late-resolvable node's answer, reading its operand through
/// `operand`; `None` when it isn't late-resolvable or can't be answered
/// yet.
fn resolve_node(ty: &Type, operand: &dyn Fn(&Type) -> Type) -> Option<Type> {
    match ty.repr() {
        Repr::KeyOf(inner) => key_of(&operand(inner)),
        Repr::ValueOf(inner) => value_of(&operand(inner)),
        Repr::OffsetAccess(base, key) => offset_access(&operand(base), key),
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
