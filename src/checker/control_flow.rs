use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::checker::{Checker, Locals, union_types};
use crate::registry::Registry;
use crate::types::Type;

/// Whether `stmt` merely exits a `switch` itself rather than the whole
/// enclosing function/loop - real PHP's own well-known `continue`-
/// inside-`switch` gotcha: a `switch` counts as one level for `break`/
/// `continue` purposes, so a bare `continue;`/`continue 1;` written
/// directly in a case is exactly `break`-equivalent (falls through to
/// after the switch, doesn't restart any enclosing loop). Only a level
/// that explicitly escapes past the switch (`continue 2;` or higher)
/// counts as a real exit. Shared by `always_terminates`'s own `Switch`
/// arm and `Checker::exits_function_or_loop` - both independently need
/// the identical "is this case's own last statement break-equivalent"
/// question, and getting it wrong the same way in only one of the two
/// would have left the other producing a different, inconsistent answer
/// for the exact same case. Real corpus false positive this fixes (found
/// verifying the `exits_function_or_loop` fix alone wasn't enough - a
/// switch statement ending in a break-equivalent `continue;` was *also*
/// still being counted by `always_terminates` as if it were a real
/// function-level exit, wrongly marking code after the switch dead).
///
/// Recurses through `Block`/`If` (a case whose own last statement is
/// `if ($c) { continue; } else { continue; }` unconditionally exits the
/// switch too, on every branch, the same as a bare `continue;` would) -
/// the identical shape `always_terminates`'s own recursion already has,
/// for the identical reason. A genuinely *mixed* branch - one side
/// break-equivalent, the other diverging a different way entirely (a
/// `: never`-returning call, `return`, `throw`) - is a real, disclosed
/// residual gap: `exits_function_or_loop`'s own recursion correctly
/// judges that shape (verified directly), but `always_terminates`'s own
/// separate, much more broadly-used recursion has no way to thread "this
/// specific `Continue` is switch-scoped" through an arbitrary nesting
/// depth without risking wrongly loosening its own unrelated, function-
/// level "does this exit for real" question elsewhere (guard-clause
/// detection, `deadCode.unreachable`'s own non-switch callers) - not
/// attempted here, narrower and more corner-case than the shapes above.
pub(crate) fn is_switch_break_equivalent(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Break(_) => true,
        StmtKind::Continue(level) => level.is_none() || *level == Some(1),
        StmtKind::Block(stmts) => stmts.last().is_some_and(is_switch_break_equivalent),
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            is_switch_break_equivalent(then_branch)
                && else_branch
                    .as_deref()
                    .is_some_and(is_switch_break_equivalent)
        }
        _ => false,
    }
}

