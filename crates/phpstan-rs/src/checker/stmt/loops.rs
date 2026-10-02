use rustc_hash::FxHashMap;
use std::rc::Rc;

use crate::checker::{
    Checker, Locals, LoopZeroIterations, count_call_operand, is_truthy_literal, merge_scopes,
    propagate_container_field_facts, stmt_contains_break, stmt_contains_escaping_exit,
};
use syntax::ast::*;
use syntax::visit::body_contains_yield;
use types::Type;

/// `Checker::in_range_index_writes`'s own trigger - whether `cond` is
/// syntactically `$i < count($container)` (either operand order,
/// reusing `count_call_operand`'s own existing `count(...)`-call
/// detection, already correct about a leading-`\`/namespaced call),
/// returning `(container name, index variable name)`. Deliberately
/// syntactic only: a `count($x)` *captured into a variable* before the
/// loop (`$n = count($x); for (; $i < $n; )`) does *not* match -
/// confirmed via `dumpType()` against real PHPStan that list-ness
/// genuinely isn't preserved for that shape, only for a fresh `count()`
/// re-evaluated in the condition itself.
fn in_range_index_write_loop_vars(cond: &Expr) -> Option<(String, String)> {
    let ExprKind::Binary(op, l, r) = &cond.peeled().kind else {
        return None;
    };
    if !matches!(op, BinOp::Lt | BinOp::Gt) {
        return None;
    }
    // `count_call_operand`'s own bool means "found in `l`" (true) or
    // "found in `r`" (false) - `$i < count($x)` has `count(...)` in `r`
    // (`count_on_left = false`), matching `Lt`'s own natural left-to-
    // right reading; `count($x) > $i` has it in `l` (`true`), the `Gt`
    // mirror.
    let (count_arg, count_on_left) = count_call_operand(l, r)?;
    let idx_expr = match (op, count_on_left) {
        (BinOp::Lt, false) => l,
        (BinOp::Gt, true) => r,
        _ => return None,
    };
    let ExprKind::Variable(idx_name) = &idx_expr.peeled().kind else {
        return None;
    };
    let ExprKind::Variable(container_name) = &count_arg.peeled().kind else {
        return None;
    };
    Some((container_name.to_string(), idx_name.to_string()))
}

