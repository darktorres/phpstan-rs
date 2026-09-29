//! Real PHPStan's `PipeOperatorRule` (`pipe.byRef`) - PHP 8.5's `|>` pipe
//! always passes its left-hand value *by value* into the right-hand
//! callable's first parameter, so a callable whose first parameter is
//! declared by-reference can never actually receive anything through a
//! pipe - a real, if narrow, correctness signal (not just style),
//! independent of the piped value's own type.
//!
//! The right-hand side is resolved through its type: a closure's, a
//! first-class callable's or a callable shape's signature, or the
//! function or method a string, an array callable or an invokable object
//! names.

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
        let ty = self.infer_expr(expr, locals);
        if let Some(sig) = ty.callable_sig() {
            return Some(sig.params.first().is_some_and(|p| p.by_ref));
        }
        let sig = self.named_callable_signature(&ty)?;
        Some(sig.params().first().is_some_and(|p| p.by_ref()))
    }
}