impl<'a> Checker<'a> {
    /// The classic guard clause (`if ($x === null) { return; }`, or the
    /// less obvious `if (! $x instanceof Foo) { $this->fatalError(...); }`)
    /// depends on recognizing a branch that never falls through, so its
    /// (narrowed) scope doesn't incorrectly leak past the `if`. A bare
    /// call expression counts too, not just `return`/`throw`/`break`/
    /// `continue`, when its own return type is `never` - a real, common
    /// pattern (`private function fatalError(string $msg): never`) that a
    /// purely syntactic check would miss entirely.
    pub(crate) fn always_terminates(&self, stmt: &Stmt, locals: &Locals) -> bool {
        match &stmt.kind {
            StmtKind::Return(_)
            | StmtKind::Throw(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_) => true,
            StmtKind::Expression(e) => self.infer_expr(e, locals) == Type::Never,
            // Any statement in the block unconditionally terminating,
            // not just the last one, is sound proof the whole block
            // does - once reached, execution never falls through to
            // whatever (if anything) comes after it, dead code or not.
            // Phase 15's own `deadCode.unreachable` (`check_block`) is
            // this arm's mirror image: it's the one that actually names
            // that dead code, while this arm just needs to know it
            // exists.
            StmtKind::Block(stmts) => stmts.iter().any(|s| self.always_terminates(s, locals)),
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.always_terminates(then_branch, locals)
                    && else_branch
                        .as_deref()
                        .is_some_and(|e| self.always_terminates(e, locals))
            }
            // A `switch` guarantees ending control flow only when there's
            // a `default` (otherwise the subject might match nothing and
            // fall straight through, doing nothing) and every non-empty
            // case body genuinely terminates - not just `break`s, which
            // only exits the switch itself and falls through to whatever
            // follows it, not out of the enclosing function. An empty
            // body (`case a: case b: ...`, a pure fallthrough label) is
            // skipped rather than required to independently terminate -
            // only the eventual non-empty body it falls into needs to.
            //
            // A real, corpus-found bug lived here for a while (found via
            // `deadCode.unreachable`'s own corpus verification, in an
            // otherwise-unrelated phase): "the eventual non-empty body it
            // falls into" was this comment's own already-correct intent,
            // but the code below it never actually walked forward to find
            // that later body - it treated *any* empty case, including
            // one with no later non-empty case left to fall into at all
            // (`case PREG_NO_ERROR: case PREG_INTERNAL_ERROR: default:`
            // as the switch's own last, entirely-empty tail, a real,
            // common "explicitly do nothing for these" idiom -
            // `phpdocumentor/reflection-docblock`'s own `PcreException`,
            // `doctrine/orm`'s own `Lexer`/`Parser`), as vacuously fine -
            // wrongly concluding the whole switch always terminates when
            // that trailing-empty path actually just falls out the
            // bottom, same as if there were no `default` label there at
            // all.
            StmtKind::Switch { cases, .. } => {
                cases.iter().any(|c| c.test.is_none())
                    && cases.iter().enumerate().all(|(i, c)| {
                        if !c.body.is_empty() {
                            return c.body.last().is_some_and(|s| {
                                !is_switch_break_equivalent(s) && self.always_terminates(s, locals)
                            });
                        }
                        cases[i + 1..]
                            .iter()
                            .find(|later| !later.body.is_empty())
                            .is_some_and(|later| {
                                later.body.last().is_some_and(|s| {
                                    !is_switch_break_equivalent(s)
                                        && self.always_terminates(s, locals)
                                })
                            })
                    })
            }
            // A `try` guarantees ending control flow when its own body
            // does (whether by returning normally or by throwing - an
            // uncaught throw propagates out of the function just as
            // surely as a caught one returns from a catch block) *and*
            // every one of its `catch` clauses also does. An empty
            // `catches` list (a bare `try { ... } finally { ... }`)
            // vacuously satisfies the second half - nothing can catch and
            // fail to terminate if nothing catches at all. `finally`
            // itself doesn't need to independently terminate for this
            // question - it always runs before whatever the try/catch
            // already decided.
            StmtKind::Try { body, catches, .. } => {
                body.last()
                    .is_some_and(|s| self.always_terminates(s, locals))
                    && catches.iter().all(|c| {
                        c.body
                            .last()
                            .is_some_and(|s| self.always_terminates(s, locals))
                    })
            }
            // A round of Piwigo-corpus bucketing found `while (true) {
            // ...; if (...) { return $x; } }` (real shape,
            // `NotificationController::findAvailableFeedId`, a
            // find-a-free-key retry loop) flagged as possibly falling
            // off the function's own end - a literal `true`/non-zero
            // condition never lets the loop exit by itself, so the only
            // way past it at all is whatever `return`/`throw` inside
            // its body runs, exactly what `always_terminates` already
            // exists to recognize for every other construct.
            // Deliberately conservative about `break`: any `Break`
            // statement *anywhere* in the body - even one that actually
            // targets a different, nested loop/switch - counts as "this
            // loop might exit on its own," never wrongly concluding
            // termination. `goto`/`label` share the same theoretical
            // gap `always_terminates` already has everywhere else (see
            // Phase 15's own note on this).
            StmtKind::While { cond, body } => is_truthy_literal(cond) && !stmt_contains_break(body),
            // Same idiom, `for`'s own real spelling: `for ($i = 0; true;
            // $i++) { ... }` (real shape, `HttpClientService::
            // guardedRequest`, a redirect-following retry loop) - a
            // `for` with no condition at all (`for (;;)`) is PHP's own
            // "no condition means always true," same real semantics as
            // an explicit truthy literal.
            StmtKind::For { cond, body, .. } => {
                cond.last().is_none_or(is_truthy_literal) && !stmt_contains_break(body)
            }
            _ => false,
        }
    }

    /// Whether `stmt` exits the whole enclosing function or loop
    /// (`return`/`throw`/`continue`, or a nested construct that
    /// guarantees one of those) rather than merely exiting a `switch`
    /// itself (`break`, or not transferring control at all). Distinct
    /// from `always_terminates`'s own notion of "doesn't fall through to
    /// the next case" (which correctly treats `break` as terminating
    /// *that* question) - a switch's own post-switch scope merge needs
    /// this different distinction instead: a case ending in `break`
    /// still reaches the code after the switch, one ending in
    /// `return`/`throw`/`continue` never does.
    pub(crate) fn exits_function_or_loop(&self, stmt: &Stmt, locals: &Locals) -> bool {
        match &stmt.kind {
            StmtKind::Return(_) | StmtKind::Throw(_) => true,
            // Real PHP's own well-known `continue`-inside-`switch` gotcha:
            // a `switch` counts as one level for `break`/`continue`
            // purposes, so a bare `continue;`/`continue 1;` written
            // directly in a case (this function's own recursion never
            // descends into a nested loop's body at all - there's no
            // `While`/`For`/`Foreach`/`DoWhile`/`Switch` arm below, so any
            // `Continue` this function actually reaches is guaranteed to
            // be switch-scoped, not further-loop-scoped) is exactly
            // `break`-equivalent: it exits the switch, not any enclosing
            // loop, so code after the switch *is* reached. Only a level
            // that explicitly escapes past the switch (`continue 2;` or
            // higher) still counts as exiting. Confirmed live, this
            // session, per the user's own explicit "do all those narrow
            // [items], our corpus not having them is not a reason not to
            // do them" - re-verified directly against `php -r` semantics,
            // not merely inferred.
            StmtKind::Continue(level) => matches!(level, Some(n) if *n > 1),
            StmtKind::Break(_) => false,
            StmtKind::Expression(e) => self.infer_expr(e, locals) == Type::Never,
            StmtKind::Block(stmts) => stmts
                .last()
                .is_some_and(|s| self.exits_function_or_loop(s, locals)),
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.exits_function_or_loop(then_branch, locals)
                    && else_branch
                        .as_deref()
                        .is_some_and(|e| self.exits_function_or_loop(e, locals))
            }
            _ => false,
        }
    }

    /// Phase 55: fixed-point convergence for a loop body (`While`/
    /// `DoWhile`/`For`/`Foreach` in `checker/stmt.rs`), replacing what used
    /// to be a single `check_stmt(body, ...)` call. Without this, a
    /// variable reassigned inside the loop body keeps its pre-loop-entry
    /// type for the *entire* single-pass check - `check_block`'s own doc
    /// comment (above `Phase 15 item (1)`) traces the exact false-positive
    /// this caused for the reverted `UnreachableStatementRule`.
    ///
    /// Mirrors real PHPStan's own approach (`NodeScopeResolver::
    /// LOOP_SCOPE_ITERATIONS = 3`, confirmed directly against
    /// `vendor/phpstan-src`): run the body against a candidate entry state,
    /// see whether unioning the result back with the true pre-loop `entry`
    /// changes anything: if not, the state has converged and this pass's
    /// diagnostics are correct as reported; if so, its diagnostics were
    /// computed against a stale entry and must be discarded (the existing
    /// `diagnostics.truncate` pattern `check_program` already uses for
    /// `saw_dynamic_var_construct`), and one more pass runs with the
    /// unioned entry. Capped at 3 total passes - always keeps whichever
    /// pass is last, converged or not, rather than looping forever on a
    /// body that can't stabilize.
    ///
    /// `loop_break_states` is pushed/popped once per pass here (not by the
    /// caller) so a pre-convergence pass's own break states never leak;
    /// only the returned, kept pass's break states matter to the caller.
    ///
    /// `loop_continue_states` gets the identical per-pass push/pop, but is
    /// used completely differently: a `continue` jumps back to the top of
    /// *this* loop, so whatever state it reached is exactly as much a real
    /// entry to the next iteration as the body's own ordinary fall-through
    /// exit (`body_locals`) is - folded into `next_entry` alongside it,
    /// every pass, not just carried forward for the final kept one the way
    /// `break_states` is. Without this, a variable reassigned *only* on a
    /// path that ends in `continue` never contributed to `next_entry` at
    /// all (the fall-through-only `merge_scopes(entry, &body_locals)`
    /// below never reflects it), so convergence wrongly concluded
    /// "nothing changed" after a single pass and kept that pass's own
    /// diagnostics - computed against the stale, pre-reassignment entry.
    /// Real corpus false positive this fixes: sebastian/diff's own
    /// `Parser::parse`, whose `if ($diff !== null) { ... }` guard sits
    /// inside a branch that always ends in `continue` - `$diff` never
    /// looked reassignable at all without this, so the guard's own
    /// `!== null` narrowing wrongly saw `Type::Never` on every pass.
    pub(crate) fn converge_loop_body(
        &mut self,
        entry: &Locals,
        declared_return: Option<&Type>,
        label: &str,
        mut run_pass: impl FnMut(&mut Self, &mut Locals, Option<&Type>, &str, usize),
    ) -> (Locals, Vec<Locals>) {
        const MAX_PASSES: usize = 3;
        let mut pass_entry = entry.clone();
        for pass in 0..MAX_PASSES {
            let is_last_allowed_pass = pass == MAX_PASSES - 1;
            let mut body_locals = pass_entry.clone();
            self.loop_break_states.push(Vec::new());
            self.loop_continue_states.push(Vec::new());
            let diagnostics_before = self.diagnostics.len();
            run_pass(self, &mut body_locals, declared_return, label, pass);
            let break_states = self.loop_break_states.pop().unwrap_or_default();
            let continue_states = self.loop_continue_states.pop().unwrap_or_default();
            let mut next_entry = merge_scopes(entry, &body_locals);
            for state in &continue_states {
                next_entry = merge_scopes(&next_entry, state);
            }
            if next_entry == pass_entry || is_last_allowed_pass {
                // Bug found and fixed this session: `continue_states`
                // above only ever fed `next_entry` (deciding the *next*
                // pass's own entry, and whether convergence has been
                // reached) - the kept pass's own `body_locals`, actually
                // returned to the caller as "the loop's exit state," was
                // always the bare fallthrough-only state, silently
                // dropping every `continue`-reached state from the very
                // same pass. A variable only ever assigned on the non-
                // `continue` path (`foreach (...) { if ($x) { continue; }
                // $flag = true; }`) therefore looked unconditionally
                // assigned after the loop instead of "maybe assigned,
                // maybe not" - a real corpus false positive this exact
                // gap produced once `BooleanNotConstantConditionRule`
                // started exercising it (`Piwigo\Db\BatchWriter::updateRow`).
                // The fix folds the same `continue_states` into the
                // *returned* state too - on the loop's own last logical
                // iteration, a `continue` there is just as valid an exit
                // path as an ordinary fallthrough, the identical
                // fallthrough-vs-break distinction every caller already
                // folds `break_states` back in for afterward.
                let mut exit_locals = body_locals;
                for state in &continue_states {
                    exit_locals = merge_scopes(&exit_locals, state);
                }
                return (exit_locals, break_states);
            }
            self.diagnostics.truncate(diagnostics_before);
            pass_entry = next_entry;
        }
        unreachable!("the loop above always returns by its last iteration")
    }

    /// Phase 15 continuation: the real, original PHPStan design -
    /// `ConstantConditionRuleHelper::getBooleanType` - reinstated now
    /// that `widen_coalesce_result`'s own general-case bug (discarding
    /// the left side's type entirely instead of unioning it in) is fixed
    /// and Phase 55's fixed-point loop convergence already fixed the
    /// other reverted-before source of imprecision. Trusts `e`'s own
    /// *inferred* type being exactly `Type::True`/`Type::False` - not
    /// just a syntactic literal (`literal_truthiness`, this function's
    /// own predecessor this session, is gone: a bare literal already
    /// infers to `Type::True`/`Type::False` exactly, so inferred-type
    /// trust is a strict superset of syntactic-literal trust, not a
    /// different, incompatible design). `should_skip_constant_condition`
    /// excludes every shape that has - or, for a `Call`-shaped condition,
    /// deliberately doesn't yet have - its own separate, dedicated check.
    pub(crate) fn inferred_constant_condition(&self, e: &Expr, locals: &Locals) -> Option<bool> {
        if should_skip_constant_condition(e) {
            return None;
        }
        match self.infer_expr(e, locals) {
            Type::True => Some(true),
            Type::False => Some(false),
            _ => None,
        }
    }

    /// `BooleanAndConstantConditionRule`/`BooleanOrConstantConditionRule` -
    /// confirmed directly against `vendor/phpstan-src`: three independent
    /// diagnostics (left side, right side, whole-expression result), the
    /// left one unconditional, the other two gated by `is_first_level`
    /// (`Checker::in_condition_position`'s own doc comment - real
    /// PHPStan's `!isInFirstLevelStatement()` guard: skipped exactly when
    /// this `&&`/`||` *is* the direct condition of an `if`/`while`/
    /// ternary, since that's already covered by that construct's own
    /// check). `whole` is the same `&Expr` `check_expr`'s own `ExprKind::
    /// Binary` arm already has in hand - never reconstructed. Its own
    /// diagnostic deliberately calls `infer_expr` directly rather than
    /// going through `inferred_constant_condition` (unlike left/right,
    /// above): `whole` is itself a `Binary(And|Or, ..)` node, exactly the
    /// shape `should_skip_constant_condition` excludes so a *condition*
    /// shaped like `&&`/`||` defers to this very function instead of
    /// being double-reported by the generic if/while/ternary check -
    /// routing this function's own result computation back through that
    /// same skip-list would make it permanently unreachable.
    ///
    /// Real PHPStan's own `isLast`/`reportAlwaysTrueInLastCondition`
    /// exemption (elseif-chain-position, `check_if`'s own equivalent) is
    /// deliberately not replicated here: it comes from `LastConditionVisitor`,
    /// which only ever tags nodes reachable from an `if`/`elseif` chain's
    /// own condition list, never a bare `&&`/`||` reached as an ordinary
    /// sub-expression - there is no "chain position" concept for one to
    /// have, so nothing is actually being skipped by omitting it.
    ///
    /// `locals` here is deliberately the scope *before* `l`'s own
    /// truthy/falsey narrowing, not the caller's already-narrowed one -
    /// real bug found and fixed this session: an earlier version had the
    /// caller pass its own already-`apply_narrowing`-ed scope for every
    /// one of these three checks, including the *left* one - so a bare-
    /// variable left operand (`$same && true`) was judged against a scope
    /// where `$same` had *already* been narrowed truthy on the assumption
    /// the left side holds, making it look constantly true by pure
    /// circularity regardless of `$same`'s real, unnarrowed type. Real
    /// corpus false positive this produced: `Piwigo\Controller\Admin\
    /// ConfigurationSubController::processSizes`'s `if ($same and
    /// $new_params->sizing->max_crop !== 0.0 and (...))`, where `$same`
    /// is a genuinely reassignable local. Only the right-side check
    /// genuinely wants the left-narrowed scope (confirmed against real
    /// PHPStan's own `getRightScope()`) - narrowed internally here,
    /// rather than by the caller, so the left check can never see it by
    /// mistake again. The whole-result check also takes the *unnarrowed*
    /// `locals`: `infer_expr`/`infer_binary`'s own `BinOp::And`/`Or` arms
    /// already do their own internal left-narrowing before inferring the
    /// right side's type, so handing them an already-narrowed scope would
    /// double-apply it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_boolean_and_or(
        &mut self,
        op: &BinOp,
        l: &Expr,
        r: &Expr,
        whole: &Expr,
        locals: &Locals,
        label: &str,
        is_first_level: bool,
    ) {
        let kind = if *op == BinOp::And {
            "booleanAnd"
        } else {
            "booleanOr"
        };
        let symbol = if *op == BinOp::And { "&&" } else { "||" };
        if let Some(value) = self.inferred_constant_condition(l, locals) {
            self.report(
                label.to_string(),
                format!(
                    "Left side of {symbol} is always {}.",
                    if value { "true" } else { "false" }
                ),
                l.span,
                4,
                &format!("{kind}.leftAlways{}", if value { "True" } else { "False" }),
            );
        }
        if is_first_level {
            return;
        }
        let mut narrowed = locals.clone();
        self.apply_narrowing(l, &mut narrowed, *op == BinOp::And);
        if let Some(value) = self.inferred_constant_condition(r, &narrowed) {
            self.report(
                label.to_string(),
                format!(
                    "Right side of {symbol} is always {}.",
                    if value { "true" } else { "false" }
                ),
                r.span,
                4,
                &format!("{kind}.rightAlways{}", if value { "True" } else { "False" }),
            );
        }
        // Deliberately bypasses `inferred_constant_condition` here (unlike
        // the left/right checks above): `whole` is itself a `Binary(And|
        // Or, ..)` node, exactly the shape `should_skip_constant_condition`
        // excludes - that exclusion exists so the generic if/while/ternary
        // check defers a *condition* shaped like `&&`/`||` to this very
        // function instead of double-reporting, not to gate this
        // function's own whole-result computation against itself. Calling
        // `inferred_constant_condition(whole, ..)` here would always
        // return `None` (the same "dead code" shape as the `check_expr`
        // dead-arm bug this session already found and fixed elsewhere).
        let whole_value = match self.infer_expr(whole, locals) {
            Type::True => Some(true),
            Type::False => Some(false),
            _ => None,
        };
        if let Some(value) = whole_value {
            self.report(
                label.to_string(),
                format!(
                    "Result of {symbol} is always {}.",
                    if value { "true" } else { "false" }
                ),
                whole.span,
                4,
                &format!("{kind}.always{}", if value { "True" } else { "False" }),
            );
        }
    }

    /// `LogicalXorConstantConditionRule` - confirmed directly against
    /// `vendor/phpstan-src`: left/right only, no whole-expression-result
    /// diagnostic and no `is_first_level` gating at all (unlike `&&`/
    /// `||`) - `xor` doesn't short-circuit, so there's no "already
    /// covered by the enclosing if/while's own check" overlap to avoid,
    /// and no real rule for it exists to model that overlap regardless.
    pub(crate) fn check_logical_xor(&mut self, l: &Expr, r: &Expr, locals: &Locals, label: &str) {
        if let Some(value) = self.inferred_constant_condition(l, locals) {
            self.report(
                label.to_string(),
                format!(
                    "Left side of xor is always {}.",
                    if value { "true" } else { "false" }
                ),
                l.span,
                4,
                &format!(
                    "logicalXor.leftAlways{}",
                    if value { "True" } else { "False" }
                ),
            );
        }
        if let Some(value) = self.inferred_constant_condition(r, locals) {
            self.report(
                label.to_string(),
                format!(
                    "Right side of xor is always {}.",
                    if value { "true" } else { "false" }
                ),
                r.span,
                4,
                &format!(
                    "logicalXor.rightAlways{}",
                    if value { "True" } else { "False" }
                ),
            );
        }
    }
}