impl<'a> Checker<'a> {
    pub(crate) fn check_while(
        &mut self,
        cond: &Expr,
        body: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        // The condition's own constant-condition check (and,
        // transitively, any standalone check reached while checking it,
        // like `BooleanNot`/`BooleanAnd`/`BooleanOr`) must run against
        // the converged fixed point, not just once against the pre-loop
        // entry `locals` before `converge_loop_body` runs - otherwise,
        // e.g., `$end = false; while (!$end) { ...; $end = true; ... }`:
        // `$end`'s pre-loop type is exactly `false`, so `!$end` looks
        // unconditionally `true` even though the loop body can reassign
        // it. Mirrors real PHPStan's own `WhileHandler::processStmt`,
        // which re-evaluates the condition at the top of *every*
        // convergence pass and only lets the FINAL, fully-converged
        // pass's walk reach the real rule-invoking callback. Moving the
        // condition check inside `converge_loop_body`'s own closure
        // reproduces that for free, since every pass but the kept one
        // already has its diagnostics discarded via
        // `converge_loop_body`'s own `diagnostics.truncate`.
        let skip_always_true = stmt_contains_break(body)
            || stmt_contains_escaping_exit(body)
            || body_contains_yield(std::slice::from_ref(body))
            || declared_return.is_some_and(Type::is_never);
        // On top of the above: moving the diagnostic-bearing condition
        // check into the convergence closure means it only runs against
        // per-pass *copies* of the entry (`pass_locals`), never mutating
        // the real `locals` directly. A real PHP `while`'s condition
        // always runs at least once, unconditionally, regardless of how
        // many times the body iterates - so a variable the condition
        // itself assigns (`while (($x = f()) !== null) {...}`, a real,
        // common idiom) must be treated as unconditionally defined
        // afterward, unlike anything only the body conditionally
        // defines - otherwise `merge_scopes` would drop it entirely
        // (present in the loop's exit state but never in the real
        // pre-loop `locals` it gets merged against). Mirrors real
        // PHPStan's own `WhileHandler::processStmt`, which does a quiet,
        // non-diagnostic evaluation of the condition against the real
        // pre-loop scope first, strictly separate from the later
        // diagnostic-bearing walk against the converged state -
        // replicated here by running `check_expr` once for real (to
        // bind any assignment side effect into `locals`) and discarding
        // whatever it reported, since the real diagnostics come from the
        // convergence-aware walk below.
        let mark = self.pass_mark();
        self.check_expr(cond, locals, label);
        self.rewind_to(mark);
        let entry = locals.clone();
        self.loop_depth += 1;
        let (body_locals, break_states) = self.converge_loop_body(
            &entry,
            declared_return,
            label,
            LoopZeroIterations::Possible {
                subject_exempt: None,
                subject_key: None,
            },
            |this, pass_locals, declared_return, label, _pass| {
                this.in_condition_position = true;
                let cond_ty = this.check_expr(cond, pass_locals, label);
                this.check_non_boolean_condition(
                    cond,
                    &cond_ty,
                    "a while condition",
                    "while.condNotBoolean",
                );
                // `WhileLoopAlwaysFalseConditionRule`/
                // `WhileLoopAlwaysTrueConditionRule`: real
                // inferred-type trust (`inferred_constant_condition`),
                // matching real PHPStan's own design - see `check_if`.
                // `alwaysFalse` has no exclusions, matching real
                // PHPStan's rule exactly. `alwaysTrue` is gated the
                // same way real PHPStan's own
                // `WhileLoopAlwaysTrueConditionRule` is: skipped when
                // the loop is breakless (`stmt_contains_break` - this
                // project's own deliberately conservative "any break
                // anywhere" check, reused rather than re-deriving real
                // PHPStan's own more precise per-level break/
                // `continue N` analysis), when the body yields (a
                // legitimate infinite generator, not dead code), or when
                // the enclosing function is declared `: never` (already
                // documents "this never returns" on its own, redundant
                // to also flag the loop).
                if this.inferred_constant_condition(cond, pass_locals) == Some(false) {
                    this.report(
                        "While loop condition is always false.".to_string(),
                        cond.span,
                        4,
                        "while.alwaysFalse",
                    );
                } else if this.inferred_constant_condition(cond, pass_locals) == Some(true)
                    && !skip_always_true
                {
                    this.report(
                        "While loop condition is always true.".to_string(),
                        cond.span,
                        4,
                        "while.alwaysTrue",
                    );
                }
                this.apply_narrowing(cond, pass_locals, true);
                this.check_stmt(body, pass_locals, declared_return, label);
            },
        );
        self.loop_depth -= 1;
        if is_truthy_literal(cond) {
            // The condition can never itself become false (a literal
            // `while (true)`/`while (1)`), so the *only* way to reach
            // code after this loop is via a `break` somewhere in the
            // body - neither the "may run zero times" merge below nor
            // the "reaching here means cond went false" narrowing after
            // it applies. `is_truthy_literal` is the same check
            // `always_terminates`'s own `While` arm uses one level up.
            // Must fold every `break`-reached state together instead of
            // merging against the pre-loop `locals` (which the
            // unconditional zero-iteration merge below would otherwise
            // do), or `while (true) { $x = ...; break; } return $x;`
            // would discard `$x` from every representative state that
            // reached the `break`, leaving it looking undefined
            // afterward.
            //
            // Seeded by the first `break`-reached state rather than the
            // (semantically meaningless here) pre-loop `locals` - with
            // none at all, `locals` is left as its own pre-loop state, a
            // safe fallback for what's genuinely unreachable code anyway
            // (an infinite loop with no escape).
            let mut states = break_states.into_iter();
            if let Some(first) = states.next() {
                let mut merged = first;
                for state in states {
                    merged = merge_scopes(&self.relation(), &merged, &state);
                }
                *locals = merged;
            }
        } else {
            // The loop may run zero times, so what's true after it
            // is "either the pre-loop state, or whatever one
            // narrowed iteration produces" - not a real fixed
            // point, but sound enough to avoid claiming more
            // precision than we have.
            *locals = merge_scopes(&self.relation(), locals, &body_locals);
            // Reaching code after the loop *without having broken
            // out of it* always means `cond` just evaluated
            // false, regardless of how many iterations ran - real,
            // additional narrowing on top of the merge above,
            // independent of it (e.g. a "loop until no longer
            // null" idiom is fully narrowed non-null afterward).
            // Applied *before* folding in break-reached states
            // below - a `break` doesn't mean `cond` went false, so
            // those states must not be narrowed as if it had.
            self.apply_narrowing(cond, locals, false);
            // Fold in every `break`-reached state too -
            // see `loop_break_states`'s own doc comment on
            // `Checker`.
            for state in break_states {
                *locals = merge_scopes(&self.relation(), locals, &state);
            }
        }
    }

