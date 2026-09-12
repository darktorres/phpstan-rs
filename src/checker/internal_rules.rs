//! Phase 18 Axis 3 Stage 1: an internal `Vec<Box<dyn InternalRule>>` +
//! declaration-level dispatch mechanism, proving the trait-object
//! dispatch pattern this axis's own eventual public `Rule`/`Scope`
//! surface (Stage 2, gated on this stage's own clean corpus diff) would
//! need, for the one already-shipped check that fits its shape cleanly:
//! `MethodThrowTypeCovarianceRule` (`checker/oop/method_overrides.rs`) -
//! a pure declaration-vs-declaration comparison needing only already-
//! populated `Registry` data, with no flow-sensitive dependency at all.
//!
//! **Representativeness caveat, disclosed rather than silently assumed
//! away**: this stage does NOT also migrate `check_missing_checked_
//! exceptions` (`checker/throws/missing.rs`) onto the same trait, even
//! though an earlier pass of this plan expected to. That check needs
//! flow-collected state (`Checker::current_function_explicit_throw_
//! points`, populated only once a function's whole body has been
//! walked) and a resolved `@throws` tag - a genuinely different, richer
//! shape than the plain `(fqn, method_name, visibility, span)` this
//! trait's own single real implementor needs, and forcing it into the
//! identical signature here would mean passing it meaningless
//! placeholder fields at one of its own two real call sites (a
//! textually-declared method) and duplicating logic at the other (a
//! trait-supplied-but-undeclared method, which never reaches `check_
//! function` at all - see `check_method_overrides`'s own doc comment).
//! This stage proves the dispatch pattern works for a real, already-
//! shipped declaration-level check; it does not, and doesn't claim to,
//! validate the harder unresolved parts of Stage 2 (a public `Scope`
//! facade over flow-sensitive per-expression state, or injecting
//! dispatch into arbitrary statement/expression node kinds mid-
//! traversal) - scope Stage 2 accordingly rather than assuming this
//! stage's own clean diff already de-risked it.

use crate::ast::Visibility;
use crate::checker::Checker;
use crate::token::Span;

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