/// Joins two branch-local scopes back into one after an `if`/`while`. A
/// variable present in both branches gets the union of its two types; one
/// present in only one branch was introduced there and can't be assumed to
/// exist afterward, so it's dropped rather than carried forward unsoundly.
///
/// Phase 17 follow-up: `provenance` is merged the same way `types` is -
/// the *union* of both sides' known declaration spans for a name present
/// in both, since either branch could be the one that actually ran. This
/// is the one function every multi-way join in this checker already
/// routes through (`Switch`'s own `exit_states` fold, `Try`'s own `end_
/// states` fold, `converge_loop_body`'s own fixed-point loop) - fixing
/// provenance-merging here once is what makes every one of those inherit
/// real reaching-definitions precision, including this project's own
/// already-correct termination-pruning (`always_terminates`) and break/
/// continue handling (`loop_break_states`/`loop_continue_states`), with
/// no further change needed at any of those call sites.
pub(crate) fn merge_scopes(then_locals: &Locals, else_locals: &Locals) -> Locals {
    let mut merged = then_locals.fresh_scope();
    for (name, then_ty) in then_locals.iter() {
        if let Some(else_ty) = else_locals.get(name) {
            let mut provenance = then_locals
                .get_provenance(name)
                .cloned()
                .unwrap_or_default();
            if let Some(else_provenance) = else_locals.get_provenance(name) {
                provenance.extend(else_provenance.iter().copied());
            }
            merged.insert_with_provenance(
                name.clone(),
                union_types(then_ty.clone(), else_ty.clone()),
                provenance,
            );
        }
    }
    merged
}