    pub(crate) fn check_do_while(
        &mut self,
        body: &Stmt,
        cond: &Expr,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        // The first pass has nothing to narrow going in - the
        // condition is only evaluated after the body has already run
        // once. Every *re*-convergence pass (`pass > 0`) is different:
        // reaching it at all means the previous pass's trailing `cond`
        // evaluated true (the only way a do-while body runs again), so
        // the "continue" half of `cond`'s narrowing is a real, provable
        // fact at that pass's entry - the same narrowing a `while`
        // loop's convergence closure applies unconditionally on every
        // pass, just shifted here to skip pass 0. Needed for
        // `do { $iterator = $iterator->getIterator(); } while
        // (!$iterator instanceof \Iterator);`: without it, pass 2's
        // entry would widen `$iterator` all the way back to the
        // un-narrowed `Traversable`, and `$iterator->getIterator()`
        // isn't a real `Traversable` method - applying `!$iterator
        // instanceof \Iterator`'s true-branch narrowing subtracts
        // `Iterator` back out first (`Type::without`'s
        // `Traversable`-minus-`Iterator` special case), correctly
        // re-narrowing to `IteratorAggregate` instead.
        let entry = locals.clone();
        self.loop_depth += 1;
        let (body_locals, break_states) = self.converge_loop_body(
            &entry,
            declared_return,
            label,
            // A do-while's body always runs at least once by
            // construction (the condition is only checked *after*) -
            // see `converge_loop_body`'s own doc comment on this
            // parameter.
            LoopZeroIterations::Impossible,
            |this, pass_locals, declared_return, label, pass| {
                if pass > 0 {
                    this.apply_narrowing(cond, pass_locals, true);
                }
                this.check_stmt(body, pass_locals, declared_return, label);
            },
        );
        // A do-while's body always runs at least once - the converged
        // result replaces `locals` directly, no zero-iterations merge
        // needed.
        *locals = body_locals;
        self.in_condition_position = true;
        // `loop_depth` deliberately stays incremented one statement
        // longer than While's arm needs it to: this trailing check of
        // the loop's own condition is *itself* still evaluating a
        // loop-carried variable's post-convergence type, exactly as
        // approximate as any pass inside the loop -
        // `check_constant_numeric_comparison`/`check_constant_loose_
        // comparison`'s own `self.loop_depth > 0` guard needs to see it
        // still incremented here: for `do { ...; $attempts++; } while
        // ((...) && $attempts < 150);`, without it `$attempts`'s
        // convergence-bounded range, nowhere near the real 150, would
        // make `$attempts < 150` look unconditionally, wrongly
        // provable.
        let cond_ty = self.check_expr(cond, locals, label);
        self.check_non_boolean_condition(
            cond,
            &cond_ty,
            "a do-while condition",
            "doWhile.condNotBoolean",
        );
        self.loop_depth -= 1;
        // `DoWhileLoopConstantConditionRule`: real inferred-type
        // trust, same as `If`/`While` above. Both directions are gated
        // on breakless-ness here: `alwaysFalse` skips when the body has
        // a `break` (`do { ...; if ($x) break; ... } while (false);` is
        // a real, common "structured goto" idiom, not dead code);
        // `alwaysTrue` skips on the same `stmt_contains_break` check
        // plus a yield check (no `declared_return: never` exclusion
        // here, unlike `While`'s - real PHPStan's rule doesn't have
        // one).
        if self.inferred_constant_condition(cond, locals) == Some(false) {
            if !stmt_contains_break(body) {
                self.report(
                    "Do-while loop condition is always false.".to_string(),
                    cond.span,
                    4,
                    "doWhile.alwaysFalse",
                );
            }
        } else if self.inferred_constant_condition(cond, locals) == Some(true)
            && !stmt_contains_break(body)
            && !stmt_contains_escaping_exit(body)
            && !body_contains_yield(std::slice::from_ref(body))
        {
            self.report(
                "Do-while loop condition is always true.".to_string(),
                cond.span,
                4,
                "doWhile.alwaysTrue",
            );
        }
        // Same as `While` - reaching code after a do-while always
        // means `cond` just evaluated false. Applied *before*
        // folding in break-reached states below, same reasoning
        // as `While` - a `break` doesn't mean `cond` went false.
        self.apply_narrowing(cond, locals, false);
        // Fold in every `break`-reached state too - see
        // `loop_break_states`'s own doc comment on `Checker`.
        for state in break_states {
            *locals = merge_scopes(&self.relation(), locals, &state);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_for(
        &mut self,
        init: &[Expr],
        cond: &[Expr],
        step: &[Expr],
        body: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        for e in init.iter() {
            self.check_expr(e, locals, label);
        }
        // A real `for`'s condition slot can hold several
        // comma-separated expressions, *all* of which real PHP
        // re-evaluates on every pass through the condition (only the
        // *last* one's truthiness gates the loop - the others are there
        // purely for a side effect, which still happens every
        // iteration). Both halves need the identical treatment `While`
        // gets above: checking any of them once, up front, against the
        // pre-loop entry, would bypass the body's fixed-point
        // convergence entirely, e.g. `for ($stop = false; !$stop && ...;
        // $qi++)`, where `$stop` is reassigned inside the body.
        match cond.last() {
            Some(c) => {
                // Same reasoning as `While`'s arm above: a variable
                // *any* condition expression assigns
                // (`for (;; ($x = f()) !== null; )`, unusual but real)
                // must be unconditionally defined afterward, since a
                // `for`'s condition always runs at least once regardless
                // of body iterations - a quiet, diagnostics-discarded
                // evaluation against the real pre-loop `locals` binds
                // every condition expression's side effect (not just
                // the last one's) before the convergence-aware,
                // diagnostic-bearing walk runs against per-pass
                // copies.
                let mark = self.pass_mark();
                for e in cond.iter() {
                    self.check_expr(e, locals, label);
                }
                self.rewind_to(mark);
                let entry = locals.clone();
                self.loop_depth += 1;
                let in_range_pair = in_range_index_write_loop_vars(c);
                if let Some(pair) = in_range_pair.clone() {
                    self.in_range_index_writes.push(pair);
                }
                let (body_locals, break_states) = self.converge_loop_body(
                    &entry,
                    declared_return,
                    label,
                    LoopZeroIterations::Possible {
                        subject_exempt: None,
                        subject_key: None,
                    },
                    |this, pass_locals, declared_return, label, _pass| {
                        for e in &cond[..cond.len() - 1] {
                            this.check_expr(e, pass_locals, label);
                        }
                        this.check_expr(c, pass_locals, label);
                        this.apply_narrowing(c, pass_locals, true);
                        this.check_stmt(body, pass_locals, declared_return, label);
                        // `step` runs after each body iteration, so
                        // it should see whatever the body just did,
                        // not the pre-loop state - and it runs on
                        // every convergence pass too, since a
                        // variable `step` mutates could itself need
                        // more than one pass to converge.
                        for e in step {
                            this.check_expr(e, pass_locals, label);
                        }
                    },
                );
                if in_range_pair.is_some() {
                    self.in_range_index_writes.pop();
                }
                self.loop_depth -= 1;
                // Same "may run zero times" merge `While` uses, for
                // the same reason - the body isn't treated as if it
                // always executes exactly once.
                *locals = merge_scopes(&self.relation(), locals, &body_locals);
                // Applied *before* folding in break-reached states
                // below, same reasoning as `While` - a `break`
                // doesn't mean `cond` went false.
                self.apply_narrowing(c, locals, false);
                // Fold in every `break`-reached state
                // too - see `loop_break_states`'s own doc comment
                // on `Checker`.
                for state in break_states {
                    *locals = merge_scopes(&self.relation(), locals, &state);
                }
            }
            None => {
                // `for (;;)` - no condition means an unconditional
                // infinite loop (barring an internal break/return/
                // etc.), so the body genuinely always executes at
                // least once - no "may run zero times" merge is
                // needed or correct here. The only way code after
                // this loop is ever reached at all is via a
                // `break` somewhere inside it - folded
                // in additively on top of the direct body-check,
                // rather than trying to model "unreachable if no
                // break exists" precisely.
                let entry = locals.clone();
                self.loop_depth += 1;
                let (body_locals, break_states) = self.converge_loop_body(
                    &entry,
                    declared_return,
                    label,
                    // `for (;;)` genuinely always executes at least
                    // once (no condition to ever prevent the first
                    // iteration) - same exemption as `do`-`while`, see
                    // `converge_loop_body`'s own doc comment.
                    LoopZeroIterations::Impossible,
                    |this, pass_locals, declared_return, label, _pass| {
                        this.check_stmt(body, pass_locals, declared_return, label);
                        for e in step {
                            this.check_expr(e, pass_locals, label);
                        }
                    },
                );
                self.loop_depth -= 1;
                *locals = body_locals;
                for state in break_states {
                    *locals = merge_scopes(&self.relation(), locals, &state);
                }
            }
        }
    }

    /// The `(key, value)` types iterating `ty` gives: an array's own, a
    /// `Traversable`'s template arguments (every `Iterator`, `Generator`
    /// and generic `Traversable` included), each member's joined for a
    /// union; `None` when some member's aren't known.
    ///
    /// `IteratorAggregate` resolves through its own `getIterator()`'s
    /// declared return type first (real, common shape: `getIterator():
    /// Iterator<K, V>` rather than the class itself declaring
    /// `@template-implements Traversable<K,V>`), only falling back to
    /// `IteratorAggregate`'s own template bindings directly when that
    /// return type doesn't resolve further - a small leniency beyond
    /// what real PHPStan's method-return-type-only resolution does,
    /// since the interface's declared generics are still a likely-correct
    /// answer in that case.
    pub(crate) fn iterable_element_type(&self, ty: &Type) -> Option<(Type, Type)> {
        syntax::stack::guard(|| {
            let members = ty.union_members();
            if members.len() > 1 {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for m in members {
                    let (k, v) = self.iterable_element_type(m)?;
                    keys.push(k);
                    values.push(v);
                }
                return Some((
                    keys.into_iter()
                        .reduce(|a, b| self.relation().union(a, b))
                        .unwrap_or(Type::mixed()),
                    values
                        .into_iter()
                        .reduce(|a, b| self.relation().union(a, b))
                        .unwrap_or(Type::mixed()),
                ));
            }
            if let Some(value) = ty.array_value_type() {
                return Some((ty.array_key_type().unwrap_or(Type::mixed()), value));
            }
            let (name, args) = ty.as_object()?;
            if self.registry.is_subclass_of(name, "IteratorAggregate") {
                let agg_bindings =
                    self.registry
                        .class_template_bindings(name, args, "IteratorAggregate");
                if let Some(sig) = self.registry.lookup_method(name, "getIterator") {
                    let declared = self.resolve_pseudo(sig.return_type(), name);
                    // Avoid infinite recursion on a `getIterator(): static`
                    // that resolved right back to this same class.
                    let recurses_into_self =
                        declared.as_object().is_some_and(|(inner, _)| inner == name);
                    if !recurses_into_self
                        && let Some(result) = self.iterable_element_type(&declared)
                        && (agg_bindings.is_empty()
                            || !(result.0.is_mixed() && result.1.is_mixed()))
                    {
                        return Some(result);
                    }
                }
                if !agg_bindings.is_empty() {
                    return Some((
                        agg_bindings.get("TKey").cloned().unwrap_or(Type::mixed()),
                        agg_bindings.get("TValue").cloned().unwrap_or(Type::mixed()),
                    ));
                }
            }
            let bindings = self
                .registry
                .class_template_bindings(name, args, "Traversable");
            if bindings.is_empty() {
                return None;
            }
            Some((
                bindings.get("TKey").cloned().unwrap_or(Type::mixed()),
                bindings.get("TValue").cloned().unwrap_or(Type::mixed()),
            ))
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_foreach(
        &mut self,
        subject: &Expr,
        key: &Option<Expr>,
        value: &Expr,
        by_ref: bool,
        body: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
        inline_var: &[(String, Type)],
    ) {
        // A bare `$variable` subject that the body mutates in place via
        // a nested write keyed by its own loop variable (`foreach
        // ($items as $day => $data) { $items[$day]['derivative'] = ...;
        // }`) must never have its already-transformed shape re-widened
        // back
        // toward the stale pre-loop one - `converge_loop_body`'s own
        // `subject_exempt` keeps exactly this one name out of the
        // "may run zero times" merge every other variable now gets.
        let subject_exempt = match &subject.kind {
            ExprKind::Variable(name) => Some(name.as_ref()),
            _ => None,
        };
        let subject_ty = self.check_expr(subject, locals, label);
        // `DeadForeachRule` - skipped inside an enclosing loop, whose
        // earlier convergence passes can still see a not-yet-grown array.
        if self.loop_depth == 0 && subject_ty.is_empty_array().yes() {
            self.report(
                "Empty array passed to foreach.".to_string(),
                subject.span,
                4,
                "foreach.emptyArray",
            );
        }
        // The loop variable(s) are a definition site, not a read -
        // bound (to the subject's known element type, when it's a
        // homogeneous array) before checking anything that could
        // reference them, at the start of every convergence pass
        // below (see that closure's own comment for why every
        // pass, not just once).
        //
        // An array shape subject must not fall straight to the
        // `Mixed` catch-all, discarding real, known field types, e.g.
        // `foreach ($versions as $key => $value) { $versions[$key] =
        // preg_replace(..., $value); }` where every field of `$versions`
        // is actually a plain `string` - with `$value` inferred as
        // `Mixed`, `preg_replace`'s own stub leaks a broader possibility
        // into `$versions`'s re-widened element type. Joining every
        // field's type (the same idea `write_index_into`'s own
        // dynamic-key write branch uses) is exact when every field
        // shares one type, still sound but coarser when they don't.
        // `Self::array_element_type` also sees through
        // an `Intersection`-wrapped `list<T>`/`non-empty-
        // array<T>` - without it, `foreach` over one of those would
        // silently infer `Mixed` for the loop variable, one of the
        // most common constructs in real PHP.
        // An array-shaped subject keeps taking the array-specific
        // path unchanged; anything that path
        // can't handle (an object) now tries `iterable_element_type`
        // before finally giving up to `Mixed`/`Mixed` - see that
        // function's own doc comment for exactly what it covers
        // (`Traversable`/`Iterator`/`Generator`/`IteratorAggregate`).
        let (key_ty, element_ty) = match subject_ty.array_value_type() {
            Some(elem) => (subject_ty.array_key_type().unwrap_or(Type::mixed()), elem),
            None => match self.iterable_element_type(&subject_ty) {
                Some(pair) => pair,
                None => {
                    // `foreach` takes an array or any object: an object
                    // that isn't `Traversable` iterates its visible
                    // properties, keyed by name. Anything else is
                    // `foreach.nonIterable`, reported, and the body still
                    // checked against `mixed`.
                    let relation = self.relation();
                    let foreachable = |m: &Type| {
                        relation
                            .is_iterable(m)
                            .or(relation.can_access_properties(m))
                    };
                    if let Some(level) = self.failing_level(&subject_ty, 3, |m| foreachable(m).no())
                    {
                        self.report(format!(
                                "Argument of an invalid type {subject_ty} supplied for foreach, only arrays and objects are supported."
                            ),
                            subject.span,
                            level,
                            "foreach.nonIterable",
                        );
                    }
                    let iterates_properties = subject_ty.union_members().iter().all(|m| {
                        relation.can_access_properties(m).yes() && relation.is_iterable(m).no()
                    });
                    if iterates_properties {
                        (Type::string(), Type::mixed())
                    } else {
                        (Type::mixed(), Type::mixed())
                    }
                }
            },
        };
        // A header-level inline `/** @var Type $value */` (or bare
        // `@var Type`, resolved to this exact loop's own value variable
        // by `statement_defined_variable_names`'s own `Foreach` arm)
        // overrides the ordinarily-inferred element type - the same
        // "trust the annotation over the inferred type" precedent every
        // other annotated statement kind already gets, applied here so
        // every convergence pass below binds the loop variable to it,
        // not just whatever `check_stmt`'s own generic preamble happened
        // to insert before this function's own rebinding overwrote it.
        let annotated = |target: &Expr| match &target.kind {
            ExprKind::Variable(name) => inline_var
                .iter()
                .find(|(n, _)| n.as_str() == name.as_ref())
                .map(|(_, ty)| ty.clone()),
            _ => None,
        };
        let element_ty = annotated(value).unwrap_or(element_ty);
        let key_ty = key.as_ref().and_then(annotated).unwrap_or(key_ty);
        // See `converge_loop_body`'s own doc comment: `Foreach` gets
        // the same "may run zero times" merge with the pre-loop state
        // `While`/`For` already had, for every variable *except*
        // `subject_exempt` above - a bare `$variable` subject the body
        // mutates in place via a nested write keyed by its own loop
        // variable keeps its post-body shape untouched, rather than
        // being widened back with the stale pre-loop one.
        // `loop_break_states` captures every `break`-reached state for
        // the loop's exit state, since `always_terminates` only stops a
        // break-ending branch from leaking into the *rest of its own
        // block*.
        //
        // The body converges to a fixed point across up to 3 passes
        // (see `converge_loop_body`) - a variable reassigned inside the
        // body is no longer stuck at its pre-loop type for the whole
        // check. The *entry* state fed to every pass, and the final
        // result, are both still `locals` directly, preserving the
        // zero-iterations trade-off described above.
        //
        // The key/value binding is re-applied at the start of *every*
        // pass, not just once before the first: real PHP rebinds a
        // foreach's loop variable(s) fresh from the subject on every
        // actual iteration, so a pass must never start from what the
        // *previous* pass's body left the loop variable mutated to, e.g.
        // `foreach ($range as $i => &$val) { ...; $val = floatval(...); }`
        // - a by-reference loop variable reassigned inside the body
        // would otherwise wrongly union its post-body `float` into the
        // next pass's entry as `string|float`, even though each real
        // iteration's `&$val` aliases a different array slot that
        // pass's body never touched.
        let entry = locals.clone();
        self.loop_depth += 1;
        // Mirrors real PHPStan's own `ForeachHandler::processStmt`'s
        // third, `isIterableAtLeastOnce()->yes()` branch: a subject
        // that's *provably* non-empty (`explode()`'s own
        // always-non-empty-list return, a non-empty `ArrayShape`
        // literal, ...) never runs zero times, so it gets the same "no
        // merge with the pre-loop entry" treatment `Impossible` already
        // gives do-while/for(;;), not `Possible`'s merge (which would
        // otherwise widen back toward the stale pre-loop state even
        // though every real iteration provably replaces it), e.g.
        // `foreach (explode(',', $order) as $rawEntry)`, where
        // `explode()` on a provably non-empty `$order` is itself
        // already a real `non-empty-list<string>`.
        let zero_iterations = if subject_ty.is_non_empty_array().yes() {
            LoopZeroIterations::Impossible
        } else {
            LoopZeroIterations::Possible {
                subject_exempt,
                subject_key: match key {
                    Some(Expr {
                        kind: ExprKind::Variable(name),
                        ..
                    }) if !by_ref => Some(name.as_ref()),
                    _ => None,
                },
            }
        };
        // Captured from *inside* the last accepted pass, before
        // `converge_loop_body`'s own "may not run" merge (which drops
        // any name, `value`'s included, not present in both the
        // pre-loop `entry` and the body's exit state) can erase it.
        // `converge_loop_body` only calls `run_pass` again when
        // convergence hasn't been reached yet, so this always ends up
        // holding the value from whichever pass was actually accepted.
        let mut last_value_ty: Option<Type> = None;
        // Whether that pass can `break` out, leaving later elements as
        // they were.
        let mut last_pass_breaks = true;
        // Variables that pass wrote at the loop's own key on every way
        // through the body (`$acc[$key] = …`).
        let mut last_written_at_key: Vec<String> = Vec::new();
        // See `ContainerFieldFactsMap`'s own doc comment (`checker.rs`) -
        // harvested from whichever pass is ultimately accepted, the same
        // way `last_value_ty` above is, and for the identical soundness
        // reason: a field write that only happened on *some* iterations
        // already wouldn't survive that iteration's own internal
        // `merge_scopes` join, so it's correctly absent from `pass_locals`
        // by the time this reads it.
        let mut last_field_facts: FxHashMap<String, Type> = FxHashMap::default();
        // Iterating anything but an array runs user code (a generator, an
        // `Iterator`'s methods) before each iteration and after the last.
        let iteration_throws = subject_ty.array_value_type().is_none();
        let (body_locals, break_states) = self.converge_loop_body(
            &entry,
            declared_return,
            label,
            zero_iterations,
            |this, pass_locals, declared_return, label, _pass| {
                if iteration_throws {
                    this.record_throw_state(
                        Type::object("Throwable".into(), Vec::new()),
                        pass_locals,
                    );
                }
                if let Some(k) = key {
                    this.bind_simple_target(k, key_ty.clone(), pass_locals);
                    this.check_expr(k, pass_locals, label);
                }
                // A foreach value can itself be a destructuring
                // target (`foreach ($pairs as [$num, $letter])`) -
                // route it through the same fully-recursive
                // helpers a plain destructuring assignment uses,
                // not the bare-variable-only `bind_simple_target`.
                // `check_assignment_target` already covers "check
                // as a read where appropriate" for whatever isn't
                // a pure definition site, so no separate
                // `check_expr` call is needed on top of it.
                this.check_assignment_target(value, pass_locals, label);
                // Same by-ref-parameter check a plain assignment already
                // gets (`ExprKind::Assign`'s own call site, `checker/
                // expr.rs`) - a `foreach` value binding is just as real a
                // per-iteration rebind as any other assignment, so a
                // value variable that happens to name one of this
                // function's own by-ref parameters (or destructures one
                // out) is checked against its declared type here too.
                this.check_param_out_assignment(value, &element_ty);
                this.check_by_ref_destructuring(value, &element_ty);
                this.bind_assignment_target(value, element_ty.clone(), pass_locals);
                // Eager propagation: `value` was just bound from
                // `subject`'s own (dynamic-keyed) element - if `subject`
                // already carries per-field facts from an earlier by-ref
                // `foreach` over it, copy them onto `value`'s own
                // `narrowing_key` synthetic keys right now, so the
                // pre-existing "check the narrowed key first" Index-read
                // fast path (`checker/expr.rs`) sees them for any read of
                // `value[literalField]` inside this same loop body -
                // covers both a by-ref and an ordinary `foreach` binding
                // uniformly, not just the by-ref-feedback case below.
                if let ExprKind::Variable(value_name) = &value.kind
                    && let ExprKind::Variable(subject_name) = &subject.kind
                {
                    propagate_container_field_facts(pass_locals, value_name, subject_name);
                }
                this.check_stmt(body, pass_locals, declared_return, label);
                if let Some(Expr {
                    kind: ExprKind::Variable(key_name),
                    ..
                }) = key
                {
                    let suffix = format!("[${key_name}]");
                    let continues = this.loop_continue_states.last();
                    last_written_at_key = pass_locals
                        .iter()
                        .filter_map(|(k, _)| k.strip_suffix(suffix.as_str()))
                        .filter(|base| base.chars().all(|c| c.is_alphanumeric() || c == '_'))
                        .filter(|base| {
                            let marker = format!("{base}{suffix}");
                            continues
                                .into_iter()
                                .flatten()
                                .all(|state| state.contains_key(&marker))
                        })
                        .map(str::to_string)
                        .collect();
                    last_pass_breaks = this
                        .loop_break_states
                        .last()
                        .is_none_or(|states| !states.is_empty());
                }
                if let ExprKind::Variable(value_name) = &value.kind {
                    // The value as each way through the body leaves it: its
                    // end, and every `continue`.
                    last_value_ty = std::iter::once(pass_locals.get(value_name).cloned())
                        .chain(
                            this.loop_continue_states
                                .last()
                                .into_iter()
                                .flatten()
                                .map(|state| state.get(value_name).cloned()),
                        )
                        .collect::<Option<Vec<Type>>>()
                        .map(|m| this.relation().union_of(m));
                    last_pass_breaks = this
                        .loop_break_states
                        .last()
                        .is_none_or(|states| !states.is_empty());
                    let prefix = format!("{value_name}[");
                    last_field_facts = pass_locals
                        .iter()
                        .filter_map(|(k, v)| {
                            k.strip_prefix(&prefix)
                                .and_then(|rest| rest.strip_suffix(']'))
                                .map(|field| (field.to_string(), v.clone()))
                        })
                        .collect();
                }
            },
        );
        self.loop_depth -= 1;
        *locals = body_locals;
        for state in break_states {
            *locals = merge_scopes(&self.relation(), locals, &state);
        }
        // A loop over a sealed shape that writes a container at its own key
        // on every way through the body, and can't `break`, has written it
        // at each key the shape surely has.
        if !last_pass_breaks
            && let Some(subject_shape) = subject_ty.as_array_shape()
            && subject_shape.is_sealed()
        {
            let subject_name = match &subject.kind {
                ExprKind::Variable(name) => Some(name.as_ref()),
                _ => None,
            };
            let keys: Vec<types::ShapeKey> = subject_shape
                .fields()
                .iter()
                .filter(|f| !f.optional)
                .map(|f| f.key.clone())
                .collect();
            for name in &last_written_at_key {
                if Some(name.as_str()) == subject_name {
                    continue;
                }
                let Some(current) = locals.get(name).cloned() else {
                    continue;
                };
                let written = keys
                    .iter()
                    .fold(current, |ty, key| ty.with_key_presence(key, true));
                if !written.is_never() {
                    locals.insert(name.clone(), written);
                }
            }
        }
        // A by-ref value variable (`foreach ($items as &$item) { $item
        // ['x'] = ...; }`) aliases into `subject`'s own underlying
        // storage - a key/field the body writes into `$item` is really
        // being written into whatever slot of `$subject` that iteration
        // is pointing at, so it belongs in `$subject`'s tracked element
        // type for code reached after the loop, not just `$item`'s own
        // (otherwise discarded) type. Every element is visited, so each
        // ends as the body leaves `$item`; a loop that can `break` leaves
        // the elements after it as they were.
        if by_ref
            && let ExprKind::Variable(subject_name) = &subject.kind
            && let Some(old_element_ty) = subject_ty.array_value_type()
            && let Some(final_value_ty) = last_value_ty
            && let Some(current_subject_ty) = locals.get(subject_name).cloned()
        {
            let widened_element_ty = if last_pass_breaks {
                self.relation().union(old_element_ty, final_value_ty)
            } else {
                final_value_ty
            };
            // See `ContainerFieldFactsMap`'s own doc comment - records
            // (or, for a second by-ref `foreach` over the same
            // container, overwrites - a disclosed, non-unsound
            // completeness limitation, not a correctness one) every
            // per-field fact this loop's own body proved unconditionally
            // true of every element, keyed by the container's own name
            // so a later read through a *differently-named* variable
            // bound from it (a plain `$x = $container[$dyn];`, or a
            // later, non-by-ref `foreach`) can still find it.
            locals
                .container_field_facts_mut()
                .insert(subject_name.to_string(), Rc::new(last_field_facts));
            locals.insert(
                subject_name.to_string(),
                current_subject_ty.with_general_array_value(&widened_element_ty),
            );
        }
    }
}
