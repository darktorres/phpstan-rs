use crate::checker::{Checker, Discriminant, Locals};
use syntax::ast::*;
use syntax::visit::any_stmt_in_body;
use types::{self, Relation, Type};

mod boolean_conditions;
mod generators;

pub(crate) use boolean_conditions::{is_constant_condition_family_identifier, is_truthy_literal};
pub(crate) use generators::{
    generator_missing_return_type, generator_template_param, type_allows_generator_return,
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
/// would leave the other producing a different, inconsistent answer for
/// the exact same case.
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
    syntax::stack::guard(|| match &stmt.kind {
        StmtKind::Break(_) => true,
        StmtKind::Continue(level) => level.is_none() || *level == Some(1),
        StmtKind::Block(stmts)
        | StmtKind::Declare {
            body: Some(stmts), ..
        } => stmts.last().is_some_and(is_switch_break_equivalent),
        StmtKind::If(if_stmt) => {
            if_stmt.else_branch.is_some() && if_stmt.bodies().all(is_switch_break_equivalent)
        }
        _ => false,
    })
}

/// `converge_loop_body`'s own "could this loop have run zero times"
/// parameter.
pub(crate) enum LoopZeroIterations<'a> {
    /// `do`-`while`/`for (;;)` - the body always runs at least once by
    /// construction (no condition is ever checked *before* the first
    /// pass), so the returned exit state is exactly the kept pass's own
    /// `body_locals` (`continue_states` folded in) - no merge with
    /// `entry` at all, for any variable.
    Impossible,
    /// `while`/`for`-with-a-condition/`foreach` - the condition/subject
    /// is checked *before* the first iteration, so the exit state merges
    /// with `entry` (a name only in `body_locals` is dropped, one in
    /// both is unioned - `merge_scopes`'s own existing rule, reused
    /// as-is). `subject_exempt` is a `foreach`'s own iterated subject,
    /// when it's a bare `$variable` name - excluded from that merge
    /// specifically (see `converge_loop_body`'s own doc comment on this
    /// field for why a self-mutating `foreach` needs it kept out).
    /// `subject_key` is that `foreach`'s bare `$key` variable, by value:
    /// every element the loop visits ends as the body leaves
    /// `$subject[$key]`.
    Possible {
        subject_exempt: Option<&'a str>,
        subject_key: Option<&'a str>,
    },
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
        syntax::stack::guard(|| {
            match &stmt.kind {
                StmtKind::Return(_)
                | StmtKind::Throw(_)
                | StmtKind::Break(_)
                | StmtKind::Continue(_) => true,
                StmtKind::Expression(e) => {
                    self.terminating_assertions.contains(&stmt.span)
                        || self.infer_expr(e, locals).is_never()
                }
                // Any statement in the block unconditionally terminating,
                // not just the last one, is sound proof the whole block
                // does - once reached, execution never falls through to
                // whatever (if anything) comes after it, dead code or not.
                // `deadCode.unreachable` (`check_block`) is this arm's
                // mirror image: it's the one that actually names that dead
                // code, while this arm just needs to know it exists.
                StmtKind::Block(stmts)
                | StmtKind::Declare {
                    body: Some(stmts), ..
                } => stmts.iter().any(|s| self.always_terminates(s, locals)),
                StmtKind::If(if_stmt) => {
                    if_stmt.else_branch.is_some()
                        && if_stmt.bodies().all(|b| self.always_terminates(b, locals))
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
                // An empty case with no later non-empty case to fall into at
                // all (`case A: case B: default:` as the switch's own last,
                // entirely-empty tail, a common "explicitly do nothing for
                // these" idiom) must not be treated as vacuously fine - that
                // trailing-empty path actually just falls out the bottom,
                // same as if there were no `default` label there at all.
                StmtKind::Switch { cases, .. } => {
                    cases.iter().any(|c| c.test.is_none())
                        && cases.iter().enumerate().all(|(i, c)| {
                            if !c.body.is_empty() {
                                return c.body.last().is_some_and(|s| {
                                    !is_switch_break_equivalent(s)
                                        && self.always_terminates(s, locals)
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
                // `while (true) { ...; if (...) { return $x; } }` (a
                // find-a-free-key retry loop) must not be flagged as
                // possibly falling off the function's own end - a literal
                // `true`/non-zero condition never lets the loop exit by
                // itself, so the only way past it at all is whatever
                // `return`/`throw` inside its body runs, exactly what
                // `always_terminates` already exists to recognize for every
                // other construct. Deliberately conservative about `break`:
                // any `Break` statement anywhere in the body - even one that
                // actually targets a different, nested loop/switch - counts
                // as "this loop might exit on its own," never wrongly
                // concluding termination.
                StmtKind::While { cond, body } => {
                    is_truthy_literal(cond) && !stmt_contains_break(body)
                }
                // Same idiom, `for`'s own real spelling: `for ($i = 0; true;
                // $i++) { ... }` (a redirect-following retry loop) - a `for`
                // with no condition at all (`for (;;)`) is PHP's own "no
                // condition means always true," same real semantics as an
                // explicit truthy literal.
                StmtKind::For { cond, body, .. } => {
                    cond.last().is_none_or(is_truthy_literal) && !stmt_contains_break(body)
                }
                _ => false,
            }
        })
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
        syntax::stack::guard(|| {
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
                // higher) still counts as exiting.
                StmtKind::Continue(level) => matches!(level, Some(n) if *n > 1),
                StmtKind::Break(_) => false,
                StmtKind::Expression(e) => {
                    self.terminating_assertions.contains(&stmt.span)
                        || self.infer_expr(e, locals).is_never()
                }
                StmtKind::Block(stmts)
                | StmtKind::Declare {
                    body: Some(stmts), ..
                } => stmts
                    .last()
                    .is_some_and(|s| self.exits_function_or_loop(s, locals)),
                StmtKind::If(if_stmt) => {
                    if_stmt.else_branch.is_some()
                        && if_stmt
                            .bodies()
                            .all(|b| self.exits_function_or_loop(b, locals))
                }
                // A `try`/`catch` introduces no new `break`/`continue` scope
                // of its own (a bare `continue;` inside a `try` still
                // targets whatever loop/switch already enclosed the `try`,
                // exactly as if the `try` weren't there), so recursing the
                // identical way `always_terminates`'s own `Try` arm does -
                // just through `exits_function_or_loop` instead - is sound
                // with no extra level-counting to get right. A nested
                // `Switch` as a case's own last statement is deliberately
                // still not handled here: a nested switch does introduce a
                // new level for `break`/`continue` purposes, so correctly
                // answering "does this nested switch exit the outer
                // function/loop" would need the nested switch's own
                // `continue N` levels reinterpreted one level shallower
                // before recursing.
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
        })
    }

    /// Fixed-point convergence for a loop body (`While`/`DoWhile`/`For`/
    /// `Foreach` in `checker/stmt.rs`) - without this, a variable
    /// reassigned inside the loop body keeps its pre-loop-entry type for
    /// the entire single-pass check, which `check_block`'s own doc
    /// comment traces the exact false-positive for.
    ///
    /// Mirrors real PHPStan's own approach (`NodeScopeResolver::
    /// LOOP_SCOPE_ITERATIONS = 3`): run the body against a candidate entry state,
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
    /// below never reflects it), so convergence would wrongly conclude
    /// "nothing changed" after a single pass and keep that pass's own
    /// diagnostics - computed against the stale, pre-reassignment entry.
    pub(crate) fn converge_loop_body(
        &mut self,
        entry: &Locals,
        declared_return: Option<&Type>,
        label: &str,
        // `while`/`for`/`foreach` can all genuinely run zero times (the
        // condition/subject is checked *before* the first iteration) -
        // `Possible` merges the exit state with `entry` accordingly (see
        // the loop's own comment below for exactly how). A `do`-`while`/
        // `for (;;)` is the one real exception: its own body always runs
        // at least once by construction (no condition is ever checked
        // before the first pass), so `check_do_while`/`check_for`'s own
        // no-condition arm pass `Impossible` to skip that merge entirely -
        // otherwise a body-only local (`do { $changed = false; ...; }
        // while ($changed);`) that must stay defined afterward would be
        // merged away as if the loop might never have run, wrongly
        // flagging `variable.undefined`.
        zero_iterations: LoopZeroIterations<'_>,
        mut run_pass: impl FnMut(&mut Self, &mut Locals, Option<&Type>, &str, usize),
    ) -> (Locals, Vec<Locals>) {
        // A variable still changing after `WIDEN_FROM_PASS` passes (a
        // counter, a string built up, an array grown key by key) is
        // widened so the next pass can converge - an int only on the side
        // it's moving, anything else to its general type; its literal values
        // would otherwise run out the passes and leave the exit state
        // short of what later iterations produce.
        const MAX_PASSES: usize = 6;
        const WIDEN_FROM_PASS: usize = 2;
        let mut pass_entry = entry.clone();
        for pass in 0..MAX_PASSES {
            let is_last_allowed_pass = pass == MAX_PASSES - 1;
            let mut body_locals = pass_entry.clone();
            self.loop_break_states.push(Vec::new());
            self.loop_continue_states.push(Vec::new());
            let mark = self.pass_mark();
            run_pass(self, &mut body_locals, declared_return, label, pass);
            let break_states = self.loop_break_states.pop().unwrap_or_default();
            let continue_states = self.loop_continue_states.pop().unwrap_or_default();
            let mut next_entry = merge_scopes(&self.relation(), entry, &body_locals);
            for state in &continue_states {
                next_entry = merge_scopes(&self.relation(), &next_entry, state);
            }
            if pass >= WIDEN_FROM_PASS && next_entry != pass_entry {
                // Only a growing value widens; one that shrank back inside
                // an earlier widening narrows to what the pass gave.
                let relation = self.relation();
                let changing: Vec<(String, Type)> = next_entry
                    .iter()
                    .filter(|(name, ty)| match pass_entry.get(name.as_str()) {
                        Some(before) => {
                            before != *ty && !relation.is_super_type_of(before, ty).yes()
                        }
                        None => true,
                    })
                    .map(|(name, ty)| {
                        let widened = match pass_entry.get(name.as_str()) {
                            Some(before) => before.widen_towards(ty),
                            None => ty.generalize(),
                        };
                        (name.clone(), widened)
                    })
                    .collect();
                for (name, ty) in changing {
                    next_entry.insert(name, ty);
                }
            }
            if next_entry == pass_entry || is_last_allowed_pass {
                // The kept pass's own `body_locals`, returned to the
                // caller as "the loop's exit state," is the bare
                // fallthrough-only state on its own - `continue_states`
                // must be folded in here too, or a variable only ever
                // assigned on the non-`continue` path (`foreach (...) {
                // if ($x) { continue; } $flag = true; }`) would look
                // unconditionally assigned after the loop instead of
                // "maybe assigned, maybe not." On the loop's own last
                // logical iteration, a `continue` there is just as valid
                // an exit path as an ordinary fallthrough, the identical
                // fallthrough-vs-break distinction every caller already
                // folds `break_states` back in for afterward.
                let mut exit_locals = body_locals;
                for state in &continue_states {
                    exit_locals = merge_scopes(&self.relation(), &exit_locals, state);
                }
                // A variable only ever assigned inside the loop body but
                // genuinely tracked before it too (`$x = null; foreach
                // (...) { $x = ...; }`) needs the same "maybe never
                // touched" treatment as one introduced fresh inside the
                // body - both must union with whatever `entry` already
                // held for that name, not just adopt the body's own
                // post-iteration value outright. `next_entry` (computed
                // just above - `merge_scopes(entry, &body_locals)`,
                // `continue_states` already folded in) already does
                // exactly this for every name uniformly: union one
                // present in both, drop one present only in
                // `body_locals`.
                //
                // The one real exception - `LoopZeroIterations::Possible`'s
                // own `subject_exempt`, a `foreach`'s own iterated subject
                // when it's a bare `$variable` - is a `foreach` mutating
                // its own subject array in place (`foreach ($items as
                // $day => ...) { $items[$day]['derivative'] = ...; }`),
                // which must never have its already-transformed shape
                // re-widened back toward the stale pre-loop one `entry`
                // still holds for that same variable - every key really
                // iterated over gets the loop's own treatment, never the
                // old one. Restored from `exit_locals`'s own (pre-union)
                // value after the general union, rather than being
                // excluded from it.
                if let LoopZeroIterations::Possible {
                    subject_exempt,
                    subject_key,
                } = zero_iterations
                {
                    // Every element visited ends as the body leaves the
                    // element its key names, when the body's exit knows
                    // it; iterating zero times left the subject as it
                    // was, which for an array is empty.
                    let subject_override = subject_exempt.and_then(|name| {
                        let iterated = exit_locals.get(name).cloned()?;
                        let iterated = match subject_key
                            .and_then(|key| exit_locals.get(&format!("{name}[${key}]")))
                        {
                            Some(element) if iterated.is_array().yes() => {
                                iterated.with_general_array_value(element)
                            }
                            _ => iterated,
                        };
                        Some(match entry.get(name) {
                            Some(before) if before.is_array().yes() => {
                                if before.is_non_empty_array().yes() {
                                    iterated
                                } else {
                                    self.relation().union(
                                        iterated,
                                        Type::array_shape(types::Shape::sealed(Vec::new())),
                                    )
                                }
                            }
                            Some(before) => self.relation().union(iterated, before.clone()),
                            None => iterated,
                        })
                    });
                    exit_locals = next_entry;
                    if let (Some(name), Some(ty)) = (subject_exempt, subject_override) {
                        exit_locals.insert(name.to_string(), ty);
                    }
                }
                return (exit_locals, break_states);
            }
            self.rewind_to(mark);
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
/// `provenance` is merged the same way `types` is - the union of both
/// sides' known declaration spans for a name present in both, since
/// either branch could be the one that actually ran. This is the one
/// function every multi-way join in this checker already routes through
/// (`Switch`'s own `exit_states` fold, `Try`'s own `end_states` fold,
/// `converge_loop_body`'s own fixed-point loop) - fixing
/// provenance-merging here once is what makes every one of those inherit
/// real reaching-definitions precision, with no further change needed
/// at any of those call sites.
#[hotpath::measure]
/// Adds the fact that `name` is `ty` to `facts`. Two facts for one
/// variable under one discriminant come from different paths, so it is
/// either.
pub(crate) fn add_implied_fact(
    relation: &Relation,
    facts: &mut Vec<(String, Type)>,
    name: &str,
    ty: &Type,
) {
    match facts.iter_mut().find(|(k, _)| k == name) {
        Some((_, existing)) if existing == ty => {}
        Some((_, existing)) => *existing = relation.union(existing.clone(), ty.clone()),
        None => facts.push((name.to_string(), ty.clone())),
    }
}

/// Both sides' facts under one discriminant. A variable only one side has
/// a fact for is, on the other side, whatever that side holds, since the
/// discriminant held there too.
fn merge_implied_facts(
    relation: &Relation,
    then_facts: &[(String, Type)],
    else_facts: &[(String, Type)],
    then_locals: &Locals,
    else_locals: &Locals,
) -> Option<std::rc::Rc<Vec<(String, Type)>>> {
    let mut combined: Vec<(String, Type)> = Vec::new();
    for (name, ty) in then_facts {
        match else_facts.iter().find(|(k, _)| k == name) {
            Some((_, other)) => {
                add_implied_fact(relation, &mut combined, name, ty);
                add_implied_fact(relation, &mut combined, name, other);
            }
            None => {
                if let Some(other) = else_locals.get(name) {
                    add_implied_fact(relation, &mut combined, name, ty);
                    add_implied_fact(relation, &mut combined, name, other);
                }
            }
        }
    }
    for (name, ty) in else_facts {
        if then_facts.iter().any(|(k, _)| k == name) {
            continue;
        }
        if let Some(other) = then_locals.get(name) {
            add_implied_fact(relation, &mut combined, name, ty);
            add_implied_fact(relation, &mut combined, name, other);
        }
    }
    (!combined.is_empty()).then(|| std::rc::Rc::new(combined))
}

/// One side's facts under `key`, merged with an `other` side that has none:
/// kept when the discriminant cannot hold there, and otherwise each widened
/// by what `other` holds for its variable.
fn widen_implied_facts(
    relation: &Relation,
    facts: &std::rc::Rc<Vec<(String, Type)>>,
    key: &Discriminant,
    other: &Locals,
) -> Option<std::rc::Rc<Vec<(String, Type)>>> {
    if !discriminant_may_hold(other, key) {
        return Some(facts.clone());
    }
    let mut widened: Vec<(String, Type)> = Vec::new();
    for (name, ty) in facts.iter() {
        if let Some(other_ty) = other.get(name) {
            add_implied_fact(relation, &mut widened, name, ty);
            add_implied_fact(relation, &mut widened, name, other_ty);
        }
    }
    (!widened.is_empty()).then(|| std::rc::Rc::new(widened))
}

/// Whether `locals` allows the discriminant `key` (`subject==value`, see
/// `Discriminant`) to hold. A subject that isn't a local, or a
/// value this can't compare, may.
fn discriminant_may_hold(locals: &Locals, key: &Discriminant) -> bool {
    let value = &key.value;
    let Some(ty) = locals.get(&key.subject) else {
        // An undefined variable reads as `null`.
        return value.is_false().yes()
            || !crate::checker::narrowing::is_real_variable_key(&key.subject);
    };
    let members = ty.union_members();
    if value.is_true().yes() {
        members
            .iter()
            .any(|m| !(m.is_null().yes() || m.is_false().yes()))
    } else if value.is_false().yes() {
        members.iter().any(|m| !m.is_true().yes())
    } else {
        members.iter().any(|m| !m.is_single_value() || m == value)
    }
}

pub(crate) fn merge_scopes(
    relation: &Relation,
    then_locals: &Locals,
    else_locals: &Locals,
) -> Locals {
    let mut merged = then_locals.fresh_scope();
    merged.set_any_variable_may_exist(
        then_locals.any_variable_may_exist() || else_locals.any_variable_may_exist(),
    );
    // A `function_exists()` guard only survives a merge
    // point when both incoming branches agree it's guaranteed - the same
    // reasoning every other narrowed fact here already gets (`Type::union`
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
    *merged.guarded_functions_mut() = then_locals
        .guarded_functions()
        .clone()
        .intersection(else_locals.guarded_functions().clone());
    *merged.guarded_constants_mut() = then_locals
        .guarded_constants()
        .clone()
        .intersection(else_locals.guarded_constants().clone());
    *merged.guarded_classes_mut() = then_locals
        .guarded_classes()
        .clone()
        .intersection(else_locals.guarded_classes().clone());
    // Identical intersection treatment for the
    // method_exists()/property_exists() guard sets - a member is only
    // still provably guarded after an if/else join when both branches
    // proved it, same as guarded_functions above.
    *merged.guarded_methods_mut() = then_locals
        .guarded_methods()
        .clone()
        .intersection(else_locals.guarded_methods().clone());
    *merged.guarded_properties_mut() = then_locals
        .guarded_properties()
        .clone()
        .intersection(else_locals.guarded_properties().clone());
    // Reinitialized on either path is possibly reinitialized.
    *merged.clone_reinitialized_mut() = then_locals
        .clone_reinitialized()
        .clone()
        .union(else_locals.clone_reinitialized().clone());
    // A condition alias only survives a merge point when both incoming
    // branches still
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
    *merged.condition_aliases_mut() = then_locals
        .condition_aliases()
        .iter()
        .filter(|(k, (_, rc))| {
            else_locals
                .condition_aliases()
                .get(*k)
                .is_some_and(|(_, other_rc)| std::rc::Rc::ptr_eq(rc, other_rc))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // A native type survives only when both branches know one.
    for (key, then_native) in then_locals.natives() {
        if let Some(else_native) = else_locals.native(key) {
            merged.set_native(
                key,
                relation.union(then_native.clone(), else_native.clone()),
            );
        }
    }
    // Same "both branches inherited the identical, unchanged alias"
    // dedup as `condition_aliases` above - a `QueryBuilder` alias has no
    // separate target to intersect against, just the one stored `Rc<Expr>`.
    *merged.query_builder_aliases_mut() = then_locals
        .query_builder_aliases()
        .iter()
        .filter(|(k, rc)| {
            else_locals
                .query_builder_aliases()
                .get(*k)
                .is_some_and(|other_rc| std::rc::Rc::ptr_eq(rc, other_rc))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // Same shape as `query_builder_aliases` above, for the identical
    // reason: a container's own per-field facts (see
    // `ContainerFieldFactsMap`'s doc comment) can be invalidated on just
    // *one* branch (a rebind, or a by-ref pass to an unrecognized
    // function - either purges the container's own entry, see
    // `purge_stale_narrowing_for`), so a merge must only keep an entry
    // both sides still carry *unchanged*, never resurrect one only one
    // side still has - unlike `branch_implied_facts` below, this is not a
    // durable structural fact safe to union.
    *merged.container_field_facts_mut() = then_locals
        .container_field_facts()
        .iter()
        .filter(|(k, rc)| {
            else_locals
                .container_field_facts()
                .get(*k)
                .is_some_and(|other_rc| std::rc::Rc::ptr_eq(rc, other_rc))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // See `DiscriminantArrayFactsMap`'s own doc comment - unions rather
    // than intersects, like `branch_implied_facts` below: a
    // `(container, subject, true)` entry only ever gets written inside
    // the `subject`-true branch, so it's naturally absent from the
    // `subject`-false branch's own fork - that absence means "the other
    // branch had no reason to touch it," not "invalidated," so it must
    // survive the merge. A key present on both sides (the same
    // container/subject/value slot written independently on each side -
    // not expected for the single-`if` shape this mechanism currently
    // triggers on, but possible from nested control flow) is unioned,
    // never silently overwritten by one side.
    *merged.discriminant_array_facts_mut() = then_locals.discriminant_array_facts().clone();
    for (key, else_ty) in else_locals.discriminant_array_facts().iter() {
        match merged.discriminant_array_facts().get(key).cloned() {
            Some(then_ty) if then_ty == *else_ty => {}
            Some(then_ty) => {
                merged
                    .discriminant_array_facts_mut()
                    .insert(key.clone(), relation.union(then_ty, else_ty.clone()));
            }
            None => {
                merged
                    .discriminant_array_facts_mut()
                    .insert(key.clone(), else_ty.clone());
            }
        }
    }
    // Unlike `condition_aliases` above, a `branch_implied_facts` entry is
    // a durable structural fact about the code at one exact discriminant
    // (see that field's own doc comment) - true whenever that
    // discriminant holds again, regardless of which branch of *this*
    // merge point actually ran, so a merge unions both sides rather than
    // intersecting. `Rc::ptr_eq`-deduped when both sides inherited the
    // identical entry unchanged (the common case - the fact was recorded
    // before this split), otherwise both sides' own fact lists are kept
    // (a genuinely new fact recorded independently on each side, from
    // two different `if`s sharing one discriminant).
    *merged.branch_implied_facts_mut() = then_locals.branch_implied_facts().clone();
    for (key, else_facts) in else_locals.branch_implied_facts().iter() {
        let combined = match then_locals.branch_implied_facts().get(key) {
            Some(then_facts) if std::rc::Rc::ptr_eq(then_facts, else_facts) => continue,
            Some(then_facts) => {
                merge_implied_facts(relation, then_facts, else_facts, then_locals, else_locals)
            }
            None => widen_implied_facts(relation, else_facts, key, then_locals),
        };
        match combined {
            Some(facts) => merged.branch_implied_facts_mut().insert(key.clone(), facts),
            None => merged.branch_implied_facts_mut().remove(key),
        };
    }
    for (key, then_facts) in then_locals.branch_implied_facts().iter() {
        if else_locals.branch_implied_facts().contains_key(key) {
            continue;
        }
        match widen_implied_facts(relation, then_facts, key, else_locals) {
            Some(facts) => merged.branch_implied_facts_mut().insert(key.clone(), facts),
            None => merged.branch_implied_facts_mut().remove(key),
        };
    }
    for (name, then_ty) in then_locals.iter() {
        if let Some(else_ty) = else_locals.get(name) {
            let mut provenance = then_locals
                .get_provenance(name)
                .cloned()
                .unwrap_or_default();
            if let Some(else_provenance) = else_locals.get_provenance(name) {
                provenance.extend(else_provenance.iter().copied());
            }
            // `Type::union` takes its operands by value, so equal types
            // (a variable neither branch reassigned, the common case) are
            // kept without cloning both.
            let merged_ty = if then_ty == else_ty {
                then_ty.clone()
            } else {
                relation.union(then_ty.clone(), else_ty.clone())
            };
            merged.insert_with_provenance(name.clone(), merged_ty, provenance);
        }
    }
    merged
}

/// `OverwrittenExitPointByFinallyRule`: collects the
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
    syntax::stack::guard(|| match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Break(_) | StmtKind::Continue(_) => {
            vec![stmt]
        }
        StmtKind::Block(stmts)
        | StmtKind::Declare {
            body: Some(stmts), ..
        } => stmts
            .last()
            .map(collect_exit_point_statements)
            .unwrap_or_default(),
        StmtKind::If(if_stmt) => if_stmt
            .bodies()
            .flat_map(collect_exit_point_statements)
            .collect(),
        _ => Vec::new(),
    })
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
/// `checker/stmt.rs`'s own constant-condition checks
/// (`literal_truthiness`'s callers) reuse this exact same conservatism
/// for their own `while (true)`/`do {} while (true)`/`do {} while
/// (false)` exclusions, real PHPStan's own
/// `WhileLoopAlwaysTrueConditionRule`/`DoWhileLoopConstantConditionRule`
/// breakless-detection.
pub(crate) fn stmt_contains_break(stmt: &Stmt) -> bool {
    any_stmt_in_body(stmt, |s| matches!(s.kind, StmtKind::Break(_)))
}

/// `while`/`do-while`'s own "always true" breakless detection needs more
/// than just `stmt_contains_break`:
/// `WhileLoopAlwaysTrueConditionRule`'s real suppression logic
/// (`BreaklessWhileLoopNode::getExitPoints()`) treats any loop exit
/// point that isn't a `break` or a bare/level-1 `continue` as
/// disqualifying - not just a `continue` whose own level escapes past
/// this loop, but a `return`/`throw` reachable inside the loop body at
/// all (e.g. `while (true) { ...; continue 2; }` with no literal
/// `break` anywhere, or `while (true) { ...; return; ... }`). Same
/// maximally-conservative "anywhere in the subtree, not
/// reachability-checked" traversal `stmt_contains_break` already uses
/// (and for the identical reason: over-suppressing only ever misses a
/// diagnostic, never fabricates one) - a `continue`/`return`/`throw`
/// buried inside a nested `if`/`switch` still disqualifies, matching
/// `getExitPoints()`'s own set of possible exits regardless of how deep
/// the conditional nesting is.
pub(crate) fn stmt_contains_escaping_exit(stmt: &Stmt) -> bool {
    any_stmt_in_body(stmt, |s| {
        matches!(
            s.kind,
            StmtKind::Continue(Some(2..)) | StmtKind::Return(_) | StmtKind::Throw(_)
        )
    })
}

/// Every label name targeted by a `goto` anywhere in `stmts`' own
/// subtree - the same maximally-conservative "anywhere, not scope-
/// aware" traversal `stmt_contains_break` already uses, for the
/// identical reason: real PHP does constrain which labels a given
/// `goto` may legally reach (it can't jump into a loop/switch/try from
/// outside it), but this project has no goto/label CFG at all, so
/// recovering which `goto` can reach which specific label isn't
/// attempted; only "is this label a real jump target from somewhere in
/// this function" matters here, and getting that wrong in the
/// conservative direction only ever suppresses a diagnostic, never
/// fabricates one. Computed once per function/closure body (see
/// `Checker::current_function_goto_targets`'s own doc comment) and
/// consulted by `check_block`'s own `deadCode.unreachable` check: a
/// labeled statement immediately following one that "always terminates"
/// isn't unreachable at all if some earlier `goto` in the same function
/// can jump straight to it.
pub(crate) fn collect_goto_targets(stmts: &[Stmt]) -> rustc_hash::FxHashSet<syntax::name::Name> {
    let mut targets = rustc_hash::FxHashSet::default();
    for stmt in stmts {
        any_stmt_in_body(stmt, |s| {
            if let StmtKind::Goto(name) = &s.kind {
                targets.insert(name.name.clone());
            }
            false
        });
    }
    targets
}