/// Phase 42(4), `OverwrittenExitPointByFinallyRule`: collects the
/// concrete `return`/`throw`/`break`/`continue` statement(s) that make
/// `stmt` genuinely exit - the same question `always_terminates` already
/// answers as a bare `bool`, but naming *which* statement(s) so a
/// `finally` block that also unconditionally exits can be reported as
/// silently overwriting them (a well-known, real PHP gotcha - a
/// `finally`'s own exit point always wins, even over an in-flight
/// exception). Deliberately narrower than `always_terminates` itself:
/// only recurses into `Block`/`If` (common enough in real code to name
/// every contributing line for without guessing) - a `Switch`/`Try`/
/// loop body can still make `always_terminates` return `true`, but this
/// returns nothing for one rather than picking an arbitrary nested line,
/// the same "don't guess" leniency this project applies elsewhere. An
/// empty result here means "no diagnostic for this side," never a false
/// positive - `always_terminates`'s own gate gets checked independently
/// wherever this is called, so the two can never disagree about whether
/// something is being overwritten, only about whether this can *name* it.
pub(crate) fn collect_exit_point_statements(stmt: &Stmt) -> Vec<&Stmt> {
    match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Break(_) | StmtKind::Continue(_) => {
            vec![stmt]
        }
        StmtKind::Block(stmts) => stmts
            .last()
            .map(collect_exit_point_statements)
            .unwrap_or_default(),
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let mut out = collect_exit_point_statements(then_branch);
            if let Some(e) = else_branch {
                out.extend(collect_exit_point_statements(e));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// `OverwrittenExitPointByFinallyRule`'s own `describeExitPoint` -
/// matches its exact wording (`RuleErrorBuilder::message` interpolates
/// this directly into "This %s is overwritten..."/"The overwriting %s
/// is on this line.").
pub(crate) fn exit_point_description(stmt: &Stmt) -> &'static str {
    match &stmt.kind {
        StmtKind::Return(_) => "return",
        StmtKind::Throw(_) => "throw",
        StmtKind::Break(_) => "break",
        StmtKind::Continue(_) => "continue",
        _ => "exit point",
    }
}

/// Does this function/method/closure body contain a `yield`/`yield from`
/// anywhere, making it a real PHP generator? A generator always returns a
/// real `Generator` object when called, regardless of whether it has an
/// explicit `return` - `return;`/`return $x;` *inside* it sets
/// `Generator::getReturn()`'s own value, not the enclosing function's
/// return value, so real PHPStan never checks either against the
/// function's own declared `Generator`/`Iterator`/`Traversable` return
/// type (only against a `@return Generator<...,...,...,TReturn>`
/// docblock's 4th argument, if present - not modeled here, no corpus
/// evidence for it).
pub(crate) fn body_contains_yield(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_contains_yield)
}

