use crate::ast::*;
use crate::ast_walk::{any_stmt_child, stmt_children};
use crate::checker::{Checker, Locals, union_types};
use crate::types::Type;

mod boolean_conditions;
mod generators;

pub(crate) use boolean_conditions::is_truthy_literal;
pub(crate) use generators::{
    body_contains_yield, expr_contains_yield, generator_missing_return_type,
    generator_template_param, type_allows_generator_return,
};

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
            // Phase 15's own disclosed gap, half closed: a `try`/`catch`
            // introduces no new `break`/`continue` scope of its own (a
            // bare `continue;` inside a `try` still targets whatever
            // loop/switch already enclosed the `try`, exactly as if the
            // `try` weren't there), so recursing the identical way
            // `always_terminates`'s own `Try` arm does - just through
            // `exits_function_or_loop` instead - is sound with no extra
            // level-counting to get right. The *other* half of the
            // originally-disclosed gap (a nested `Switch` as a case's own
            // last statement) is deliberately still not handled here: a
            // nested switch *does* introduce a new level for `break`/
            // `continue` purposes, so correctly answering "does this
            // nested switch exit the outer function/loop" needs the
            // nested switch's own `continue N` levels reinterpreted one
            // level shallower before recursing - real complexity, not
            // effort, and the exact reason this half was originally left
            // out (see this function's own doc comment above).
            // Phase 15's own disclosed gap, half closed: a `try`/`catch`
            // introduces no new `break`/`continue` scope of its own (a
            // bare `continue;` inside a `try` still targets whatever
            // loop/switch already enclosed the `try`, exactly as if the
            // `try` weren't there), so recursing the identical way
            // `always_terminates`'s own `Try` arm does - just through
            // `exits_function_or_loop` instead - is sound with no extra
            // level-counting to get right. The *other* half of the
            // originally-disclosed gap (a nested `Switch` as a case's own
            // last statement) is deliberately still not handled here: a
            // nested switch *does* introduce a new level for `break`/
            // `continue` purposes, so correctly answering "does this
            // nested switch exit the outer function/loop" needs the
            // nested switch's own `continue N` levels reinterpreted one
            // level shallower before recursing - real complexity, not
            // effort, and the exact reason this half was originally left
            // out (see this function's own doc comment above).
            StmtKind::Try { body, catches, .. } => {
                body.last()
                    .is_some_and(|s| self.exits_function_or_loop(s, locals))
                    && catches.iter().all(|c| {
                        c.body
                            .last()
                            .is_some_and(|s| self.exits_function_or_loop(s, locals))
                    })
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
    // Phase 71 Item 4: a `function_exists()` guard only survives a merge
    // point when both incoming branches agree it's guaranteed - the same
    // reasoning every other narrowed fact here already gets (`union_types`
    // above widens a type across both branches; a guard, being a boolean
    // fact rather than a type, "widens" to "not guaranteed" the moment
    // either branch doesn't have it). Since only the `function_exists(...)`
    // true-branch itself ever inserts a name (`apply_narrowing`), this
    // also correctly makes the fact vanish the instant execution falls
    // through a non-terminating `if` with no early exit on the other
    // side - `check_if`'s own existing `then_terminates`/`else_terminates`
    // branch-selection logic (not this function) is what already makes
    // the "unconditional early return in the negative branch persists the
    // fact past the `if`" idiom work, by handing back the whole surviving
    // branch's `Locals` (guard set included) instead of a merge at all.
    merged.guarded_functions = then_locals
        .guarded_functions
        .clone()
        .intersection(else_locals.guarded_functions.clone());
    // ROADMAP "Known but not yet scheduled": identical intersection
    // treatment for the method_exists()/property_exists() guard sets -
    // a member is only still provably guarded after an if/else join when
    // *both* branches proved it, same as guarded_functions above.
    merged.guarded_methods = then_locals
        .guarded_methods
        .clone()
        .intersection(else_locals.guarded_methods.clone());
    merged.guarded_properties = then_locals
        .guarded_properties
        .clone()
        .intersection(else_locals.guarded_properties.clone());
    // ROADMAP "Known but not yet scheduled" follow-up: a condition alias
    // only survives a merge point when both incoming branches still
    // agree on the exact same stored condition - `Rc::ptr_eq` (not a
    // deep `Expr` comparison) is a correct, cheap test for this: it's
    // exactly true whenever both branches inherited the alias unchanged
    // from before the split (the overwhelmingly common case - the
    // assignment happened before an unrelated `if`/`else`, both forks
    // are still the same `Rc` allocation), and correctly false whenever
    // either branch reassigned the aliased name or the alias's own
    // target in between (`Locals::declare`'s own purge already dropped
    // it on that side, so the key is simply absent there and this filter
    // drops it too) - no case needs comparing two independently-
    // constructed-but-textually-identical conditions from different
    // branches, so no `Expr: PartialEq` bound is needed anywhere.
    merged.condition_aliases = then_locals
        .condition_aliases
        .iter()
        .filter(|(k, (_, rc))| {
            else_locals
                .condition_aliases
                .get(*k)
                .is_some_and(|(_, other_rc)| std::rc::Rc::ptr_eq(rc, other_rc))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
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
    // `any_stmt_child` (non-allocating - see its own doc comment) instead
    // of `stmt_children`/a `for` loop: this runs on every function/
    // method/closure body checked, a real, measured cost at that call
    // volume; found and fixed via a perf-optimization investigation
    // workflow. The closure always returns `false`, so `any_stmt_child`
    // never short-circuits - every child gets visited, matching the
    // original unconditional `for child in stmt_children(stmt) { ... }`
    // loop exactly.
    fn walk(stmt: &Stmt, targets: &mut std::collections::HashSet<String>) {
        if let StmtKind::Goto(name) = &stmt.kind {
            targets.insert(name.clone());
        }
        any_stmt_child(stmt, &mut |child| {
            walk(child, targets);
            false
        });
    }
    let mut targets = std::collections::HashSet::new();
    for stmt in stmts {
        walk(stmt, &mut targets);
    }
    targets
}
