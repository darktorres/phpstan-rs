//! Invoking a value that names a function or method rather than carrying
//! a callable signature of its own - `'strlen'`, `'Foo::bar'`,
//! `[$obj, 'method']`, `['Foo', 'bar']`, or an object with `__invoke()` -
//! mirroring real PHPStan's `CallCallablesRule`: the resolved signature's
//! arity and argument types, and `callable.inaccessibleMethod` for a
//! method the calling scope can't see.

use crate::checker::calls::CheckArgsCall;
use crate::checker::{CalleeLabel, Checker, Locals};
use syntax::ast::{Arg, Visibility};
use syntax::span::Span;
use types::Type;

impl<'a> Checker<'a> {
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
            report!(
                self,
                format!("Call to {word} method {method}() of class {owner}."),
                span,
                2,
                "callable.inaccessibleMethod",
            );
        }
        self.check_args(CheckArgsCall {
            callee_label: CalleeLabel::Text(&named.description),
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