/// Phase 70: whether `ty` is a real PHP generator-compatible return type,
/// mirroring real PHP's own engine-level check exactly (confirmed
/// directly via `php`/`php -l`), which only needs *one* member of a
/// union to be `Traversable`-compatible, not all of them (`int|Generator`
/// is genuinely accepted by real PHP, however misleading, and `iterable`
/// itself is `array|Traversable` - the `array` half alone would
/// otherwise wrongly sink the whole check). `Mixed` (no useful
/// information either way) is treated as always compatible, matching
/// this project's usual "don't guess against yourself" leniency for an
/// unresolved/undeclared type.
pub(crate) fn type_allows_generator_return(ty: &Type, registry: &Registry) -> bool {
    match ty {
        Type::Mixed => true,
        Type::Object(name, _) => registry.is_subclass_of(name, "Traversable"),
        Type::Union(members) | Type::Intersection(members) => members
            .iter()
            .any(|m| type_allows_generator_return(m, registry)),
        _ => false,
    }
}

/// Phase 70: an arrow function's own body is a single expression, not a
/// `Vec<Stmt>` `body_contains_yield` above can walk - this is that same
/// walk's expression-only counterpart (`expr_children` already stops at
/// a nested `Closure`/`ArrowFn` boundary, the correct semantics here
/// too: a `yield` inside a nested one doesn't make *this* arrow function
/// a generator).
pub(crate) fn expr_contains_yield(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Yield(..) | ExprKind::YieldFrom(_))
        || expr_children(e).into_iter().any(expr_contains_yield)
}

