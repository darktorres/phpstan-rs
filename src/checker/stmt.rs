use crate::ast::*;
use crate::checker::{Checker, Locals, merge_scopes, purge_stale_narrowing_for};
use crate::docblock;
use crate::types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_block(
        &mut self,
        stmts: &[Stmt],
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        // Phase 15 item (1), `UnreachableStatementRule`: investigated and
        // declined - see ROADMAP.md. Implemented, corpus-verified, and
        // reverted: wiring `always_terminates` into a `deadCode.
        // unreachable` diagnostic here surfaced a genuine, deep, pre-
        // existing gap in the checker's own narrowing - a variable
        // reassigned inside a loop keeps its pre-loop-entry type for the
        // *entire* single-pass body check (no fixed-point iteration
        // across loop bodies), so a later `if ($x !== null)` guard on a
        // variable the loop itself reassigns away from `null` can
        // wrongly narrow to `Type::Never`, which this check then
        // (incorrectly) read as "this statement always terminates."
        // Fixing that belongs to Phase 55's own tracked narrowing-engine
        // generalization (real fixed-point loop analysis), not a quick
        // patch here. Two narrower, genuinely-safe bugs this same
        // verification pass *did* find and fix outright (independent of
        // whether this diagnostic itself ships): `widen_coalesce_result`
        // wrongly inferring `$x ?? throw ...;` as `Type::Never` for the
        // whole expression, and `always_terminates`'s own `Switch` arm
        // wrongly treating a trailing, entirely-empty `case`/`default`
        // group as vacuously terminating.
        for stmt in stmts {
            self.check_stmt(stmt, locals, declared_return, label);
        }
    }

    #[hotpath::measure]
    pub(crate) fn check_stmt(
        &mut self,
        stmt: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        let inline_var = self.inline_var_annotation(stmt);
        if let Some((name, ty)) = &inline_var {
            purge_stale_narrowing_for(locals, name);
            locals.insert(name.clone(), ty.clone());
        }
        match &stmt.kind {
            StmtKind::Expression(e) => {
                self.in_bare_statement_position = true;
                self.check_expr(e, locals, label);
                self.check_no_discard_statement(e, locals, label);
                // Phase 41 stage 1: NoopRule's own supported slice - see
                // `checker::noop`'s own module doc comment.
                self.check_noop_statement(e, label);
                // `assert($cond)` is a real, common narrowing idiom real
                // PHPStan explicitly documents support for - if execution
                // continues past this statement, `$cond` held, so it's
                // narrowed the same way a terminating `if`-guard clause's
                // surviving branch already is, just without any
                // branch/merge machinery since there's only one path
                // forward (assert either holds or the program aborts).
                // Deliberately not `instanceof`-specific: apply_narrowing
                // already handles whatever condition shapes it
                // understands and no-ops harmlessly on the rest.
                if let ExprKind::Call(callee, args) = &e.kind
                    && matches!(&callee.kind, ExprKind::Name(name) if name == "assert")
                    && let Some(cond_arg) = args.first()
                {
                    self.apply_narrowing(&cond_arg.value, locals, true);
                }
                // Phase 27: PHPUnit's own narrowing-relevant assertions
                // (`self::assertIsString($x)`, `$this->assertNotNull($x)`)
                // - the exact same "statement narrows unconditionally"
                // shape as `assert($cond)` above, just per-assertion-name
                // rather than a generic boolean condition.
                match &e.kind {
                    ExprKind::MethodCall(_, method, args, _)
                    | ExprKind::StaticCall(_, method, args) => {
                        self.apply_phpunit_assertion_narrowing(method, args, locals);
                    }
                    _ => {}
                }
                // A bare `@phpstan-assert Type $target` on this call's
                // own declaration - the same "narrows unconditionally,
                // no branch needed" shape `assert($cond)`/PHPUnit's own
                // assertions get just above, generalized via a real
                // `FunctionSig` lookup instead of a hardcoded name table
                // (see `FunctionSig::assert_if_true`'s own doc comment).
                match &e.kind {
                    ExprKind::MethodCall(recv, method, args, _) => {
                        if let Type::Object(class, _) = self.infer_expr(recv, locals)
                            && let Some(sig) = self.registry.lookup_method(&class, method)
                            && !sig.assert.is_empty()
                        {
                            self.apply_assert_infos(&sig.assert, sig, Some(recv), args, locals);
                        }
                    }
                    ExprKind::StaticCall(class_name, method, args) => {
                        if let Some(class) = self.resolve_static_class_name(class_name)
                            && let Some(sig) = self.registry.lookup_method(&class, method)
                            && !sig.assert.is_empty()
                        {
                            self.apply_assert_infos(&sig.assert, sig, None, args, locals);
                        }
                    }
                    ExprKind::Call(callee, args) => {
                        if let ExprKind::Name(name) = &callee.kind {
                            let (primary, fallback) =
                                self.name_ctx.resolve_function_candidates(name);
                            if let Some(sig) =
                                self.registry.lookup_function(&primary).or_else(|| {
                                    fallback
                                        .as_deref()
                                        .and_then(|g| self.registry.lookup_function(g))
                                })
                                && !sig.assert.is_empty()
                            {
                                self.apply_assert_infos(&sig.assert, sig, None, args, locals);
                            }
                        }
                    }
                    _ => {}
                }
                // Phase 32 Finding K: a bare `$cond || neverReturningCall();`
                // statement - PHP's classic "guard or die" idiom
                // (`$pos !== false || $this->ierror('Missing .', 400);`).
                // `ierror(): never` is a real `never` return type; if
                // execution reaches the statement *after* this one, the
                // `||`'s right side didn't run, so (by short-circuit
                // evaluation) the left side was already true - the same
                // "statement narrows unconditionally" reasoning as
                // `assert($cond)` above, just for a condition spelled as
                // an `Or` instead of a direct call. `Or`-only, not `And` -
                // `A && B;` gives no equivalent guarantee (if `A` is
                // false, `B` never runs and the statement simply ends,
                // without proving `A` either way).
                if let ExprKind::Binary(BinOp::Or, lhs, rhs) = &e.kind
                    && self.infer_expr(rhs, locals) == Type::Never
                {
                    self.apply_narrowing(lhs, locals, true);
                }
            }
            StmtKind::Return(Some(e)) => {
                let actual = self.check_expr(e, locals, label);
                if let Some(expected) = declared_return {
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    // Phase 61: a `never`-typed function/method reaching
                    // *any* `return` at all - value or bare - is its own
                    // dedicated real-PHPStan identifier/message
                    // (`return.never`, "should never return but return
                    // statement found"), not an ordinary type mismatch -
                    // `never` promises the caller execution can't
                    // continue past this call, a stronger claim than
                    // "returns the wrong type." Checked before the
                    // generic path below, which would otherwise report
                    // this identically to any other type mismatch.
                    if expected == Type::Never {
                        self.report(
                            label.to_string(),
                            "should never return, but a return statement was reached".to_string(),
                            e.span,
                            3,
                            "return.never",
                        );
                    } else if expected == Type::Void {
                        // Phase 61: same idea, one position over - a
                        // `void` function returning an actual value is
                        // real PHPStan's own dedicated `return.void`,
                        // not the generic mismatch path (`void` isn't a
                        // "wrong type" the way e.g. `string` vs `int`
                        // is - it's "no value was ever supposed to come
                        // back").
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected void, got {actual}"),
                            e.span,
                            3,
                            "return.void",
                        );
                    } else if actual != Type::Mixed
                        && !self.registry.is_assignable(&actual, &expected)
                    {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got {actual}"),
                            e.span,
                            3,
                            "return.type",
                        );
                    }
                }
            }
            // A bare `return;` always returns `null` in real PHP,
            // regardless of the function's declared return type - a
            // genuine mismatch inside a function declared to return a
            // concrete, non-nullable, non-`void` type (the same easy
            // slip as forgetting to add a return value after an
            // early-exit guard clause).
            StmtKind::Return(None) => {
                // A bare `return;` inside a generator (Finding 3) sets
                // Generator::getReturn()'s own value, not this function's
                // return value - real PHPStan never checks it against the
                // declared Generator/Iterator/Traversable return type.
                if let Some(expected) = declared_return
                    && !self.current_function_is_generator
                {
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    // Phase 61: see the identical `return.never` special
                    // case on `Return(Some(e))` just above - a bare
                    // `return;` reaches this exact "never returns" claim
                    // just as surely as one with a value does.
                    if expected == Type::Never {
                        self.report(
                            label.to_string(),
                            "should never return, but a return statement was reached".to_string(),
                            stmt.span,
                            3,
                            "return.never",
                        );
                    } else if expected != Type::Void
                        && expected != Type::Mixed
                        && !self.registry.is_assignable(&Type::Null, &expected)
                    {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got null"),
                            stmt.span,
                            3,
                            "return.type",
                        );
                    }
                }
            }
            StmtKind::Echo(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    let ty = self.check_expr(e, locals, label);
                    self.check_echo_validity(&ty, i, e.span, label);
                    // Phase 67/Phase 39 follow-up: `echo` implicitly
                    // invokes `__toString()` on an object argument - see
                    // `check_implicit_tostring_usage`'s own doc comment.
                    self.check_implicit_tostring_usage(&ty, e.span, label);
                }
            }
            StmtKind::Block(stmts) => self.check_block(stmts, locals, declared_return, label),
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(cond, locals, label);
                // Phase 15 item (2), `IfConstantConditionRule`: investigated
                // and declined - see ROADMAP.md. Implemented and corpus-
                // verified as "the condition's own inferred type is exactly
                // `Type::True`/`Type::False`", then reverted: real corpus
                // testing found this project's own narrowing has two
                // separate, pervasive gaps that each independently make a
                // plain variable's type wrongly collapse to a literal
                // `True`/`False` long after it should have widened back to
                // `bool` - a `foreach`-scoped "first iteration" flag
                // (`$first = true; foreach (...) { if ($first) { $first =
                // false; } else { ... } }`, one of the most common PHP
                // idioms there is) never gets fixed-point-merged across
                // loop iterations, and `$x = $expr ?? false;` collapses to
                // the bare literal `false` whenever `$expr`'s own type
                // isn't confidently known, discarding the "or whatever
                // `$expr` actually was" half entirely. Both are real,
                // pre-existing gaps this check merely exposed, not new
                // bugs - fixing them belongs to Phase 55's own tracked
                // narrowing-engine generalization, not a quick patch here.

                let mut then_locals = locals.clone();
                self.apply_narrowing(cond, &mut then_locals, true);
                self.check_stmt(then_branch, &mut then_locals, declared_return, label);

                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                if let Some(e) = else_branch {
                    self.check_stmt(e, &mut else_locals, declared_return, label);
                }

                // The classic guard clause - `if ($x === null) { return; }`
                // - depends on this: a branch that unconditionally
                // returns/throws/breaks/continues never reaches the merge
                // point, so only the surviving branch's (narrowed) scope
                // should propagate past the if. Without this, the
                // terminated branch's narrowing (here, "$x is null")
                // would incorrectly leak into the code after the guard.
                let then_terminates = self.always_terminates(then_branch, &then_locals);
                let else_terminates = else_branch
                    .as_deref()
                    .is_some_and(|e| self.always_terminates(e, &else_locals));

                *locals = match (then_terminates, else_terminates) {
                    (true, false) => else_locals,
                    (false, true) => then_locals,
                    _ => merge_scopes(&then_locals, &else_locals),
                };
            }
            StmtKind::While { cond, body } => {
                self.check_expr(cond, locals, label);
                // Phase 15 item (2), `WhileLoopAlwaysFalseConditionRule`/
                // `WhileLoopAlwaysTrueConditionRule`: investigated and
                // declined alongside `StmtKind::If`'s own identical
                // check - see that arm's doc comment and ROADMAP.md for
                // the real, corpus-found narrowing gaps that make this
                // unsafe to ship.
                let mut body_locals = locals.clone();
                self.apply_narrowing(cond, &mut body_locals, true);
                self.loop_break_states.push(Vec::new());
                self.loop_depth += 1;
                self.check_stmt(body, &mut body_locals, declared_return, label);
                self.loop_depth -= 1;
                let break_states = self.loop_break_states.pop().unwrap_or_default();
                // The loop may run zero times, so what's true after it is
                // "either the pre-loop state, or whatever one narrowed
                // iteration produces" - not a real fixed point, but sound
                // enough to avoid claiming more precision than we have.
                *locals = merge_scopes(locals, &body_locals);
                // Reaching code after the loop *without having broken out
                // of it* always means `cond` just evaluated false,
                // regardless of how many iterations ran - real,
                // additional narrowing on top of the merge above,
                // independent of it (e.g. a "loop until no longer null"
                // idiom is fully narrowed non-null afterward). Applied
                // *before* folding in break-reached states below - a
                // `break` doesn't mean `cond` went false, so those states
                // must not be narrowed as if it had.
                self.apply_narrowing(cond, locals, false);
                // Phase 29: fold in every `break`-reached state too - see
                // `loop_break_states`'s own doc comment on `Checker`.
                for state in break_states {
                    *locals = merge_scopes(locals, &state);
                }
            }
            StmtKind::DoWhile { body, cond } => {
                // The condition is only evaluated after the body has
                // already run once, so there's nothing to narrow going in.
                self.loop_break_states.push(Vec::new());
                self.loop_depth += 1;
                self.check_stmt(body, locals, declared_return, label);
                self.loop_depth -= 1;
                let break_states = self.loop_break_states.pop().unwrap_or_default();
                self.check_expr(cond, locals, label);
                // Same as `While` - reaching code after a do-while always
                // means `cond` just evaluated false. Applied *before*
                // folding in break-reached states below, same reasoning
                // as `While` - a `break` doesn't mean `cond` went false.
                self.apply_narrowing(cond, locals, false);
                // Phase 29: fold in every `break`-reached state too - see
                // `loop_break_states`'s own doc comment on `Checker`.
                for state in break_states {
                    *locals = merge_scopes(locals, &state);
                }
            }
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                for e in init.iter().chain(cond.iter()) {
                    self.check_expr(e, locals, label);
                }
                // A real `for`'s condition slot can hold several
                // comma-separated expressions - only the *last* one's
                // truthiness actually gates the loop, matching PHP's own
                // semantics (the others are only there for a side effect).
                match cond.last() {
                    Some(c) => {
                        let mut body_locals = locals.clone();
                        self.apply_narrowing(c, &mut body_locals, true);
                        self.loop_break_states.push(Vec::new());
                        self.loop_depth += 1;
                        self.check_stmt(body, &mut body_locals, declared_return, label);
                        self.loop_depth -= 1;
                        let break_states = self.loop_break_states.pop().unwrap_or_default();
                        // `step` runs after each body iteration, so it
                        // should see whatever the body just did, not the
                        // pre-loop state.
                        for e in step {
                            self.check_expr(e, &mut body_locals, label);
                        }
                        // Same "may run zero times" merge `While` uses,
                        // for the same reason - unlike before this fix,
                        // this is no longer treated as if the body always
                        // executes exactly once.
                        *locals = merge_scopes(locals, &body_locals);
                        // Applied *before* folding in break-reached states
                        // below, same reasoning as `While` - a `break`
                        // doesn't mean `cond` went false.
                        self.apply_narrowing(c, locals, false);
                        // Phase 29: fold in every `break`-reached state
                        // too - see `loop_break_states`'s own doc comment
                        // on `Checker`.
                        for state in break_states {
                            *locals = merge_scopes(locals, &state);
                        }
                    }
                    None => {
                        // `for (;;)` - no condition means an unconditional
                        // infinite loop (barring an internal break/return/
                        // etc.), so the body genuinely always executes at
                        // least once - no "may run zero times" merge is
                        // needed or correct here. The only way code after
                        // this loop is ever reached at all is via a
                        // `break` somewhere inside it (Phase 29) - folded
                        // in additively on top of the direct body-check,
                        // rather than trying to model "unreachable if no
                        // break exists" precisely.
                        self.loop_break_states.push(Vec::new());
                        self.loop_depth += 1;
                        self.check_stmt(body, locals, declared_return, label);
                        self.loop_depth -= 1;
                        let break_states = self.loop_break_states.pop().unwrap_or_default();
                        for e in step {
                            self.check_expr(e, locals, label);
                        }
                        for state in break_states {
                            *locals = merge_scopes(locals, &state);
                        }
                    }
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                body,
                ..
            } => {
                let subject_ty = self.check_expr(subject, locals, label);
                // The loop variable(s) are a definition site, not a read -
                // bind them (to the subject's known element type, when
                // it's a homogeneous array) before checking anything that
                // could reference them.
                //
                // Phase 12 (unplanned follow-on): a `Type::ArrayShape`
                // subject used to fall straight to the `Mixed` catch-all,
                // discarding real, known field types - a real corpus false
                // positive this surfaced: `MaintenanceActionDispatcher::
                // dispatch`'s `foreach ($versions as $key => $value) {
                // $versions[$key] = preg_replace(..., $value); }`, where
                // every field of `$versions` is actually a plain `string`.
                // With `$value` inferred as `Mixed`, `preg_replace`'s own
                // stub (which returns a broader type for a
                // statically-unknown subject) leaked an `array<string>`
                // possibility into `$versions`'s own re-widened element
                // type that stuck around once Finding E's `Union`-awareness
                // stopped silently collapsing it. Joining every field's own
                // type (the same "give the reader every field's type is at
                // least possible" idea `write_index_into`'s own dynamic-key
                // write branch already uses) is real, if narrow, precision:
                // exact when every field shares one type (the common case
                // this false positive came from), still sound but coarser
                // when they don't.
                // Phase 14: `Self::array_element_type` also sees through
                // an `Intersection`-wrapped `list<T>`/`non-empty-
                // array<T>` - without it, `foreach` over one of those
                // silently inferred `Mixed` for the loop variable, a
                // real, confirmed regression (one of the most common
                // constructs in real PHP).
                let element_ty = Self::array_element_type(&subject_ty).unwrap_or(Type::Mixed);
                if let Some(k) = key {
                    self.bind_simple_target(k, Type::Mixed, locals);
                    self.check_expr(k, locals, label);
                }
                // A foreach value can itself be a destructuring target
                // (`foreach ($pairs as [$num, $letter])`, a real, common
                // way to iterate a list of tuples) - route it through the
                // same fully-recursive helpers a plain destructuring
                // assignment uses, not the bare-variable-only
                // bind_simple_target (which silently bound nothing at all
                // for anything else). check_assignment_target already
                // covers "check as a read where appropriate" for whatever
                // isn't a pure definition site (an Index's key, a
                // PropertyAccess's base, ...), so there's no separate
                // check_expr call needed on top of it.
                self.check_assignment_target(value, locals, label);
                self.bind_assignment_target(value, element_ty, locals);
                // Phase 29: `Foreach` still checks its body once directly
                // against the real `locals` (deliberately *not* also
                // adopting `While`/`For`'s own separate "may run zero
                // times" merge with the pre-loop state here - tried
                // during this fix and reverted: a common, real idiom
                // mutates the very same array a `foreach` iterates via a
                // nested write keyed by the loop variable itself
                // (`foreach ($items as $day => $data) { ...;
                // $items[$day]['derivative'] = ...; }`, confirmed in
                // `CalendarMonthly::buildMonthCalendar`) - unioning in the
                // *pre-loop* shape as an "also possible" alternative there
                // is actively wrong, since every key iterated over really
                // does get the loop's own treatment, never the stale
                // pre-loop one). What *was* missing, unconditionally: a
                // `break` reached anywhere in `body` had its own state
                // silently discarded from everything past the loop -
                // `always_terminates` correctly stops a break-ending
                // branch from leaking into the *rest of its own block*,
                // but nothing captured it for the loop's own exit state.
                // `loop_break_states` closes exactly that gap, without
                // touching the single-representative-pass model otherwise.
                self.loop_break_states.push(Vec::new());
                self.loop_depth += 1;
                self.check_stmt(body, locals, declared_return, label);
                self.loop_depth -= 1;
                let break_states = self.loop_break_states.pop().unwrap_or_default();
                for state in break_states {
                    *locals = merge_scopes(locals, &state);
                }
            }
            StmtKind::Switch { subject, cases } => {
                self.check_expr(subject, locals, label);
                // Two independent facts are tracked while walking cases
                // in order:
                //
                // `narrowed_locals` accumulates "subject != case_i.test"
                // for every case with a test, unconditionally - this is
                // purely a statement about the dispatch comparison chain
                // (direct dispatch past case_i requires its value not to
                // have matched) and holds regardless of whether case_i's
                // own body terminates or falls through.
                //
                // `running`/`*locals` tracks what a case reached via real
                // fallthrough would see - only meaningful when the
                // *immediately preceding* case is known not to terminate,
                // tracked by `previous_terminates` (starts `true`: the
                // first case has no predecessor to fall through from, so
                // it's always reached via direct dispatch only).
                //
                // A case reached only by direct dispatch (previous case
                // terminates, or this is the first case) is checked
                // against `narrowed_locals` plus its own positive
                // "subject === test" narrowing. A case that might also be
                // reached via fallthrough is checked against the plain
                // fallthrough-inherited `running` state instead, without
                // extra narrowing - conservative (it doesn't claim the
                // precision of the direct-dispatch-only path when
                // fallthrough is also possible), matching this project's
                // general preference for missing precision over a false
                // positive.
                // A third thing tracked alongside the two above:
                // `exit_states` collects every case's own ending state
                // that legitimately reaches the code *after* the whole
                // switch - either it ends in `break` (exits the switch,
                // not the enclosing function/loop), or it's the very last
                // case and simply falls off its own end with no explicit
                // exit at all. A case ending in `return`/`throw`/
                // `continue` never contributes here, since control never
                // reaches past the switch on that path. If there's no
                // `default:`, the subject might also match nothing at
                // all, so the original pre-switch state is its own valid
                // exit path too.
                let mut running = locals.clone();
                let mut narrowed_locals = locals.clone();
                let mut previous_terminates = true;
                let mut exit_states: Vec<Locals> = Vec::new();
                if !cases.iter().any(|c| c.test.is_none()) {
                    exit_states.push(locals.clone());
                }
                let case_count = cases.len();
                // Phase 29: a bare `break;` written directly inside a
                // case targets *this* switch, not any enclosing loop -
                // pushing a frame here (discarded below, never read) only
                // exists to intercept it, since `loop_break_states`'s
                // depth-counting would otherwise attribute it to whichever
                // loop happens to be open around this switch. This
                // switch's own `exit_states` mechanism above already
                // handles `break` completely on its own.
                self.loop_break_states.push(Vec::new());
                for (i, case) in cases.iter().enumerate() {
                    if let Some(t) = &case.test {
                        self.check_expr(t, locals, label);
                    }
                    let mut case_locals = if previous_terminates {
                        let mut nl = narrowed_locals.clone();
                        if let Some(t) = &case.test {
                            self.apply_equality_narrowing(subject, t, &mut nl, true);
                        }
                        nl
                    } else {
                        running.clone()
                    };
                    self.check_block(&case.body, &mut case_locals, declared_return, label);
                    let last_stmt = case.body.last();
                    let case_terminates =
                        last_stmt.is_some_and(|s| self.always_terminates(s, &case_locals));
                    if let Some(t) = &case.test {
                        self.apply_equality_narrowing(subject, t, &mut narrowed_locals, false);
                    }
                    let is_last = i + 1 == case_count;
                    let reaches_after_switch = match last_stmt {
                        Some(s) if matches!(s.kind, StmtKind::Break(_)) => true,
                        Some(s) => is_last && !self.exits_function_or_loop(s, &case_locals),
                        None => is_last,
                    };
                    if reaches_after_switch {
                        exit_states.push(case_locals.clone());
                    }
                    if !case_terminates {
                        running = case_locals;
                    }
                    previous_terminates = case_terminates;
                }
                self.loop_break_states.pop();
                // If no path reaches past the switch (every case
                // return/throw/continues, and a `default:` exists), the
                // code after it is dead - leave `locals` alone rather
                // than guess, the same shape `always_terminates`'s own
                // `Switch` arm independently recognizes.
                if let Some(first) = exit_states.first().cloned() {
                    *locals = exit_states
                        .into_iter()
                        .skip(1)
                        .fold(first, |acc, s| merge_scopes(&acc, &s));
                }
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.check_block(body, locals, declared_return, label);
                let try_end_locals = locals.clone();
                let try_terminates = body
                    .last()
                    .is_some_and(|s| self.always_terminates(s, &try_end_locals));
                // Every reachable state after the whole try/catch - the
                // try body's own full-completion state, plus each catch
                // clause's own ending state (a variable a catch clause
                // assigns, like `$body = ...;`, previously vanished
                // entirely here: `locals` kept only the try body's own
                // state, discarding every catch's, matching every other
                // control-flow join this checker already does for
                // if/else - a branch that always terminates is excluded
                // from the merge exactly the same way there too).
                let mut end_states: Vec<Locals> = if try_terminates {
                    Vec::new()
                } else {
                    vec![try_end_locals]
                };
                // Phase 42's own next-recommended step:
                // `CatchWithUnthrownExceptionRule`'s "already caught"
                // half (`catch.alreadyCaught`) - a real, unconditional
                // PHP-level dead-code shape (PHP tries each `catch`
                // clause in source order and stops at the first match,
                // so a later clause every one of whose types is already
                // a subtype of something an *earlier* clause caught can
                // never actually run) that needs no throw-type tracking
                // at all, only the registry's existing `is_subclass_of`
                // - unlike this same rule's own "never thrown in the try
                // block" half, which needs a real per-callee throw-type
                // model this project doesn't have yet (still open).
                let mut previously_caught: Vec<Type> = Vec::new();
                for c in catches {
                    // Scoped to this catch block only, in its own clone
                    // rather than the shared `locals` - matches real PHP
                    // semantics (the caught variable is unset at the end of
                    // its catch block) and keeps one catch's binding from
                    // leaking into a sibling catch or the code after the
                    // whole try.
                    // Finding 57: every type in a multi-type catch must
                    // implement Throwable, independently of whatever
                    // Finding 27's own union-binding does with them - a
                    // real, unconditional PHP fatal error at
                    // declaration time otherwise ("Cannot catch X
                    // because it does not implement Throwable").
                    for t in &c.types {
                        if let Type::Object(class, _) = self.resolve_name_expr(t) {
                            // Phase 48: `CaughtExceptionExistenceRule`'s own
                            // existence half, checked first - a typo'd/
                            // missing exception class previously fell
                            // straight through to `is_subclass_of`, which
                            // silently treats a name the registry has no
                            // entry for as "not a subclass of Throwable"
                            // (indistinguishable from a real, existing,
                            // non-Throwable class) and reported the
                            // confusingly wrong "does not implement
                            // Throwable" - `else if` here reports at most
                            // one diagnostic per catch type instead of
                            // both.
                            if !self.class_name_exists(&class) {
                                self.report(
                                    label.to_string(),
                                    format!("caught class {class} not found"),
                                    c.span,
                                    0,
                                    // Real PHPStan's own
                                    // `CaughtExceptionExistenceRule` reuses
                                    // the generic `class.notFound`
                                    // identifier here too (confirmed
                                    // against `vendor/phpstan-src`) -
                                    // `catch.notThrowable` just below is
                                    // the one genuinely catch-specific
                                    // identifier this rule has.
                                    "class.notFound",
                                );
                            } else if !self.registry.is_subclass_of(&class, "Throwable") {
                                self.report(
                                    label.to_string(),
                                    format!(
                                        "cannot catch {class}: it does not implement Throwable"
                                    ),
                                    c.span,
                                    0,
                                    "catch.notThrowable",
                                );
                            }
                        }
                    }
                    let clause_types: Vec<Type> =
                        c.types.iter().map(|t| self.resolve_name_expr(t)).collect();
                    if !clause_types.is_empty()
                        && clause_types.iter().all(|ct| {
                            let Type::Object(class, _) = ct else {
                                return false;
                            };
                            previously_caught.iter().any(|pt| {
                                let Type::Object(prev, _) = pt else {
                                    return false;
                                };
                                class == prev || self.registry.is_subclass_of(class, prev)
                            })
                        })
                    {
                        let desc = clause_types
                            .iter()
                            .map(Type::to_string)
                            .collect::<Vec<_>>()
                            .join("|");
                        self.report(
                            label.to_string(),
                            format!("Dead catch - {desc} is already caught above."),
                            c.span,
                            4,
                            "catch.alreadyCaught",
                        );
                    }
                    previously_caught.extend(clause_types);
                    let mut catch_locals = locals.clone();
                    if let Some(var) = &c.var {
                        // A multi-type catch (`catch (TypeA|TypeB $e)`,
                        // real, valid PHP since 7.1) binds the caught
                        // variable to the *union* of every listed type -
                        // binding only the first discarded the others
                        // entirely, making the checker's verdict on
                        // member access through the variable depend on
                        // catch-type order, which real PHP itself doesn't
                        // care about at all.
                        let ty = match c.types.as_slice() {
                            [] => Type::Mixed,
                            [single] => self.resolve_name_expr(single),
                            multiple => Type::Union(
                                multiple.iter().map(|t| self.resolve_name_expr(t)).collect(),
                            ),
                        };
                        purge_stale_narrowing_for(&mut catch_locals, var);
                        catch_locals.insert(var.clone(), ty);
                    }
                    self.check_block(&c.body, &mut catch_locals, declared_return, label);
                    if !c
                        .body
                        .last()
                        .is_some_and(|s| self.always_terminates(s, &catch_locals))
                    {
                        end_states.push(catch_locals);
                    }
                }
                if let Some(first) = end_states.first().cloned() {
                    *locals = end_states
                        .into_iter()
                        .skip(1)
                        .fold(first, |acc, s| merge_scopes(&acc, &s));
                }
                if let Some(f) = finally {
                    self.check_block(f, locals, declared_return, label);
                }
            }
            StmtKind::Throw(e) => {
                // Finding 57: `throw` only accepts a real Throwable - a
                // real, unconditional PHP fatal error otherwise,
                // independent of anything about the thrown value at
                // runtime. Only checked when the thrown expression's own
                // class is actually known (a concrete `Type::Object`) -
                // anything else (Mixed, a dynamic expression) isn't
                // guessed at, matching this project's usual leniency for
                // "we don't know" cases.
                if let Type::Object(class, _) = self.check_expr(e, locals, label)
                    && !self.registry.is_subclass_of(&class, "Throwable")
                {
                    self.report(
                        label.to_string(),
                        format!("cannot throw {class}: it does not implement Throwable"),
                        e.span,
                        0,
                        "throw.notThrowable",
                    );
                }
            }
            // `global $x;` binds $x to whatever the global scope holds,
            // which this checker doesn't track - Mixed is the honest
            // answer, but the important part is that it's bound at all, so
            // using it afterward isn't flagged as undefined.
            StmtKind::Global(names) => {
                for name in names {
                    // Phase 65: `$this` is real PHP's own implicitly-
                    // bound reference, not a real global - "Cannot use
                    // $this as global variable." is a real,
                    // unconditional fatal error, confirmed directly.
                    if name == "this" {
                        self.report(
                            label.to_string(),
                            "cannot use $this as global variable".to_string(),
                            stmt.span,
                            0,
                            "global.this",
                        );
                        continue;
                    }
                    locals.entry(name.clone()).or_insert(Type::Mixed);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    // Phase 65: same idea as `global $this;` just above -
                    // "Cannot use $this as static variable.", confirmed
                    // directly.
                    if name == "this" {
                        self.report(
                            label.to_string(),
                            "cannot use $this as static variable".to_string(),
                            stmt.span,
                            0,
                            "static.this",
                        );
                        continue;
                    }
                    let ty = default
                        .as_ref()
                        .map(|e| self.check_expr(e, locals, label))
                        .unwrap_or(Type::Mixed);
                    purge_stale_narrowing_for(locals, name);
                    locals.insert(name.clone(), ty);
                    self.static_var_names.insert(name.clone());
                }
            }
            // `check_program`'s own top-level loop already handles a
            // top-level FunctionDecl/ClassDecl/EnumDecl by checking its
            // body - but any of the same three declared *nested* inside a
            // block (the extremely common `if (!function_exists('foo')) {
            // function foo() {} }` polyfill idiom, or a class conditionally
            // declared inside an if) used to fall into the wildcard arm
            // below and never got its own body checked at all - it was
            // indexed into the registry (so calls to it resolve), but
            // silently exempt from every diagnostic, undefined-variable
            // detection included. Mirrors check_program's handling.
            StmtKind::FunctionDecl(f) => self.check_function(f, None, false),
            StmtKind::ClassDecl(c) => {
                let fqn = self.name_ctx.declared_name(&c.name);
                self.check_attributes(&c.attrs, &fqn);
                for p in &c.properties {
                    self.check_attributes(&p.attrs, &fqn);
                }
                for cc in &c.consts {
                    self.check_attributes(&cc.attrs, &fqn);
                }
                // A conditionally-declared trait (the common `if
                // (version_compare(...)) { trait Foo {...} }`/`if
                // (PHP_VERSION_ID ...) { trait Foo {...} }` shape - real
                // corpus example, symfony/cache's own `Redis63ProxyTrait`)
                // reaches this nested arm instead of `check_program`'s own
                // top-level one, which already knows to dispatch a trait
                // through `check_trait_methods` (Phase 13 Finding E) so
                // its `$this->hostOnlyMethod()` calls are checked against
                // a real using class, not standalone. This arm's own
                // unconditional per-method loop below skipped that
                // entirely, checking every trait method with `current_
                // class` set to the *trait's own* name regardless - a
                // real false positive (`$this->initializeLazyObject()`,
                // provided by a sibling trait on the real host `RedisProxy`,
                // not by `Redis63ProxyTrait` itself) invisible to a
                // top-level-only trait but real for one declared here.
                if c.kind == ClassKind::Trait {
                    self.check_trait_methods(&fqn, c);
                } else {
                    for m in &c.methods {
                        self.check_function(&m.function, Some(&fqn), m.is_static);
                    }
                }
            }
            StmtKind::EnumDecl(e) => {
                let fqn = self.name_ctx.declared_name(&e.name);
                self.check_attributes(&e.attrs, &fqn);
                for case in &e.cases {
                    self.check_attributes(&case.attrs, &fqn);
                }
                for cc in &e.consts {
                    self.check_attributes(&cc.attrs, &fqn);
                }
                for m in &e.methods {
                    self.check_function(&m.function, Some(&fqn), m.is_static);
                }
            }
            // Phase 29: a bare `break;` (`None`, level 1) or an explicit
            // `break N;` targets the Nth-innermost open loop/switch frame
            // - `checked_sub` naturally no-ops for an out-of-range level
            // (more levels than open loops/switches, invalid PHP anyway),
            // matching this project's "don't guess" default. The level is
            // already parse-time-resolved to a plain `i64`, no expression
            // evaluation needed.
            StmtKind::Break(level) => {
                let depth = level.unwrap_or(1).max(1) as usize;
                if let Some(idx) = self.loop_break_states.len().checked_sub(depth) {
                    self.loop_break_states[idx].push(locals.clone());
                }
            }
            _ => {}
        }
        // Re-applied *after* dispatch too, but only when this exact
        // statement is itself a plain assignment to the same variable the
        // assertion names (the bare `/** @var Type */ $x = expr();` form,
        // or an explicitly-named `@var Type $x` sitting on its own
        // assignment) - the whole point of `@var` there is to override
        // whatever the assignment's own natural inference produces, which
        // `bind_assignment_target` (called from the `match` above, for
        // the `Assign` case) would otherwise silently clobber the
        // pre-applied assertion with. For every other statement shape
        // (an unrelated statement merely *reading* the variable), this
        // never matches - the single before-dispatch application above
        // already covers it, since the statement's own body needs to see
        // the narrowed type *during* its own check, not after.
        //
        // Deliberately does NOT extend this to a tag sitting directly on
        // a `foreach` header (`/** @var T $x */ foreach (...) { ... }`):
        // `bind_assignment_target` for the loop variable, and the loop
        // body's own check, both happen *inside* the `Foreach` arm above,
        // before this point ever runs - reapplying only here would fix
        // the type seen *after* the loop but leave the body itself seeing
        // the loop's own natural (usually `Mixed`) element type, a
        // half-working result worse than not supporting the shape at
        // all. The evidenced real-world case (`findCallSites`) instead
        // puts the tag on a statement *inside* the loop body, which the
        // before-dispatch application above already handles correctly
        // and completely on its own.
        if let Some((name, ty)) = inline_var
            && statement_defined_variable_names(&stmt.kind).contains(&name)
        {
            purge_stale_narrowing_for(locals, &name);
            locals.insert(name, ty);
        }
    }

    /// Recognizes an inline `/** @var Type $name */` (or bare `/** @var
    /// Type */`) doc comment sitting on `stmt` itself, and resolves it to
    /// the `(name, type)` assertion it makes - `None` for a statement with
    /// no doc comment, one with no `@var` tag, or a bare tag whose target
    /// can't be inferred (see `statement_defined_variable_names`'s own
    /// "don't guess" scope).
    fn inline_var_annotation(&self, stmt: &Stmt) -> Option<(String, Type)> {
        let doc = stmt.doc_comment.as_deref()?;
        // A cheap substring pre-filter - `check_stmt` is already
        // `#[hotpath::measure]`-instrumented, confirming this is a real
        // hot path worth avoiding a full docblock parse on every
        // statement's own `@param`/`@return`-only declaration comment.
        if !doc.contains("@var") {
            return None;
        }
        let block = docblock::parse(doc);
        let ty_str = block.var_type?;
        // A *named* tag (`@var Type $name`) always wins when present. The
        // bare form (`@var Type`, no name) applies to whatever single
        // variable the statement it precedes assigns, inferred from that
        // statement's own left-hand side, not guessed at from thin air.
        let name = block.var_name.or_else(|| {
            match statement_defined_variable_names(&stmt.kind).as_slice() {
                [single] => Some(single.clone()),
                _ => None,
            }
        })?;
        Some((name, self.resolve_type_hint(Some(&ty_str))))
    }
}

