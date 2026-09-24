//! Real PHPStan's `PipeOperatorRule` (`pipe.byRef`) - PHP 8.5's `|>` pipe
//! always passes its left-hand value *by value* into the right-hand
//! callable's first parameter, so a callable whose first parameter is
//! declared by-reference can never actually receive anything through a
//! pipe - a real, if narrow, correctness signal (not just style),
//! independent of the piped value's own type.
//!
//! Real PHPStan resolves this against the right-hand side's own
//! *type* (`Type::getCallableParametersAcceptors`), which only ever
//! answers something concrete for a PHPDoc-shaped `callable(...)`/
//! `Closure(...)` type. This project doesn't synthesize that shape for
//! an ordinary closure/arrow-fn *literal* expression's own inferred type
//! (a bare literal infers as the coarse `Type::Object("Closure", [])| -
//! see `ExprKind::Closure`'s own `check_expr` arm) - resolving through
//! the type system here would need building that machinery first, a
//! large, disproportionate undertaking for this one narrow check.
//! Instead, this resolves directly against the right-hand side's own
//! *literal syntax* - the same "handle the shape that actually comes
//! up" precedent `check_closure_literal_with_param_overrides` and
//! `FirstClassCallable`'s own resolution already established - covering
//! every real pipe-target idiom: a closure/arrow-fn literal, a
//! first-class callable (`foo(...)`/`$obj->method(...)`/
//! `Foo::method(...)`), a bare string function name, and a two-element
//! `[$obj, 'method']`/`['Class', 'method']` array callable. Anything
//! else (a plain variable holding an already-built `Closure`, for
//! instance) can't be resolved this way and is silently skipped -
//! matching real PHPStan's own silent skip whenever its type-level
//! resolution can't produce a confident `isCallable()` type either.

use crate::ast::*;
use crate::checker::{Checker, DiagnosticBuilder, Locals, unwrap_grouping};
use crate::types::Type;

impl<'a> Checker<'a> {
    /// `pipe.byRef` - see this module's own doc comment for scope.
    pub(crate) fn check_pipe_byref(&mut self, rhs: &Expr, locals: &Locals, label: &str) {
        let Some(true) = self.pipe_target_first_param_is_by_ref(rhs, locals) else {
            return;
        };
        self.report_builder(DiagnosticBuilder::new(
            label.to_string(),
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
        match &unwrap_grouping(expr).kind {
            ExprKind::Closure { params, .. } | ExprKind::ArrowFn { params, .. } => {
                Some(params.first().is_some_and(|p| p.by_ref))
            }
            ExprKind::FirstClassCallable(inner) => {
                self.first_class_callable_first_param_by_ref(&unwrap_grouping(inner).kind, locals)
            }
            ExprKind::Literal(Literal::String(name)) => self
                .registry
                .lookup_function(name)
                .map(|sig| sig.params.first().is_some_and(|p| p.by_ref)),
            // `[$obj, 'method']` / `['Class', 'method']` array callable -
            // a plain two-element list, no string key on either item.
            ExprKind::ArrayLiteral(items) => match items.as_slice() {
                [recv_item, method_item]
                    if recv_item.key.is_none() && method_item.key.is_none() =>
                {
                    let ExprKind::Literal(Literal::String(method)) =
                        &unwrap_grouping(&method_item.value).kind
                    else {
                        return None;
                    };
                    let class = match &unwrap_grouping(&recv_item.value).kind {
                        ExprKind::Literal(Literal::String(name)) => name.to_string(),
                        _ => match self.infer_expr(&recv_item.value, locals) {
                            Type::Object(class, _) => class,
                            _ => return None,
                        },
                    };
                    self.registry
                        .lookup_method(&class, method)
                        .map(|sig| sig.params.first().is_some_and(|p| p.by_ref))
                }
                _ => None,
            },
            _ => None,
        }
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
                let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                let sig = self.registry.lookup_function(&primary).or_else(|| {
                    fallback
                        .as_deref()
                        .and_then(|g| self.registry.lookup_function(g))
                })?;
                Some(sig.params.first().is_some_and(|p| p.by_ref))
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                let class = match self.infer_expr(recv, locals) {
                    Type::Object(class, _) => class,
                    _ => return None,
                };
                let sig = self.registry.lookup_method(&class, method)?;
                Some(sig.params.first().is_some_and(|p| p.by_ref))
            }
            ExprKind::StaticCall(target, method, _) => {
                let class = self.infer_static_target_class(target, locals)?;
                let sig = self.registry.lookup_method(&class, method)?;
                Some(sig.params.first().is_some_and(|p| p.by_ref))
            }
            _ => None,
        }
    }
}