/// Mirrors `deps::collect_stmt`'s own exhaustive per-variant shape: each
/// `StmtKind` variant's own `Expr`-bearing field(s), checked via
/// `expr_contains_yield`, plus recursing into `stmt_children` for nested
/// statements. `stmt_children` (and `expr_children`, transitively)
/// already stop at `Closure`/`ArrowFn`/`NewAnonymousClass`/`FunctionDecl`/
/// `ClassDecl`/`EnumDecl` boundaries by design - exactly the correct
/// semantics here too, since a `yield` inside any of those nested scopes
/// does NOT make the *outer* function a generator.
fn stmt_contains_yield(stmt: &Stmt) -> bool {
    let own = match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => expr_contains_yield(e),
        StmtKind::Return(Some(e)) => expr_contains_yield(e),
        StmtKind::Return(None) => false,
        StmtKind::Echo(exprs) => exprs.iter().any(expr_contains_yield),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => expr_contains_yield(cond),
        StmtKind::For {
            init, cond, step, ..
        } => init.iter().chain(cond).chain(step).any(expr_contains_yield),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            expr_contains_yield(subject)
                || key.as_ref().is_some_and(expr_contains_yield)
                || expr_contains_yield(value)
        }
        StmtKind::GlobalConst(consts) => consts.iter().any(|c| expr_contains_yield(&c.value)),
        StmtKind::StaticVar(vars) => vars
            .iter()
            .any(|(_, d)| d.as_ref().is_some_and(expr_contains_yield)),
        StmtKind::Switch { subject, cases } => {
            expr_contains_yield(subject)
                || cases
                    .iter()
                    .any(|c| c.test.as_ref().is_some_and(expr_contains_yield))
        }
        _ => false,
    };
    own || stmt_children(stmt).into_iter().any(stmt_contains_yield)
}

