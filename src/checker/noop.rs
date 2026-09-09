//! Phase 41 stage 1 (see ROADMAP.md's own recommendation: "NoopRule +
//! the four `WithoutSideEffectsRule`s [M effort, ships fast, no purity
//! model needed]") - real PHPStan's `Rules\DeadCode\NoopRule`, narrowed
//! to the slice this project's own AST can support without ambiguity.
//!
//! **The gate that matters most, found by reading real PHPStan's own
//! source rather than guessing**: `NoopRule` doesn't itself decide
//! whether a bare-statement expression "does nothing" - it only runs at
//! all once `Analyser\StmtHandler\ExpressionHandler` has already
//! established the *whole* expression has zero impure points and zero
//! explicit throw points (`vendor/phpstan-src/src/Analyser/StmtHandler/
//! ExpressionHandler.php`). Without an equivalent gate here, this check
//! would misfire on `$cond || $this->die('message');`'s own well-known
//! "guard or die" idiom - a real, legitimate corpus pattern this exact
//! file's neighbor (`checker/stmt.rs`'s own "Phase 32 Finding K") has
//! dedicated narrowing support for, whose top-level shape (a bare `||`
//! statement) is otherwise indistinguishable from a genuine no-op.
//! `expr_is_side_effect_free` below is a syntactic approximation of that
//! same "zero impure points" precondition - no attempt at real purity
//! *inference* (`@phpstan-pure` tags, transitive call-graph purity -
//! Phase 41's own later, XL-effort stage), just a conservative "does
//! this subtree contain anything that could possibly do something"
//! scan. Conservative in the safe direction throughout: anything this
//! can't prove side-effect-free is treated as side-effecting, never the
//! other way around.
//!
//! Real PHPStan's version additionally splits `and`/`or`
//! (`Node\Expr\BinaryOp\LogicalAnd`/`LogicalOr`) from `&&`/`||`
//! (`BooleanAnd`/`BooleanOr`) into two different identifiers with two
//! different *additional* gates on top of the shared purity one above:
//! `logicalAnd.resultUnused`/`logicalOr.resultUnused` fire regardless of
//! whether the statement also contains an assignment (the whole point,
//! since `$x = a() and b();` parses as `($x = a()) and b();` and
//! silently discards `b()`'s own result - though note `a()`/`b()` being
//! real calls means this exact example still wouldn't reach `NoopRule`
//! at all, per the purity gate above; the distinction only matters for a
//! side-effect-free assignment target, e.g. `$x = true and someCheck();`
//! where `someCheck()` genuinely has no side effects), while
//! `booleanAnd.resultUnused`/`booleanOr.resultUnused` require the
//! *additional* absence of any assignment anywhere in the statement.
//! This project's own parser deliberately erases the `and`/`or`-vs-
//! `&&`/`||` distinction at parse time (see `BinOp::LogicalXor`'s own
//! doc comment in `ast.rs`: "only their *precedence* differs, which is
//! purely a parse-time concern already resolved by the time this tree
//! exists"), so that additional split isn't recoverable here - every
//! `And`/`Or` case below is treated as the `&&`/`||`-shaped one
//! (requiring no assignment anywhere, which `expr_is_side_effect_free`
//! already guarantees, since an assignment is itself a side effect) -
//! a strict, never-wrong subset of the real rule's own combined scope.
//!
//! `xor` (`BinOp::LogicalXor`) has no competing spelling - PHP's `xor`
//! keyword is the only way to write it - so it needs no such
//! disambiguation, just the shared purity gate.
//!
//! Still open: the four `CallToFunctionStatementWithoutSideEffectsRule`/
//! `*MethodStatement*`/`*StaticMethodStatement*`/`*ConstructorStatement*`
//! rules (a call/`new` whose own signature is known to have no side
//! effects - needs real purity data this project doesn't track yet, the
//! same Phase 41 gap), and the generic "any other side-effect-free bare
//! expression" `expr.resultUnused` fallback (needs a source-expression
//! printer this project doesn't have, to name the useless expression in
//! the message).

use crate::ast::{BinOp, Expr, ExprKind, FunctionDecl, UnaryOp};
use crate::ast_walk::expr_children;
use crate::checker::{Checker, Locals};
use crate::docblock;
use crate::registry::FunctionSig;
use crate::types::Type;

