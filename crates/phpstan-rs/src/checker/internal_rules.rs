//! An internal `Vec<Box<dyn InternalRule>>` + declaration-level dispatch
//! mechanism, for declaration-vs-declaration checks needing only
//! already-populated `Registry` data with no flow-sensitive dependency:
//! `MethodThrowTypeCovarianceRule` (`checker/oop/method_overrides.rs`)
//! is the one implementor today.
//!
//! `check_missing_checked_exceptions` (`checker/throws/missing.rs`)
//! deliberately does NOT use this trait: it needs flow-collected state
//! (`Checker::current_function_explicit_throw_points`, populated only
//! once a function's whole body has been walked) and a resolved
//! `@throws` tag - a genuinely different, richer shape than the plain
//! `(fqn, method_name, visibility, span)` this trait's implementor
//! needs, and forcing it into the identical signature here would mean
//! passing it meaningless placeholder fields at one of its two real
//! call sites (a textually-declared method) and duplicating logic at
//! the other (a trait-supplied-but-undeclared method, which never
//! reaches `check_function` at all - see `check_method_overrides`'s own
//! doc comment).

use crate::checker::Checker;
use syntax::ast::Visibility;
use syntax::token::Span;

pub(crate) trait InternalRule {
    fn check_method(
        &self,
        checker: &mut Checker,
        fqn: &str,
        method_name: &str,
        visibility: &Visibility,
        span: Span,
    );
}

struct MethodThrowTypeCovarianceRule;

impl InternalRule for MethodThrowTypeCovarianceRule {
    fn check_method(
        &self,
        checker: &mut Checker,
        fqn: &str,
        method_name: &str,
        visibility: &Visibility,
        span: Span,
    ) {
        checker.check_method_throw_type_covariance(fqn, method_name, visibility, span);
    }
}

/// Every declaration-level (`(fqn, method_name, visibility, span)`-
/// shaped) internal rule, in the order they run - adding a second real
/// implementor of this exact shape is now a one-line addition here, not
/// a new hardcoded call site at `check_one_method_override`'s own call
/// site.
pub(crate) fn declaration_level_rules() -> Vec<Box<dyn InternalRule>> {
    vec![Box::new(MethodThrowTypeCovarianceRule)]
}
