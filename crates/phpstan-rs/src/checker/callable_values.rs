//! Invoking a value that names a function or method rather than carrying
//! a callable signature of its own - `'strlen'`, `'Foo::bar'`,
//! `[$obj, 'method']`, `['Foo', 'bar']`, or an object with `__invoke()` -
//! mirroring real PHPStan's `CallCallablesRule`: the resolved signature's
//! arity and argument types, and `callable.inaccessibleMethod` for a
//! method the calling scope can't see.

use crate::checker::calls::CheckArgsCall;
use crate::checker::{Checker, Locals};
use crate::registry::FunctionReflection;
use syntax::ast::{Arg, Visibility};
use syntax::token::Span;
use types::{ShapeKey, Type};

/// The function or method a callable value names.
struct NamedCallable<'r> {
    description: String,
    sig: &'r FunctionReflection,
    /// `(owner, method)` for a method, `None` for a plain function.
    method: Option<(String, String)>,
    defining_class: Option<String>,
}

impl<'a> Checker<'a> {
    fn resolve_named_callable(&self, ty: &Type) -> Option<NamedCallable<'a>> {
        match ty {
            Type::StringLiteral(s) => {
                let s = s.as_str()?;
                if let Some((class, method)) = s.split_once("::") {
                    return self.resolve_callable_method(class, method, format!("callable '{s}'"));
                }
                let (_, sig) = self
                    .registry
                    .lookup_function_with_name(s.trim_start_matches('\\'))?;
                Some(NamedCallable {
                    description: format!("callable '{s}'"),
                    sig,
                    method: None,
                    defining_class: None,
                })
            }
            Type::ArrayShape(shape) => {
                let [first, second] = shape.fields.as_slice() else {
                    return None;
                };
                let (Type::StringLiteral(method), true) = (&second.ty, shape.is_sealed()) else {
                    return None;
                };
                let method = method.as_str()?;
                if first.key != ShapeKey::Int(0)
                    || second.key != ShapeKey::Int(1)
                    || first.optional
                    || second.optional
                {
                    return None;
                }
                let target = &first.ty;
                let class = match target {
                    Type::Object(class, _) => class.to_string(),
                    Type::ClassString(inner) => match inner.as_ref() {
                        Type::Object(class, _) => class.to_string(),
                        _ => return None,
                    },
                    Type::StringLiteral(class) => class.as_str()?.to_string(),
                    _ => return None,
                };
                self.resolve_callable_method(&class, method, format!("callable {ty}"))
            }
            Type::Object(class, _) => {
                let (owner, sig) = self.registry.lookup_method_with_owner(class, "__invoke")?;
                Some(NamedCallable {
                    description: format!("callable {ty}"),
                    sig,
                    method: Some((owner.to_string(), "__invoke".to_string())),
                    defining_class: Some(class.clone().to_string()),
                })
            }
            _ => None,
        }
    }

    fn resolve_callable_method(
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
        let (owner, sig) = self.registry.lookup_method_with_owner(canonical, method)?;
        Some(NamedCallable {
            description,
            sig,
            method: Some((owner.to_string(), method.to_string())),
            defining_class: Some(canonical.to_string()),
        })
    }

    /// Checks invoking `callee_ty` with `args` when it names a function
    /// or method.
    pub(crate) fn check_named_callable_invocation(
        &mut self,
        callee_ty: &Type,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        let Some(named) = self.resolve_named_callable(callee_ty) else {
            return;
        };
        if let Some((owner, method)) = &named.method
            && named.sig.visibility() != Visibility::Public
            && !self.is_member_access_visible(owner, &named.sig.visibility(), method, "method")
        {
            let word = if named.sig.visibility() == Visibility::Private {
                "private"
            } else {
                "protected"
            };
            self.report(
                format!("Call to {word} method {method}() of class {owner}."),
                span,
                2,
                "callable.inaccessibleMethod",
            );
        }
        self.check_args(CheckArgsCall {
            callee_label: &named.description,
            callee: crate::checker::calls::parameters::Callee::Callable(&named.description),
            sig: named.sig,
            args,
            locals,
            defining_class: named.defining_class.as_deref(),
            is_builtin: false,
            span,
        });
    }
}