/// Whether `e`'s own subtree provably has no observable side effect at
/// all - see the module doc comment for why this exists and what it
/// approximates. `expr_children`'s own "stops at a nested `Closure`/
/// `ArrowFn`/`NewAnonymousClass` boundary" semantics apply here too
/// (matching `expr_contains_yield`'s identical, already-established
/// precedent): merely *constructing* a closure/anonymous-class value
/// has no side effect of its own - only calling it would, which is a
/// separate expression this same recursion would independently flag if
/// it appeared directly in `e`'s own subtree.
fn expr_is_side_effect_free(e: &Expr) -> bool {
    let self_free = !matches!(
        e.kind,
        ExprKind::Call(..)
            | ExprKind::MethodCall(..)
            | ExprKind::StaticCall(..)
            | ExprKind::DynamicMethodCall(..)
            | ExprKind::New(..)
            | ExprKind::NewAnonymousClass(..)
            | ExprKind::Assign(..)
            | ExprKind::CompoundAssign(..)
            | ExprKind::Throw(..)
            | ExprKind::Clone(..)
            | ExprKind::Yield(..)
            | ExprKind::YieldFrom(..)
            | ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                _
            )
    );
    self_free && expr_children(e).into_iter().all(expr_is_side_effect_free)
}

impl<'a> Checker<'a> {
    /// Called from `StmtKind::Expression`'s own handling - `e` is the
    /// bare statement's own top-level expression, already fully checked
    /// by the time this runs (so this only ever *adds* a diagnostic, it
    /// never needs to walk into `e` itself for anything but the
    /// `expr_is_side_effect_free` scan above).
    pub(crate) fn check_noop_statement(&mut self, e: &Expr, label: &str) {
        let (message, identifier) = match &e.kind {
            ExprKind::Binary(BinOp::LogicalXor, ..) if expr_is_side_effect_free(e) => (
                "Unused result of \"xor\" operator.".to_string(),
                "logicalXor.resultUnused",
            ),
            ExprKind::Binary(BinOp::And, ..) if expr_is_side_effect_free(e) => (
                "Unused result of \"&&\" operator.".to_string(),
                "booleanAnd.resultUnused",
            ),
            ExprKind::Binary(BinOp::Or, ..) if expr_is_side_effect_free(e) => (
                "Unused result of \"||\" operator.".to_string(),
                "booleanOr.resultUnused",
            ),
            ExprKind::Ternary(..) if expr_is_side_effect_free(e) => (
                "Unused result of ternary operator.".to_string(),
                "ternary.resultUnused",
            ),
            _ => return,
        };
        self.report(label.to_string(), message, e.span, 4, identifier);
    }

