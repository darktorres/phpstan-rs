//! `parameterByRef.type` - a by-ref parameter directly assigned a value
//! its own declared type (native, or `@param-out` when present - see
//! `Checker::current_function_by_ref_params`'s own doc comment) can
//! never actually hold. Also `parameterByRef.executionEnd`/
//! `parameterByRef.tooWide` - see `check_by_ref_execution_end`/
//! `check_by_ref_too_wide` below.
//!
//! `parameterByRef.type` runs unconditionally for every by-ref
//! parameter, not gated on any opt-in flag, matching real PHPStan's own
//! `ParameterOutAssignedTypeRule` (tag-independent - falls back to the
//! native type with no `@param-out` tag at all, `TooWideParameterOutTypeCheck.php:87-92`'s
//! identical `getOutType() ?? getType()` fallback).

use crate::checker::Checker;
use syntax::ast::{Expr, ExprKind, Visibility};
use types::Type;
use types::union_types;

/// Same shape as the identically-named private helper in
/// `return_type_too_wide.rs`/`property_type_too_wide.rs`/`throws/
/// too_wide.rs` - lets a single non-`Union` actual type (e.g. only one
/// exit point was ever recorded) be compared as a one-member set
/// instead of forcing every caller to special-case it.
impl<'a> Checker<'a> {
    /// Mirrors `should_check_too_wide_method_return_type`'s gate exactly
    /// (`checker/return_type_too_wide.rs`) - private/final/first-
    /// declaration, else opt-in via `RulesSettings::check_too_wide_by_ref_type_in_protected_and_public_methods`.
    pub(crate) fn should_check_too_wide_by_ref_type(
        &self,
        class_name: &str,
        method_name: &str,
        visibility: Visibility,
    ) -> bool {
        if visibility == Visibility::Private {
            return true;
        }
        let Some(info) = self.registry.classes.get(class_name) else {
            return true;
        };
        if info.is_final() || info.final_methods().contains(method_name) {
            return true;
        }
        if !self
            .settings
            .rules
            .check_too_wide_by_ref_type_in_protected_and_public_methods
        {
            return false;
        }
        self.registry
            .find_method_signature_prototype(class_name, method_name)
            .is_some()
    }

    /// Called from `ExprKind::Assign`/`CompoundAssign`'s existing
    /// `Variable`-target handling (`checker/expr.rs`) - `actual` is the
    /// real value type just computed for this assignment (the RHS type
    /// for a plain `=`, the composed operator result for `+=`/`??=`/
    /// etc.). A no-op for any target that isn't a bare by-ref-parameter
    /// variable (destructuring/`foreach`-rebind targets are handled at
    /// their own separate call sites - see this module's sibling
    /// functions below).
    pub(crate) fn check_param_out_assignment(&mut self, lhs: &Expr, actual: &Type) {
        let ExprKind::Variable(name) = &lhs.kind else {
            return;
        };
        self.check_by_ref_value(name, actual, lhs.span);
    }

    /// The shared "does this by-ref parameter's own declared type accept
    /// `actual`" check, factored out so destructuring (`bind_assignment_
    /// target`'s own recursive targets) and a `foreach`-by-ref rebind can
    /// reuse it without going through a real `ExprKind::Assign` node at
    /// all. Doesn't itself update `current_function_by_ref_exit_snapshots` -
    /// that's only ever taken from `locals` at a real execution-end
    /// point (`record_by_ref_exit_snapshot` below), not at every
    /// intermediate assignment.
    pub(crate) fn check_by_ref_value(
        &mut self,
        name: &str,
        actual: &Type,
        span: syntax::token::Span,
    ) {
        let Some(declared) = self.current_function_by_ref_params.get(name).cloned() else {
            return;
        };
        // See `checker::calls`' own identical comment: `accepts_at_
        // configured_level` already accepts a `Mixed` actual value
        // unconditionally below real PHPStan's own level 9/10.
        if self.rule_level_accepts(&declared, actual).yes() {
            return;
        }
        self.report(
            format!("by-ref parameter ${name} expects {declared}, {actual} given"),
            span,
            3,
            "parameterByRef.type",
        );
    }

