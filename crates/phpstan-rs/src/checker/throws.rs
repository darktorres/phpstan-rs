//! The checked-exception/throw-type model.
//!
//! - `throws.notThrowable` (a `@throws` tag naming something that isn't
//!   actually a `Throwable` subtype) and `throws.void` (an explicit
//!   `@throws void` directly contradicted by a real `throw` visible in
//!   the body) - self-contained, needing no per-callee throw-type model.
//! - Transitive throw-point collection: `Checker::current_function_
//!   throw_points`/`current_function_has_unannotated_call`, recorded at
//!   every real `throw` and every already-resolved-signature call choke
//!   point (`checker/expr.rs`'s `ExprKind::Call`/`New`/`MethodCall`/
//!   `StaticCall` arms, `checker/stmt/try_catch.rs`'s `check_throw`).
//! - Per-callee dynamic throw-type extensions: `dynamic` submodule,
//!   tried before each hook's plain `FunctionSig.throws` fallback.
//! - Too-wide throw-type checks: `too_wide` submodule.
//! - Opt-in "missing checked exception" family, gated on
//!   `Registry::check_missing_checked_exceptions`: `missing` submodule.
//!
//! Declined: a method-throw-type covariance check, since no call-graph-
//! wide implicit-throw model exists to build it on.

use crate::checker::Checker;
use crate::registry::{FunctionSig, Registry};
use syntax::ast::{Expr, ExprKind, Stmt, StmtKind};
use syntax::token::Span;
use syntax::visit::{child_exprs, child_stmts};
use types::Type;

mod dynamic;
mod missing;
mod too_wide;

/// Whether `stmts` contains a real, direct `throw` anywhere in its own
/// body - `Some(span)` of the first one found. Mirrors
/// `visit`'s `body_contains_yield` trio, including their
/// "stops at a nested `Closure`/`ArrowFn`/`NewAnonymousClass`/
/// `FunctionDecl`/`ClassDecl`/`EnumDecl` boundary" semantics - a `throw`
/// inside a nested closure belongs to that closure, not the outer one.
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
        child_stmts(stmt)
            .into_iter()
            .find_map(stmt_contains_explicit_throw)
    })
}

// `check_get_hook`/`check_set_hook`'s `throws.void` check needs this for
// an arrow-form hook body, which is a single `Expr`, not a `Vec<Stmt>`.
pub(crate) fn expr_contains_explicit_throw(e: &Expr) -> Option<Span> {
    if let ExprKind::Throw(_) = &e.kind {
        return Some(e.span);
    }
    child_exprs(e)
        .into_iter()
        .find_map(expr_contains_explicit_throw)
}

impl<'a> Checker<'a> {
    /// Records one call's contribution to `current_function_throw_points`/
    /// `current_function_has_unannotated_call` - shared by every
    /// call-shape choke point. `dynamic` is a per-callee override,
    /// already computed by the caller (tried first); `sig` is the
    /// plainly-resolved callee signature, whose `throws` field is the
    /// fallback once `dynamic` is `None`.
    ///
    /// `sig` being `None` (a dynamic or genuinely unresolvable call) is
    /// the one case that marks the enclosing declaration `current_
    /// function_has_unannotated_call`: real PHP lets an arbitrary callee
    /// throw anything, so there's no bound to offer. A *resolved*
    /// signature with no `@throws` tag (`sig.throws == None`) is
    /// deliberately treated as "known to throw nothing," not "unknown" -
    /// the common case of an ordinary function nobody documented a
    /// `@throws` tag on because it genuinely doesn't throw. Treating it
    /// as "unknown" instead would make `current_function_has_
    /// unannotated_call` true for nearly every real function body,
    /// suppressing the too-wide-throw-type comparison almost everywhere.
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

/// Every member of a union must itself be a `Throwable` subtype for the
/// whole tag to be valid. Anything besides a plain class reference
/// (`Mixed` from an unparseable type, `Void` as the explicit "never
/// throws" escape hatch, a template param, ...) is left alone - "don't
/// know, don't guess." Same reasoning for a class name the registry has
/// no entry for at all (e.g. a native-extension-only exception class
/// never vendored into the stub corpus): `is_subclass_of` on an unknown
/// class can't confidently say anything either way, and treating
/// "unknown" the same as "confirmed not Throwable" would misreport a
/// real typo'd/missing class.
fn throws_is_valid(ty: &Type, registry: &Registry) -> bool {
    match ty {
        Type::Union(members) => members.iter().all(|m| throws_is_valid(m, registry)),
        Type::Object(class, _) => {
            !registry.classes.contains_key(class.as_str())
                || registry.is_subclass_of(class, "Throwable")
        }
        _ => true,
    }
}