    /// Phase 41/82's transitive-purity payoff: a bare, statement-level
    /// call whose target `registry::purity::Registry::
    /// resolve_transitive_purity` proved effect-free is dead code, the
    /// exact same family as `check_noop_statement` above (a discarded
    /// call-shaped expression instead of a logical/ternary one) - real
    /// PHPStan's own four `CallTo*StatementWithoutImpurePointsRule`s,
    /// unified into one check here since only the message/identifier
    /// differ per call shape, all four transcribed directly from
    /// `vendor/phpstan-src`, not guessed. Resolution mirrors whatever
    /// this project's own ordinary call-checking already does for each
    /// shape (`infer_expr`/`find_method_owner`/`resolve_static_class_name`),
    /// since real type inference is available here, unlike
    /// `registry::purity`'s own registry-build-time classification pass;
    /// this reads `is_transitively_pure` straight off whatever
    /// `FunctionSig` the call actually, concretely resolves to, with no
    /// need to reconstruct a `PurityDependency` key at all.
    pub(crate) fn check_pure_call_without_effect(
        &mut self,
        e: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let (message, identifier) = match &e.kind {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return;
                };
                let Some(sig) = self.registry.lookup_function(name) else {
                    return;
                };
                if !sig.is_transitively_pure {
                    return;
                }
                (
                    format!("Call to function {name}() on a separate line has no effect."),
                    "function.resultUnused",
                )
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let Some((owner, sig)) = self.find_method_owner(&recv_ty, method) else {
                    return;
                };
                if !sig.is_transitively_pure {
                    return;
                }
                // Real PHPStan's own `PossiblyPureMethodCallCollector`
                // re-applies the identical overridability gate
                // `registry::purity`'s own classification pass already
                // used to decide `sig.is_transitively_pure` in the first
                // place - a *different* question (does *this* call site
                // definitely reach `sig` itself, not some subclass's own
                // override of it). Only handled for a plain `Object`
                // receiver, matching this whole feature's own disclosed
                // scope (a `Union`/`Intersection` receiver is treated
                // conservatively - not flagged - the same "don't guess"
                // default everywhere else in this feature).
                let Type::Object(recv_class, _) = &recv_ty else {
                    return;
                };
                if !crate::registry::is_non_overridable(
                    self.registry,
                    recv_class,
                    &owner,
                    method,
                    sig,
                ) {
                    return;
                }
                (
                    format!("Call to method {owner}::{method}() on a separate line has no effect."),
                    "method.resultUnused",
                )
            }
            ExprKind::StaticCall(class_name, method, _) => {
                let Some(resolved_class) = self.resolve_static_class_name(class_name) else {
                    return;
                };
                let Some((owner, sig)) = self
                    .registry
                    .lookup_method_with_owner(&resolved_class, method)
                else {
                    return;
                };
                if !sig.is_transitively_pure {
                    return;
                }
                (
                    format!("Call to {owner}::{method}() on a separate line has no effect."),
                    "staticMethod.resultUnused",
                )
            }
            ExprKind::New(_, _) => {
                let new_ty = self.infer_expr(e, locals);
                let Type::Object(class, _) = &new_ty else {
                    return;
                };
                let Some(sig) = self.registry.lookup_method(class, "__construct") else {
                    return;
                };
                if !sig.is_transitively_pure {
                    return;
                }
                (
                    format!("Call to new {class}() on a separate line has no effect."),
                    "new.resultUnused",
                )
            }
            _ => return,
        };
        self.report(label.to_string(), message, e.span, 4, identifier);
    }

    /// `@phpstan-pure`/`@phpstan-impure` tag validation (Phase 41 follow-
    /// up) - real PHPStan's own `FunctionPurityCheck`, scoped to the one
    /// direction this project's own *binary* purity classification
    /// (`FunctionSig::is_transitively_pure` - deliberately not the
    /// richer per-point `certain`/`maybe` taxonomy real PHPStan's own
    /// version needs, see Phase 41's own Done writeup) can support
    /// without a real risk of false positives: a declaration tagged
    /// `@phpstan-impure` that this project's own classifier *proves*
    /// pure (`impureFunction.pure`/`impureMethod.pure`, confirmed
    /// directly against real PHPStan's own identical identifier/message
    /// pair). The opposite direction (`@phpstan-pure` tagged on
    /// something the classifier calls impure) is deliberately **not**
    /// built - `is_transitively_pure == false` conflates "genuinely
    /// impure" with "unknown, so conservatively disqualified" (an
    /// unresolvable receiver unconditionally disqualifies a declaration
    /// here, per this project's own disclosed precision limit versus
    /// real PHPStan), so flagging that direction would misfire on every
    /// actually-pure function this project simply can't *prove* pure -
    /// the same "favor false negatives" precedent this project holds
    /// throughout, applied to which *direction* of a two-way check is
    /// safe to build at all, not just how each one individually is
    /// worded.
    pub(crate) fn check_declared_purity_tag(
        &mut self,
        f: &FunctionDecl,
        sig: Option<&FunctionSig>,
        is_method: bool,
        label: &str,
    ) {
        let Some(sig) = sig else {
            return;
        };
        if !sig.is_transitively_pure {
            return;
        }
        let Some(doc) = f.doc_comment.as_deref() else {
            return;
        };
        if !doc.contains("@phpstan-impure") && !doc.contains("@impure") {
            return;
        }
        let block = docblock::parse(doc);
        if !block.impure {
            return;
        }
        let (description, identifier) = if is_method {
            ("Method", "impureMethod.pure")
        } else {
            ("Function", "impureFunction.pure")
        };
        self.report(
            label.to_string(),
            format!(
                "{description} {label}() is marked as impure but does not have any side effects."
            ),
            f.name_span,
            0,
            identifier,
        );
    }
}
