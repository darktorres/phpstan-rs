//! `parameterByRef.type` - a by-ref parameter directly assigned a value
//! its own declared type (native, or `@param-out` when present - see
//! `Checker::current_function_by_ref_params`'s own doc comment) can
//! never actually hold.
//!
//! Runs unconditionally for every by-ref parameter, not gated on any
//! opt-in flag, matching real PHPStan's own `ParameterOutAssignedTypeRule`
//! (tag-independent - falls back to the native type with no
//! `@param-out` tag at all, `TooWideParameterOutTypeCheck.php:87-92`'s
//! identical `getOutType() ?? getType()` fallback).

use crate::ast::{Expr, ExprKind};
use crate::checker::Checker;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Called from `ExprKind::Assign`/`CompoundAssign`'s existing
    /// `Variable`-target handling (`checker/expr.rs`) - `actual` is the
    /// real value type just computed for this assignment (the RHS type
    /// for a plain `=`, the composed operator result for `+=`/`??=`/
    /// etc.). A no-op for any target that isn't a bare by-ref-parameter
    /// variable (destructuring/`foreach`-rebind targets are handled at
    /// their own separate call sites - see this module's sibling
    /// functions below).
    pub(crate) fn check_param_out_assignment(&mut self, lhs: &Expr, actual: &Type, label: &str) {
        let ExprKind::Variable(name) = &lhs.kind else {
            return;
        };
        self.check_by_ref_value(name, actual, lhs.span, label);
    }

    /// The shared "does this by-ref parameter's own declared type accept
    /// `actual`" check + accumulator update, factored out so destructuring
    /// (`bind_assignment_target`'s own recursive targets) and a
    /// `foreach`-by-ref rebind can reuse it without going through a real
    /// `ExprKind::Assign` node at all.
    pub(crate) fn check_by_ref_value(
        &mut self,
        name: &str,
        actual: &Type,
        span: crate::token::Span,
        label: &str,
    ) {
        let Some(declared) = self.current_function_by_ref_params.get(name).cloned() else {
            return;
        };
        self.current_function_by_ref_assigned_types
            .entry(std::sync::Arc::from(name))
            .or_default()
            .push(actual.clone());
        if *actual == Type::Mixed || self.registry.accepts_at_configured_level(actual, &declared) {
            return;
        }
        self.report(
            label.to_string(),
            format!("by-ref parameter ${name} expects {declared}, {actual} given"),
            span,
            3,
            "parameterByRef.type",
        );
    }
}