    /// Snapshots every currently-in-scope by-ref parameter (plus every
    /// closure `use (&$x)` capture - see `current_function_byref_
    /// snapshot_names`'s own doc comment) own `locals`-tracked type into
    /// `current_function_by_ref_exit_snapshots` - called at each real
    /// execution-end point (`StmtKind::Return`, both arms, and the
    /// falls-off-the-end path in `check_function_impl`). A parameter
    /// missing from `locals` entirely (shouldn't happen - every
    /// parameter is declared into `locals` up front - but this project's
    /// own "don't guess" precedent prefers a `Mixed` fallback over a
    /// panic here) falls back to `Type::Mixed(false)`, matching
    /// `check_by_ref_value`'s own "never flag a `Mixed` value" leniency.
    pub(crate) fn record_by_ref_exit_snapshot(&mut self, locals: &crate::checker::Locals) {
        if self.current_function_by_ref_params.is_empty()
            && self.current_function_byref_snapshot_names.is_empty()
        {
            return;
        }
        let names: Vec<std::sync::Arc<str>> = self
            .current_function_by_ref_params
            .keys()
            .cloned()
            .chain(self.current_function_byref_snapshot_names.iter().cloned())
            .collect();
        for name in names {
            let ty = locals.get(&name).cloned().unwrap_or(Type::mixed());
            self.current_function_by_ref_exit_snapshots
                .entry(name)
                .or_default()
                .push(ty);
        }
    }

    /// Real PHPStan's `ParameterOutExecutionEndTypeRule` (`parameterByRef.
    /// executionEnd`) - unlike `parameterByRef.type` above, this only
    /// ever fires for a by-ref parameter that actually declares a real
    /// `@param-out` tag (real PHPStan bails via `getOutType() === null`
    /// otherwise; only the tag-independent direct-assignment rule falls
    /// back to the native type). Checks the *union* of every real exit
    /// point's own snapshot, not each individually - a parameter that's
    /// `int` on one path and `string` on another is fine as long as
    /// `@param-out int|string` accepts the union; only a genuinely
    /// unacceptable exit state should ever be reported, not "some path
    /// disagreed with another."
    pub(crate) fn check_by_ref_execution_end(&mut self, span: syntax::token::Span) {
        let out_types: Vec<(std::sync::Arc<str>, Type)> = self
            .current_function_by_ref_params_with_tag
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();
        for (name, declared) in out_types {
            let Some(snapshots) = self.current_function_by_ref_exit_snapshots.get(&name) else {
                continue;
            };
            let Some(actual) = snapshots.iter().cloned().reduce(union_types) else {
                continue;
            };
            if self.rule_level_accepts(&declared, &actual).yes() {
                continue;
            }
            self.report(format!(
                    "by-ref parameter ${name} expects {declared} at the end of execution, {actual} given"
                ),
                span,
                3,
                "parameterByRef.executionEnd",
            );
        }
    }

    /// Real PHPStan's `TooWideParameterOutTypeCheck`/`TooWideFunctionParameterOutTypeRule`/
    /// `TooWideMethodParameterOutTypeRule` (`parameterByRef.tooWide`) -
    /// the by-ref-parameter analogue of `check_too_wide_return_type`,
    /// same shape (only acts on a declared `Union`, needs at least one
    /// real snapshot recorded). Gated by the caller on `should_check_
    /// too_wide_by_ref_type` (mirroring `should_check_too_wide_method_
    /// return_type`'s own visibility/finality gate) - callers must check
    /// that before calling this.
    pub(crate) fn check_too_wide_by_ref_type(&mut self, span: syntax::token::Span) {
        let params: Vec<(std::sync::Arc<str>, Type)> = self
            .current_function_by_ref_params
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();
        for (name, declared) in params {
            let declared_members = declared.union_members();
            if declared_members.len() < 2 {
                continue;
            }
            let Some(snapshots) = self.current_function_by_ref_exit_snapshots.get(&name) else {
                continue;
            };
            let Some(actual) = snapshots.iter().cloned().reduce(union_types) else {
                continue;
            };
            if actual.is_mixed() {
                continue;
            }
            let actual_members = actual.union_members();
            let too_wide: Vec<String> = declared_members
                .iter()
                .filter(|m| {
                    !actual_members
                        .iter()
                        .any(|a| a.is_mixed() || self.accepts(m, a).yes())
                })
                .map(|m| m.to_string())
                .collect();
            if too_wide.is_empty() {
                continue;
            }
            self.report(format!(
                    "by-ref parameter ${name}'s declared type {declared} is never narrower than {actual} at the end of execution - consider narrowing it"
                ),
                span,
                5,
                "parameterByRef.tooWide",
            );
        }
    }
}
