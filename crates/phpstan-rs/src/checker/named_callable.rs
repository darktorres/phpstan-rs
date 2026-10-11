//! The function or method a callable value names: a function name, a
//! `'Class::method'` string, an array callable, or an object's `__invoke()`.
//! Shared by the callable invocation rule (`callable_values`) and the
//! analyser's own needs, so the resolution lives outside the rules.

use crate::checker::Checker;
use crate::registry::FunctionReflection;
use types::{ShapeKey, Type};

pub(crate) struct NamedCallable<'r> {
    pub(crate) description: String,
    pub(crate) sig: &'r FunctionReflection,
    /// `(owner, method)` for a method, `None` for a plain function.
    pub(crate) method: Option<(String, String)>,
    pub(crate) defining_class: Option<String>,
}

impl<'a> Checker<'a> {
    /// The signature of the function or method a value of type `ty`
    /// names: a function name, `'Class::method'`, an array callable, or
    /// an object's `__invoke()`.
    pub(crate) fn named_callable_signature(&self, ty: &Type) -> Option<&'a FunctionReflection> {
        self.resolve_named_callable(ty).map(|named| named.sig)
    }

    pub(crate) fn resolve_named_callable(&self, ty: &Type) -> Option<NamedCallable<'a>> {
        if let Some(s) = ty.string_literal_value() {
            let s = s.as_str()?;
            if let Some((class, method)) = s.split_once("::") {
                return self.resolve_callable_method(class, method, format!("callable '{s}'"));
            }
            let (_, sig) = self
                .registry
                .lookup_function_with_name(s.trim_start_matches('\\'))?;
            return Some(NamedCallable {
                description: format!("callable '{s}'"),
                sig,
                method: None,
                defining_class: None,
            });
        }
        if let [shape] = ty.constant_arrays()[..] {
            let [first, second] = shape.fields() else {
                return None;
            };
            let method = second.ty.string_literal_value()?.as_str()?;
            if first.key != ShapeKey::Int(0)
                || second.key != ShapeKey::Int(1)
                || first.optional
                || second.optional
            {
                return None;
            }
            let target = &first.ty;
            let class = if let Some((class, _)) = target.as_object() {
                class.to_string()
            } else if let Some(class) = target.class_string_class() {
                class.to_string()
            } else {
                target.string_literal_value()?.as_str()?.to_string()
            };
            return self.resolve_callable_method(&class, method, format!("callable {ty}"));
        }
        let (class, _) = ty.as_object()?;
        let (owner, sig) = self.lookup_method_with_owner(class, "__invoke")?;
        Some(NamedCallable {
            description: format!("callable {ty}"),
            sig,
            method: Some((owner.to_string(), "__invoke".to_string())),
            defining_class: Some(class.to_string()),
        })
    }

    pub(crate) fn resolve_callable_method(
        &self,
        class: &str,
        method: &str,
        description: String,
    ) -> Option<NamedCallable<'a>> {
        if method.contains("::") {
            return None;
        }
        let class = class.trim_start_matches('\\');
        let (canonical, _) = self.registry.lookup_class(class)?;
        let (owner, sig) = self.lookup_method_with_owner(canonical, method)?;
        Some(NamedCallable {
            description,
            sig,
            method: Some((owner.to_string(), method.to_string())),
            defining_class: Some(canonical.to_string()),
        })
    }
}
