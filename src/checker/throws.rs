//! Phase 42: the checked-exception/throw-type model.
//!
//! - `InvalidThrowsPhpDocValueRule` (`throws.notThrowable`, a `@throws`
//!   tag naming something that isn't actually a `Throwable` subtype) and
//!   `ThrowsVoidFunctionWithExplicitThrowPointRule`/
//!   `ThrowsVoidMethodWithExplicitThrowPointRule` (`throws.void`, an
//!   explicit `@throws void` directly contradicted by a real `throw`
//!   visible in the body) - this module's own original, self-contained
//!   slice, needing no per-callee throw-type model at all.
//! - Sub-item A (transitive throw-point collection): `Checker::current_
//!   function_throw_points`/`current_function_has_unannotated_call`
//!   (`checker.rs`'s own doc comments on each), recorded at every real
//!   `throw` and every already-resolved-signature call choke point
//!   (`checker/expr.rs`'s `ExprKind::Call`/`New`/`MethodCall`/
//!   `StaticCall` arms, `checker/stmt/try_catch.rs`'s `check_throw`).
//! - Sub-item D (per-callee dynamic throw-type extensions): `dynamic`
//!   submodule, tried before each hook's plain `FunctionSig.throws`
//!   fallback.
//! - Sub-item C (`TooWideFunctionThrowTypeRule`/`TooWideMethodThrowTypeRule`):
//!   `too_wide` submodule.
//! - Bonus item (opt-in "missing checked exception" family, gated on
//!   `Registry::check_missing_checked_exceptions`): `missing` submodule.
//!
//! Declined, unchanged since the original decision: `MethodThrowTypeCovarianceRule`
//! (Sub-item B - no call-graph-wide implicit-throw model exists to build
//! it on).

use crate::ast::{Expr, ExprKind, Stmt, StmtKind};
use crate::ast_walk::{expr_children, stmt_children};
use crate::checker::Checker;
use crate::registry::{FunctionSig, Registry};
use crate::token::Span;
use crate::types::Type;

mod dynamic;
mod missing;
mod too_wide;

/// Whether `stmts` contains a real, direct `throw` (statement or
/// expression) anywhere in its own body - `Some(span)` of the first one
/// found, `None` if there isn't one. Mirrors `checker::control_flow`'s
/// own `body_contains_yield`/`stmt_contains_yield`/`expr_contains_yield`
/// trio exactly, including their identical "stops at a nested
/// `Closure`/`ArrowFn`/`NewAnonymousClass`/`FunctionDecl`/`ClassDecl`/
/// `EnumDecl` boundary" semantics (`expr_children`/`stmt_children`'s own
/// doc comments) - a `throw` inside a nested closure belongs to *that*
/// closure's own reflection, not this outer function/method.
pub(crate) fn body_contains_explicit_throw(stmts: &[Stmt]) -> Option<Span> {
    stmts.iter().find_map(stmt_contains_explicit_throw)
}

fn stmt_contains_explicit_throw(stmt: &Stmt) -> Option<Span> {
    if let StmtKind::Throw(_) = &stmt.kind {
        return Some(stmt.span);
    }
    let own = match &stmt.kind {
        StmtKind::Expression(e) => expr_contains_explicit_throw(e),
        StmtKind::Return(Some(e)) => expr_contains_explicit_throw(e),
        StmtKind::Return(None) => None,
        StmtKind::Echo(exprs) => exprs.iter().find_map(expr_contains_explicit_throw),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => expr_contains_explicit_throw(cond),
        StmtKind::For {
            init, cond, step, ..
        } => init
            .iter()
            .chain(cond)
            .chain(step)
            .find_map(expr_contains_explicit_throw),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => expr_contains_explicit_throw(subject)
            .or_else(|| key.as_ref().and_then(expr_contains_explicit_throw))
            .or_else(|| expr_contains_explicit_throw(value)),
        StmtKind::GlobalConst(consts) => consts
            .iter()
            .find_map(|c| expr_contains_explicit_throw(&c.value)),
        StmtKind::StaticVar(vars) => vars
            .iter()
            .find_map(|(_, d)| d.as_ref().and_then(expr_contains_explicit_throw)),
        StmtKind::Switch { subject, cases } => {
            expr_contains_explicit_throw(subject).or_else(|| {
                cases
                    .iter()
                    .find_map(|c| c.test.as_ref().and_then(expr_contains_explicit_throw))
            })
        }
        _ => None,
    };
    own.or_else(|| {
        stmt_children(stmt)
            .into_iter()
            .find_map(stmt_contains_explicit_throw)
    })
}

