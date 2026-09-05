//! Phase 42's own self-contained slices - `InvalidThrowsPhpDocValueRule`
//! (`throws.notThrowable`, a `@throws` tag naming something that isn't
//! actually a `Throwable` subtype) and `ThrowsVoidFunctionWithExplicitThrowPointRule`/
//! `ThrowsVoidMethodWithExplicitThrowPointRule` (`throws.void`, an
//! explicit `@throws void` directly contradicted by a real `throw`
//! visible in the body). Both need no per-callee throw-type model at
//! all - the first is a pure tag-validity check, the second only ever
//! looks at *this* declaration's own body, not anything it calls.
//!
//! Everything else this phase's own ROADMAP entry describes (transitive
//! throw-type union across the call graph, the throw-type-covariance
//! rule family - deliberately declined this pass, see ROADMAP.md for
//! why - the opt-in "missing checked exception" rule family, the
//! dynamic per-callee throw-type extensions) is still open.

use crate::ast::{Expr, ExprKind, Stmt, StmtKind};
use crate::ast_walk::{expr_children, stmt_children};
use crate::checker::Checker;
use crate::registry::Registry;
use crate::token::Span;
use crate::types::Type;

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