/// Every bare-variable name `kind` itself unconditionally (re)binds via a
/// plain `$x = ...;` expression statement's own LHS. Used two ways:
/// inferring a *nameless* `@var Type` tag's own target (only when there's
/// exactly one candidate), and deciding whether an inline `@var` assertion
/// (named or not) needs re-applying *after* this statement's own dispatch,
/// because the assignment's own natural binding would otherwise silently
/// overwrite it first. Deliberately doesn't cover `foreach` - see the
/// comment in `check_stmt` on why a tag on a `foreach` header isn't
/// supported at all, rather than half-supported.
fn statement_defined_variable_names(kind: &StmtKind) -> Vec<String> {
    match kind {
        StmtKind::Expression(e) => match &e.kind {
            ExprKind::Assign(lhs, _) => match &lhs.kind {
                ExprKind::Variable(name) => vec![name.clone()],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
        // Phase 60 corpus find: `StmtKind::StaticVar`'s own arm above
        // infers purely from the declaration's default expression
        // (`static $x = null;` types `$x` as exactly `Null`), unlike
        // every other statement shape here, which unconditionally
        // clobbered an already-applied `/** @var string|null */`
        // annotation instead of merely leaving it alone - real PHPStan's
        // own `@var` on a `static` declaration is exactly the same
        // "trust the annotation over the initializer" override every
        // other assignment already gets. Listed here so both existing
        // mechanisms (the nameless-tag target inference and the
        // post-dispatch reapplication) pick a `static` declaration up
        // for free, with no new plumbing of their own.
        StmtKind::StaticVar(vars) => vars.iter().map(|(name, _)| name.clone()).collect(),
        _ => Vec::new(),
    }
}