fn expr_contains_explicit_throw(e: &Expr) -> Option<Span> {
    if let ExprKind::Throw(_) = &e.kind {
        return Some(e.span);
    }
    expr_children(e)
        .into_iter()
        .find_map(expr_contains_explicit_throw)
}

impl<'a> Checker<'a> {
    /// Phase 42 Sub-item A: records one call's own contribution to
    /// `current_function_throw_points`/`current_function_has_unannotated_
    /// call` - shared by every call-shape choke point (`checker/expr.rs`'s
    /// `ExprKind::Call`/`New`/`MethodCall`/`StaticCall` arms). `dynamic`
    /// is Sub-item D's own per-callee override, already computed by the
    /// caller (tried first); `sig` is the plainly-resolved callee
    /// signature, whose own `throws` field is the fallback once `dynamic`
    /// is `None`.
    ///
    /// `sig` being `None` (no signature at all - a dynamic call, or a
    /// genuinely unresolvable one) is the one case that marks the whole
    /// enclosing declaration `current_function_has_unannotated_call`:
    /// real PHP lets an arbitrarily-implemented callee throw literally
    /// anything, so this project genuinely has no bound to offer. A
    /// *resolved* signature with no `@throws` tag of its own (`sig.throws
    /// == None`) is deliberately treated as "known to throw nothing," not
    /// "unknown" - the overwhelmingly common real case (an ordinary
    /// function nobody ever documented a `@throws` tag on, because it
    /// genuinely doesn't throw), matching this project's own established
    /// "absence of a tag makes no claim either way, but isn't treated as
    /// a worse-than-nothing unknown" convention for `@internal`/
    /// `@deprecated` elsewhere. Treating it as "unknown" instead would
    /// make `current_function_has_unannotated_call` true for nearly every
    /// real function body (anything calling an untagged stdlib function
    /// at all), suppressing Sub-item C's whole comparison almost
    /// everywhere - a real, considered trade-off, not an oversight.
    pub(crate) fn record_call_throw_point(
        &mut self,
        dynamic: Option<Type>,
        sig: Option<&FunctionSig>,
    ) {
        if let Some(t) = dynamic {
            if t != Type::Void {
                self.current_function_throw_points.push(t);
            }
            return;
        }
        let Some(sig) = sig else {
            self.current_function_has_unannotated_call = true;
            return;
        };
        if let Some(t) = &sig.throws
            && *t != Type::Void
        {
            self.current_function_throw_points.push(t.clone());
        }
    }

    /// `throws` is already the union of every `@throws` tag on this
    /// declaration - checked once, at the declaration site, from
    /// `check_function`'s own "one choke point every function-shaped
    /// declaration passes through" comment.
    pub(crate) fn check_throws_tag_validity(&mut self, throws: &Type, span: Span, label: &str) {
        if throws_is_valid(throws, self.registry) {
            return;
        }
        self.report(
            label.to_string(),
            format!("PHPDoc tag @throws with type {throws} is not subtype of Throwable"),
            span,
            2,
            "throws.notThrowable",
        );
    }
}

/// Mirrors real PHPStan's own `InvalidThrowsPhpDocValueRule::isThrowsValid`:
/// every member of a union must itself be a `Throwable` subtype for the
/// whole tag to be valid. Anything this project's own type system can
/// produce for a `@throws` tag besides a plain class reference (`Mixed`
/// from an unparseable type, `Void` - real PHPStan's own explicit
/// "never throws" escape hatch, a template param, ...) is left alone -
/// "don't know, don't guess," this project's usual leniency for
/// anything short of a confident violation. Same reasoning for a class
/// name the registry has no entry for at all (real corpus shape: a
/// native-extension-only exception class, e.g. `SVMException` from the
/// `svm` extension, whose own type never gets vendored into this
/// project's own stub corpus the way `Exception`/`RuntimeException`/etc.
/// are) - `is_subclass_of` on an unknown class can't confidently say
/// anything either way, and treating "unknown" the same as "confirmed
/// not Throwable" would misreport a real typo'd/missing class the same
/// confusing way `CaughtExceptionExistenceRule`'s own existence-check
/// ordering (`checker/stmt.rs`'s `StmtKind::Try` handling) already
/// avoids for the analogous `catch` case.
fn throws_is_valid(ty: &Type, registry: &Registry) -> bool {
    match ty {
        Type::Union(members) => members.iter().all(|m| throws_is_valid(m, registry)),
        Type::Object(class, _) => {
            !registry.classes.contains_key(class) || registry.is_subclass_of(class, "Throwable")
        }
        _ => true,
    }
}
