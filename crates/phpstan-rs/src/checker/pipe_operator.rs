//! Real PHPStan's `PipeOperatorRule` (`pipe.byRef`) - PHP 8.5's `|>` pipe
//! always passes its left-hand value *by value* into the right-hand
//! callable's first parameter, so a callable whose first parameter is
//! declared by-reference can never actually receive anything through a
//! pipe - a real, if narrow, correctness signal (not just style),
//! independent of the piped value's own type.
//!
//! The right-hand side is resolved through its type: a closure's or a
//! callable shape's signature, or the function or method a string, an
//! array callable or an invokable object names. A first-class callable
//! (`foo(...)`) is resolved from its syntax, its type not carrying the
//! signature.

use crate::checker::{Checker, Locals};
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;

impl<'a> Checker<'a> {
    /// `pipe.byRef` - see this module's own doc comment for scope.
    pub(crate) fn check_pipe_byref(&mut self, rhs: &Expr, locals: &Locals) {
        let Some(true) = self.pipe_target_first_param_is_by_ref(rhs, locals) else {
            return;
        };
        self.report_builder(DiagnosticBuilder::new(
            "Parameter #1 of callable on the right side of pipe operator is passed by reference."
                .to_string(),
            rhs.span,
            0,
            "pipe.byRef",
        ));
    }

    /// `None` when the right-hand side's own callable shape can't be
    /// resolved at all (silently skipped by the caller); `Some(bool)`
    /// once resolved, `true` meaning its first parameter (there must be
    /// at least one) is declared by-reference. A resolved callable with
    /// zero parameters is `Some(false)` - never flagged, matching real
    /// PHPStan's own `foreach` over zero parameters never entering its
    /// loop body at all.
    fn pipe_target_first_param_is_by_ref(&self, expr: &Expr, locals: &Locals) -> Option<bool> {
        if let ExprKind::FirstClassCallable(inner) = &expr.peeled().kind {
            return self.first_class_callable_first_param_by_ref(&inner.peeled().kind, locals);
        }
        let ty = self.infer_expr(expr, locals);
        if let Some(sig) = ty.callable_sig() {
            return Some(sig.params.first().is_some_and(|p| p.by_ref));
        }
        let sig = self.named_callable_signature(&ty)?;
        Some(sig.params().first().is_some_and(|p| p.by_ref()))
    }

    /// The referenced function/method's own first-parameter-by-ref
    /// answer for a `FirstClassCallable`'s inner `Call`/`MethodCall`/
    /// `StaticCall` - a read-only peek at its already-resolved
    /// signature, not a re-check (the pipe's own `check_expr` pass over
    /// `rhs` already ran every ordinary diagnostic this same expression
    /// gets on its own).
    fn first_class_callable_first_param_by_ref(
        &self,
        inner: &ExprKind,
        locals: &Locals,
    ) -> Option<bool> {
        match inner {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return None;
                };
                let (_, sig) = self.registry.resolve_function(name)?;
                Some(sig.params().first().is_some_and(|p| p.by_ref()))
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let (class, _) = recv_ty.as_object()?;
                let sig = self.registry.lookup_method(class, method)?;
                Some(sig.params().first().is_some_and(|p| p.by_ref()))
            }
            ExprKind::StaticCall(target, method, _) => {
                let class = self.infer_static_target_class(target, locals)?;
                let sig = self.registry.lookup_method(&class, method)?;
                Some(sig.params().first().is_some_and(|p| p.by_ref()))
            }
            _ => None,
        }
    }
}