/// Real PHP's own two spellings for "this condition is always true":
/// the literal `true`, and any non-zero integer literal (`while (1)`,
/// a real, common pre-`true`-keyword-idiom carryover). Deliberately not
/// attempting anything symbolic beyond a bare literal - a variable or
/// expression that merely *evaluates* to a truthy constant isn't
/// something this checker tracks precisely enough to trust here.
pub(crate) fn is_truthy_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Literal(Literal::Bool(true)) => true,
        ExprKind::Literal(Literal::Int(n)) => *n != 0,
        _ => false,
    }
}

/// Whether `stmt` contains a `break` anywhere in its own subtree,
/// including inside a nested loop/switch that statement doesn't
/// unqualified-`break` out of at all - deliberately conservative in the
/// same direction `always_terminates`'s own `while (true)` case needs:
/// treating any `break` found anywhere as "this loop might exit on its
/// own" only ever causes a missed simplification, never a wrong
/// termination conclusion, so there's no need to track which specific
/// loop/switch a given `break`'s own level actually targets.
///
/// Phase 15: widened from private to `pub(crate)` - `checker/stmt.rs`'s
/// own new constant-condition checks (`literal_truthiness`'s callers)
/// reuse this exact same conservatism for their own `while (true)`/
/// `do {} while (true)`/`do {} while (false)` exclusions, real PHPStan's
/// own `WhileLoopAlwaysTrueConditionRule`/`DoWhileLoopConstantCondition
/// Rule` breakless-detection.
pub(crate) fn stmt_contains_break(stmt: &Stmt) -> bool {
    matches!(stmt.kind, StmtKind::Break(_))
        || stmt_children(stmt).into_iter().any(stmt_contains_break)
}

/// A second, real gap in `while`/`do-while`'s own "always true" breakless
/// detection, found re-examining Phase 15's own disclosed Pending entry
/// ("breakless-detection reuses `stmt_contains_break`'s own 'any break
/// anywhere' conservatism... a narrow, disclosed simplification") against
/// real PHPStan's own actual source rather than trusting the prior
/// write-up: `WhileLoopAlwaysTrueConditionRule`'s real suppression logic
/// (`BreaklessWhileLoopNode::getExitPoints()`, confirmed directly against
/// `vendor/phpstan-src`) treats *any* loop exit point that isn't a
/// `break` or a bare/level-1 `continue` as disqualifying - not just a
/// `continue` whose own level escapes past this loop (the only half the
/// prior write-up named), but a `return`/`throw` reachable inside the
/// loop body at all. Neither was checked here before this fix, and both
/// have real, live corpus false positives: `PhpBench\Path\Path::
/// getLongestCommonBasePath()`'s own `while (true) { ...; continue 2; }`
/// (no literal `break` anywhere, vendored identically in `symfony/
/// filesystem`, Rector's and easy-coding-standard's own bundled copies -
/// four duplicate false positives from one real gap) and `Amp\Http\
/// Server\Driver\Http1Driver::handleClient()`'s own `while (true) { ...;
/// return; ... }`. Same maximally-conservative "anywhere in the subtree,
/// not reachability-checked" traversal `stmt_contains_break` already
/// uses (and for the identical reason: over-suppressing only ever misses
/// a diagnostic, never fabricates one) - a `continue`/`return`/`throw`
/// buried inside a nested `if`/`switch` still disqualifies, matching
/// `getExitPoints()`'s own set of *possible* exits regardless of how
/// deep the conditional nesting is.
pub(crate) fn stmt_contains_escaping_exit(stmt: &Stmt) -> bool {
    matches!(
        stmt.kind,
        StmtKind::Continue(Some(n)) if n > 1
    ) || matches!(stmt.kind, StmtKind::Return(_) | StmtKind::Throw(_))
        || stmt_children(stmt)
            .into_iter()
            .any(stmt_contains_escaping_exit)
}

/// Every label name targeted by a `goto` anywhere in `stmts`' own
/// subtree - the same maximally-conservative "anywhere, not scope-
/// aware" traversal `stmt_contains_break` already uses, for the
/// identical reason: real PHP does constrain which labels a given
/// `goto` may legally reach (it can't jump into a loop/switch/try from
/// outside it), but this project has no goto/label CFG at all (a
/// disclosed, permanent-for-now gap - ROADMAP), so recovering *which*
/// `goto` can reach *which* specific label isn't attempted; only "is
/// this label a real jump target from *somewhere* in this function"
/// matters here, and getting that wrong in the conservative direction
/// only ever suppresses a diagnostic, never fabricates one. Computed
/// once per function/closure body (see `Checker::current_function_goto_
/// targets`'s own doc comment) and consulted by `check_block`'s own
/// `deadCode.unreachable` check: a labeled statement immediately
/// following one that "always terminates" isn't unreachable at all if
/// some earlier `goto` in the same function can jump straight to it.
/// Real corpus false positive this fixes: `Amp\Http\Client\Connection\
/// Internal\Http1Parser::parse()`'s own `return $response; body_identity:
/// { ... }`, where `goto body_identity;` (twice, higher up in the same
/// function - a real, hand-written HTTP/1.1 parsing state machine) is
/// exactly this shape.
pub(crate) fn collect_goto_targets(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    fn walk(stmt: &Stmt, targets: &mut std::collections::HashSet<String>) {
        if let StmtKind::Goto(name) = &stmt.kind {
            targets.insert(name.clone());
        }
        for child in stmt_children(stmt) {
            walk(child, targets);
        }
    }
    let mut targets = std::collections::HashSet::new();
    for stmt in stmts {
        walk(stmt, &mut targets);
    }
    targets
}

/// Phase 15 continuation: real PHPStan's own `ConstantConditionRuleHelper
/// ::shouldSkip`, confirmed directly against `vendor/phpstan-src` - every
/// one of these shapes has its own separate, dedicated rule (an
/// equality/identity/ordering comparison, `instanceof`, `!`, `&&`/`||`,
/// a ternary). Excluding them here means `inferred_constant_condition`
/// only ever answers for the shapes that genuinely have nowhere else to
/// be caught.
///
/// A bare function/method/static call condition (real PHPStan's own
/// `FunctionCallConstantConditionRule`/`*Helper`) is **not** excluded
/// here, unlike an earlier version of this comment claimed - re-verified
/// directly, not assumed, after `PHASE_DEPENDENCIES.md`'s own "genuinely
/// independent" framing turned out to disagree with this file's own
/// stale reasoning. Reading `FunctionCallConstantConditionHelper.php` in
/// full reveals it isn't a separate rule at all: it's the exact same
/// `if`/`while`/`elseif`/`ternary`/`&&`/`||` rules this project already
/// has, deferring *their own* reporting - through a shared helper, not a
/// Collector - only when their own condition happens to be a call, for
/// two narrow reasons neither of which needs whole-codebase aggregation:
/// (1) dedup against a sibling `ImpossibleCheckType*` rule family this
/// project has no equivalent of at all, so this concern doesn't apply
/// here; (2) reconciling a *trait* method's own call condition across
/// every class that `use`s it, which `check_trait_methods`'s own
/// existing per-host re-check plus its own span+message dedup already
/// provides generically, no new mechanism needed. So a bare call
/// condition is just another shape `inferred_constant_condition` answers
/// for directly, the same as any other - real PHPStan's own message/
/// identifier for it is always whichever rule's own (`if.alwaysTrue`,
/// `while.alwaysTrue`, ...), never a separate one of its own, confirming
/// there's genuinely nothing more to build here than removing the
/// exclusion.
fn should_skip_constant_condition(e: &Expr) -> bool {
    matches!(
        &e.kind,
        ExprKind::Binary(
            BinOp::Eq
                | BinOp::NotEq
                | BinOp::Identical
                | BinOp::NotIdentical
                | BinOp::Gt
                | BinOp::GtEq
                | BinOp::Lt
                | BinOp::LtEq,
            _,
            _,
        ) | ExprKind::Instanceof(_, _)
            | ExprKind::Unary(UnaryOp::Not, _)
            | ExprKind::Binary(BinOp::And | BinOp::Or, _, _)
            | ExprKind::Ternary(_, _, _)
    )
}
