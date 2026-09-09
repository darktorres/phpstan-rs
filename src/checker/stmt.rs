use crate::ast::*;
use crate::checker::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_PROPERTY, Checker,
    Locals, body_contains_yield, collect_exit_point_statements, exit_point_description,
    is_switch_break_equivalent, is_truthy_literal, merge_scopes, purge_stale_narrowing_for,
    stmt_contains_break, stmt_contains_escaping_exit,
};
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
        // Phase 15 item (1), `UnreachableStatementRule`: investigated,
        // corpus-verified, reverted, and now reinstated. The original
        // revert's root cause - a variable reassigned inside a loop kept
        // its pre-loop-entry type for the whole single-pass body check,
        // so a later guard on it could wrongly narrow to `Type::Never`,
        // read here as "this statement always terminates" - is fixed by
        // Phase 55's fixed-point loop-body convergence (`converge_loop_
        // body`). Real PHPStan's own equivalent (`UnreachableStatementRule`,
        // `deadCode.unreachable`, level 4) reports exactly once per
        // maximal unreachable run, at its first statement - matched here
        // via `reported_unreachable`, gating further reports without
        // gating further checking. Every statement is still run through
        // `check_stmt` regardless - real PHPStan still fully analyzes
        // code after an `UnreachableStatementNode` for every other rule,
        // and this project's own corpus verification confirmed why:
        // stopping early here silently dropped real, unrelated diagnostics
        // that happened to sit in a since-dead branch (amphp/http-client's
        // `Http1Parser::parse`, verified against an early, since-reverted
        // version of this change that did stop).
        let mut reported_unreachable = false;
        for (i, stmt) in stmts.iter().enumerate() {
            self.check_stmt(stmt, locals, declared_return, label);
            if !reported_unreachable
                && self.always_terminates(stmt, locals)
                && let Some(next) = stmts.get(i + 1)
                // A labeled statement `goto`-targeted somewhere else in
                // this same function is a real jump target, not dead
                // code, whatever the statement immediately before it
                // does - see `collect_goto_targets`'s own doc comment
                // for the real corpus false positive this guards
                // against.
                && !matches!(&next.kind, StmtKind::Label(name) if self.current_function_goto_targets.contains(name))
            {
                self.report(
                    label.to_string(),
                    "Unreachable statement - code above always terminates.".to_string(),
                    next.span,
                    4,
                    "deadCode.unreachable",
                );
                reported_unreachable = true;
            }
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
            self.check_inline_var_annotation_type(stmt, name, ty, locals, label);
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
                // Phase 41/82: a bare call to a now-known-pure declaration.
                self.check_pure_call_without_effect(e, locals, label);
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
                //
                // `name.trim_start_matches('\\')`: a fully-qualified call
                // (`\assert(...)`) keeps its leading backslash in
                // `ExprKind::Name`, unlike ordinary call resolution - the
                // same gap `apply_narrowing`'s own `ExprKind::Call` arm
                // already fixed for `is_string`/`is_array`/etc (see its
                // own comment). Confirmed by a real corpus false positive
                // this exact miss caused: amphp/http-client's
                // `FollowRedirects::removeDotSegments`, whose loop opens
                // every iteration with `\assert(\is_string($input));` -
                // written with the fully-qualified form throughout (a
                // common defensive-global-namespace style), so the
                // narrowing this whole block exists for never ran at all.
                //
                // Phase 55 follow-up: `assert($cond) && sideEffect();` (a
                // real, common idiom - the assertion doubling as a guard
                // clause for whatever follows) needs the identical
                // narrowing, not just a bare `assert($cond);` alone -
                // `bare_assert_condition`'s own doc comment has the
                // "which chain position is actually safe" reasoning.
                if let Some(cond) = bare_assert_condition(e) {
                    self.apply_narrowing(cond, locals, true);
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
                        // Real bug found and fixed this session: this used
                        // to require the receiver's own type to be
                        // literally `Type::Object`, silently skipping a
                        // `@phpstan-assert` narrowing opportunity for a
                        // `Union`/`Intersection` receiver.
                        let recv_ty = self.infer_expr(recv, locals);
                        if let Some((_, sig)) = self.find_method_owner(&recv_ty, method)
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
                    } else if expected == Type::Void && actual != Type::Void {
                        // Phase 61: same idea, one position over - a
                        // `void` function returning an actual value is
                        // real PHPStan's own dedicated `return.void`,
                        // not the generic mismatch path (`void` isn't a
                        // "wrong type" the way e.g. `string` vs `int`
                        // is - it's "no value was ever supposed to come
                        // back"). `actual != Type::Void` is a real,
                        // necessary exception, not a defensive
                        // afterthought: `return anotherVoidFn();`
                        // delegating to another void-returning call is
                        // legal, common PHP (real PHPStan doesn't flag
                        // it either - a `void` function returning
                        // *nothing new*, just handing back void-ness
                        // itself, isn't "returning a value" at all).
                        // Without this, every such delegation reported
                        // the nonsensical "expected void, got void" -
                        // a real corpus false positive this project's
                        // own registry fix for conditional-function-
                        // declaration arity tiebreaking (a top-10-bucket
                        // scoping round) unmasked by finally resolving
                        // `\RectorPrefix202608\trigger_deprecation(...)`
                        // to its own real, correctly-`void`-typed
                        // signature instead of leaving it unresolved.
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
                self.check_if(
                    cond,
                    then_branch,
                    else_branch,
                    locals,
                    declared_return,
                    label,
                    false,
                );
            }
            StmtKind::While { cond, body } => {
                // Phase 15 continuation: real bug found and fixed this
                // session - the condition's own constant-condition check
                // (and, transitively, any standalone check reached while
                // checking it, like `BooleanNot`/`BooleanAnd`/`BooleanOr`)
                // used to run exactly once, against the pre-loop entry
                // `locals`, *before* `converge_loop_body` ever ran -
                // completely bypassing Phase 55's own fixed-point
                // convergence. Real corpus false positive this produced:
                // `$end = false; while (!$end) { ...; $end = true; ... }`
                // (`Piwigo\Admin\Extensions\CoreUpdateService::upgradeTo`)
                // - `$end`'s pre-loop type is exactly `false`, so `!$end`
                // looked unconditionally `true` even though the loop body
                // can reassign it. Confirmed directly against real
                // PHPStan's own `WhileHandler::processStmt`: it re-
                // evaluates the condition at the top of *every*
                // convergence pass (discarding all but the last with a
                // no-op node callback), and only the FINAL, fully-
                // converged pass's walk of the condition ever reaches the
                // real rule-invoking callback. Moving the condition check
                // inside `converge_loop_body`'s own closure reproduces
                // that exactly for free: every pass but the kept one
                // already has its diagnostics discarded via
                // `converge_loop_body`'s own `diagnostics.truncate`.
                let skip_always_true = stmt_contains_break(body)
                    || stmt_contains_escaping_exit(body)
                    || body_contains_yield(std::slice::from_ref(body.as_ref()))
                    || declared_return == Some(&Type::Never);
                // Real bug found and fixed this session, on top of the
                // one above: moving the diagnostic-bearing condition
                // check into the convergence closure means it now only
                // ever runs against per-pass *copies* of the entry
                // (`pass_locals`), never mutating the real `locals`
                // directly the way a single up-front call used to. A
                // real PHP `while`'s own condition always runs at least
                // once, unconditionally, regardless of how many times the
                // body itself iterates - so a variable *the condition
                // itself* assigns (`while (($x = f()) !== null) {...}`,
                // a real, common idiom) must be treated as unconditionally
                // defined afterward, unlike anything only the *body*
                // conditionally defines. Without this, `merge_scopes`
                // dropped it entirely (present in the loop's own exit
                // state but never in the real pre-loop `locals` it gets
                // merged against). Confirmed directly against real
                // PHPStan's own `WhileHandler::processStmt`: it also does
                // a quiet, non-diagnostic evaluation of the condition
                // against the real pre-loop scope first (its own
                // `$condResult`/`$condScope`, built with a `NoopNodeCallback`),
                // strictly separate from the later diagnostic-bearing
                // walk against the converged state - replicated here by
                // running `check_expr` once for real (to bind any
                // assignment side effect into `locals`) and discarding
                // whatever it reported, since the real diagnostics come
                // from the convergence-aware walk below. Real corpus
                // false positive this fixes: `Symfony\Component\
                // DependencyInjection\Compiler\AbstractRecursivePass::
                // getConstructor`'s `while ((null === $class = $definition
                // ->getClass()) && $definition instanceof ChildDefinition)`.
                let diagnostics_before = self.diagnostics.len();
                self.check_expr(cond, locals, label);
                self.diagnostics.truncate(diagnostics_before);
                let entry = locals.clone();
                self.loop_depth += 1;
                let (body_locals, break_states) = self.converge_loop_body(
                    &entry,
                    declared_return,
                    label,
                    |this, pass_locals, declared_return, label, _pass| {
                        this.in_condition_position = true;
                        this.check_expr(cond, pass_locals, label);
                        // Phase 15, `WhileLoopAlwaysFalseConditionRule`/
                        // `WhileLoopAlwaysTrueConditionRule`: real
                        // inferred-type trust (`inferred_constant_
                        // condition`), matching real PHPStan's own design
                        // now that both reasons the original version was
                        // reverted are fixed - see `check_if`'s own doc
                        // comment. `alwaysFalse` has no exclusions,
                        // matching real PHPStan's own rule exactly.
                        // `alwaysTrue` is gated the same way real
                        // PHPStan's own `WhileLoopAlwaysTrueConditionRule`
                        // is (confirmed directly against `vendor/
                        // phpstan-src`): skipped when the loop is
                        // breakless (`stmt_contains_break` - this
                        // project's own already-existing, deliberately
                        // conservative "any break anywhere" check, reused
                        // rather than re-deriving real PHPStan's own more
                        // precise per-level break/`continue N` analysis -
                        // a narrow, disclosed simplification, see
                        // ROADMAP), when the body yields (a legitimate
                        // infinite generator, not dead code), or when the
                        // enclosing function is declared `: never`
                        // (already documents "this never returns" on its
                        // own, redundant to also flag the loop).
                        if this.inferred_constant_condition(cond, pass_locals) == Some(false) {
                            this.report(
                                label.to_string(),
                                "While loop condition is always false.".to_string(),
                                cond.span,
                                4,
                                "while.alwaysFalse",
                            );
                        } else if this.inferred_constant_condition(cond, pass_locals) == Some(true)
                            && !skip_always_true
                        {
                            this.report(
                                label.to_string(),
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
                    // The condition can never itself become false (a
                    // literal `while (true)`/`while (1)`), so the *only*
                    // way to reach code after this loop at all is via a
                    // `break` somewhere in the body - neither the "may
                    // run zero times" merge below nor the "reaching here
                    // means cond went false" narrowing after it applies:
                    // this loop always runs at least once, and never
                    // exits any other way. `is_truthy_literal` is the
                    // same check `always_terminates`'s own `While` arm
                    // already uses for the identical reasoning, one level
                    // up (a break-less `while (true) {...}` never lets
                    // control reach past it at all). Real corpus false
                    // positive this fixes: `while (true) { $x = ...;
                    // break; } return $x;` - the old unconditional
                    // zero-iteration merge below discarded `$x` from
                    // every representative state that reached the
                    // `break`, leaving it looking undefined afterward.
                    //
                    // Folds every `break`-reached state together, seeded
                    // by the first rather than the (semantically
                    // meaningless here) pre-loop `locals` - with none at
                    // all, `locals` is left as its own pre-loop state, a
                    // safe fallback for what's genuinely unreachable code
                    // anyway (an infinite loop with no escape).
                    let mut states = break_states.into_iter();
                    if let Some(first) = states.next() {
                        let mut merged = first;
                        for state in states {
                            merged = merge_scopes(&merged, &state);
                        }
                        *locals = merged;
                    }
                } else {
                    // The loop may run zero times, so what's true after it
                    // is "either the pre-loop state, or whatever one
                    // narrowed iteration produces" - not a real fixed
                    // point, but sound enough to avoid claiming more
                    // precision than we have.
                    *locals = merge_scopes(locals, &body_locals);
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
                    // Phase 29: fold in every `break`-reached state too -
                    // see `loop_break_states`'s own doc comment on
                    // `Checker`.
                    for state in break_states {
                        *locals = merge_scopes(locals, &state);
                    }
                }
            }
            StmtKind::DoWhile { body, cond } => {
                // The first pass has nothing to narrow going in - the
                // condition is only evaluated after the body has already
                // run once. Every *re*-convergence pass (`pass > 0`) is
                // different: reaching it at all means the previous pass's
                // own trailing `cond` evaluated true (that's the only way
                // a do-while body runs again), so the "continue" half of
                // `cond`'s own narrowing is a real, provable fact at that
                // pass's entry - the same narrowing a `while` loop's own
                // convergence closure already applies unconditionally on
                // every pass (its condition sits at the *start* of the
                // body instead), just shifted here to skip pass 0.
                //
                // Real corpus bug this fixes (`Latte\Essential\
                // CachingIterator::__construct`'s `do { $iterator =
                // $iterator->getIterator(); } while (!$iterator instanceof
                // \Iterator);`): without this, pass 2's own entry widens
                // `$iterator` all the way back to the un-narrowed
                // `Traversable` (the union of pass 1's `IteratorAggregate`
                // entry and its own post-body `Traversable` exit), and
                // `$iterator->getIterator()` isn't a real `Traversable`
                // method - a false `method.notFound`. Applying `!$iterator
                // instanceof \Iterator`'s own true-branch narrowing here
                // subtracts `Iterator` back out first (`narrow_type_
                // excluding`'s existing `Traversable`-minus-`Iterator`
                // special case), correctly re-narrowing to
                // `IteratorAggregate` instead.
                let entry = locals.clone();
                self.loop_depth += 1;
                let (body_locals, break_states) = self.converge_loop_body(
                    &entry,
                    declared_return,
                    label,
                    |this, pass_locals, declared_return, label, pass| {
                        if pass > 0 {
                            this.apply_narrowing(cond, pass_locals, true);
                        }
                        this.check_stmt(body, pass_locals, declared_return, label);
                    },
                );
                // A do-while's body always runs at least once - the
                // converged result replaces `locals` directly, the same
                // "no zero-iterations merge" choice this arm already made.
                *locals = body_locals;
                self.in_condition_position = true;
                // `loop_depth` deliberately stays incremented one
                // statement longer than While's own arm needs it to -
                // real corpus false positive this fixes (Symfony's own
                // `LoggerTest`'s `do { ...; $attempts++; } while ((...) &&
                // $attempts < 150);`): this trailing check of the loop's
                // own condition is *itself* still evaluating a loop-
                // carried variable's post-convergence type, exactly as
                // approximate as any pass inside the loop - `check_
                // constant_numeric_comparison`/`check_constant_loose_
                // comparison`'s own `self.loop_depth > 0` guard (`checker/
                // expr.rs`) needs to see it still incremented here, or
                // `$attempts`'s own convergence-bounded range (this
                // project's 3-pass approximation reaches nowhere near the
                // real 150) makes `$attempts < 150` look unconditionally,
                // wrongly provable.
                self.check_expr(cond, locals, label);
                self.loop_depth -= 1;
                // Phase 15, `DoWhileLoopConstantConditionRule`: real
                // inferred-type trust, same as `If`/`While` above. Both
                // directions are gated on breakless-ness here (confirmed
                // directly against real PHPStan's own rule): `alwaysFalse`
                // skips when the body has a `break` (`do { ...; if ($x)
                // break; ... } while (false);` is a real, common
                // "structured goto" idiom, not dead code); `alwaysTrue`
                // skips on the same `stmt_contains_break` check plus a
                // yield check (no `declared_return: never` exclusion here
                // - confirmed real PHPStan's own rule doesn't have one,
                // unlike `While`'s).
                if self.inferred_constant_condition(cond, locals) == Some(false) {
                    if !stmt_contains_break(body) {
                        self.report(
                            label.to_string(),
                            "Do-while loop condition is always false.".to_string(),
                            cond.span,
                            4,
                            "doWhile.alwaysFalse",
                        );
                    }
                } else if self.inferred_constant_condition(cond, locals) == Some(true)
                    && !stmt_contains_break(body)
                    && !stmt_contains_escaping_exit(body)
                    && !body_contains_yield(std::slice::from_ref(body.as_ref()))
                {
                    self.report(
                        label.to_string(),
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
                for e in init.iter() {
                    self.check_expr(e, locals, label);
                }
                // A real `for`'s condition slot can hold several
                // comma-separated expressions, *all* of which real PHP
                // re-evaluates on every single pass through the condition
                // (only the *last* one's truthiness actually gates the
                // loop - the others are there purely for a side effect,
                // but that side effect happens every iteration, not just
                // once). Both halves - the non-last expressions here, and
                // the last one's own check moved into the convergence
                // closure below - need the identical Phase 15 continuation
                // fix (the same real bug this session's own `While` fix
                // addresses: checking any of them once, up front, against
                // the pre-loop entry, would bypass Phase 55's fixed-point
                // convergence entirely, the exact false positive real
                // corpus code hit - `QMultiToken::parseExpression`'s
                // `for ($stop = false; !$stop && ...; $qi++)`, where
                // `$stop` is reassigned inside the body).
                match cond.last() {
                    Some(c) => {
                        // Same real bug and fix as `While`'s own arm
                        // above: a variable *any* condition expression
                        // itself assigns (`for (;; ($x = f()) !== null; )`,
                        // unusual but real) must be unconditionally
                        // defined afterward, since a `for`'s condition
                        // always runs at least once regardless of body
                        // iterations - a quiet, diagnostics-discarded
                        // evaluation against the real pre-loop `locals`
                        // binds every condition expression's own side
                        // effect (not just the last one's) before the
                        // convergence-aware, diagnostic-bearing walk runs
                        // against per-pass copies.
                        let diagnostics_before = self.diagnostics.len();
                        for e in cond.iter() {
                            self.check_expr(e, locals, label);
                        }
                        self.diagnostics.truncate(diagnostics_before);
                        let entry = locals.clone();
                        self.loop_depth += 1;
                        let (body_locals, break_states) = self.converge_loop_body(
                            &entry,
                            declared_return,
                            label,
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
                        self.loop_depth -= 1;
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
                        let entry = locals.clone();
                        self.loop_depth += 1;
                        let (body_locals, break_states) = self.converge_loop_body(
                            &entry,
                            declared_return,
                            label,
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
                // bound (to the subject's known element type, when it's a
                // homogeneous array) before checking anything that could
                // reference them, at the start of every convergence pass
                // below (see that closure's own comment for why every
                // pass, not just once).
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
                //
                // Phase 55: the body itself now converges to a fixed point
                // across up to 3 passes (see `converge_loop_body`'s own doc
                // comment) - a variable reassigned inside the body is no
                // longer stuck at its pre-loop type for the whole check.
                // The *entry* state fed to every pass, and the final
                // result, are both still `locals` directly - never merged
                // with a separate pre-loop snapshot, preserving the
                // deliberate zero-iterations trade-off described above
                // exactly. Only the within/across-iteration staleness this
                // phase targets is fixed.
                //
                // The key/value binding itself is re-applied at the start
                // of *every* pass, not just once before the first - real
                // PHP rebinds a foreach's loop variable(s) fresh from the
                // subject on every actual iteration, so a pass must never
                // start from what the *previous* pass's body left the
                // loop variable mutated to. Confirmed necessary by a real
                // corpus false positive this exact ordering fixed:
                // `QNumericRangeScope::parse`'s `foreach ($range as $i =>
                // &$val) { ...; $val = floatval(...); }` - a by-reference
                // loop variable reassigned inside the body would otherwise
                // wrongly union its post-body `float` into the *next*
                // pass's entry as `string|float`, even though each real
                // iteration's `&$val` aliases a *different* array slot
                // that pass's body never touched.
                let entry = locals.clone();
                self.loop_depth += 1;
                let (body_locals, break_states) = self.converge_loop_body(
                    &entry,
                    declared_return,
                    label,
                    |this, pass_locals, declared_return, label, _pass| {
                        if let Some(k) = key {
                            this.bind_simple_target(k, Type::Mixed, pass_locals);
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
                        this.bind_assignment_target(value, element_ty.clone(), pass_locals);
                        this.check_stmt(body, pass_locals, declared_return, label);
                    },
                );
                self.loop_depth -= 1;
                *locals = body_locals;
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
                //
                // Phase 55: `loop_continue_states` gets the identical
                // push/pop, for the identical reason - real PHP's own
                // `continue`/`break` both count a `switch` as one level
                // (the well-known "bare `continue;` directly inside a
                // `switch` behaves like `break;`" gotcha), so the two
                // stacks must stay depth-aligned or `continue N`'s own
                // frame-index math would attribute a level to the wrong
                // enclosing loop.
                self.loop_break_states.push(Vec::new());
                self.loop_continue_states.push(Vec::new());
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
                        Some(s) if is_switch_break_equivalent(s) => true,
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
                self.loop_continue_states.pop();
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
                // Phase 42(4), `OverwrittenExitPointByFinallyRule`:
                // gathers every concrete exit-point statement from the
                // try body and each catch clause that itself always
                // terminates - if the `finally` block below *also*
                // unconditionally exits, every one of these gets
                // silently discarded at runtime (real PHP's own well-
                // known gotcha: a `finally`'s own `return`/`throw`/
                // `break`/`continue` always wins, even over an in-
                // flight exception). See `collect_exit_point_statements`'s
                // own doc comment for why this only names the common
                // `Block`/`If`-nested shapes rather than every construct
                // `always_terminates` itself recognizes.
                let mut try_catch_exit_points: Vec<&Stmt> = Vec::new();
                if try_terminates && let Some(s) = body.last() {
                    try_catch_exit_points.extend(collect_exit_point_statements(s));
                }
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
                            } else {
                                if !self.registry.is_subclass_of(&class, "Throwable") {
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
                                self.check_internal_catch(&class, c.span, label);
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
                        // Phase 17 follow-up: real declaration - `var_span`
                        // is always `Some` exactly when `var` is (the
                        // parser sets both together), `c.span` is a
                        // defensive fallback only.
                        catch_locals.declare(var.clone(), ty, c.var_span.unwrap_or(c.span));
                    }
                    self.check_block(&c.body, &mut catch_locals, declared_return, label);
                    let catch_terminates = c
                        .body
                        .last()
                        .is_some_and(|s| self.always_terminates(s, &catch_locals));
                    if catch_terminates {
                        if let Some(s) = c.body.last() {
                            try_catch_exit_points.extend(collect_exit_point_statements(s));
                        }
                    } else {
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
                    // Phase 42(4): only worth naming the finally block's
                    // own exit point(s) if there's actually something
                    // real on the try/catch side for them to overwrite -
                    // mirrors `OverwrittenExitPointByFinallyRule`'s own
                    // `count($node->getTryCatchExitPoints()) === 0`
                    // early return exactly.
                    if !try_catch_exit_points.is_empty()
                        && let Some(last) = f.last()
                        && self.always_terminates(last, locals)
                    {
                        let finally_exit_points = collect_exit_point_statements(last);
                        if !finally_exit_points.is_empty() {
                            for exit in &try_catch_exit_points {
                                self.report(
                                    label.to_string(),
                                    format!(
                                        "This {} is overwritten by a different one in the finally block below.",
                                        exit_point_description(exit)
                                    ),
                                    exit.span,
                                    4,
                                    "finally.exitPoint",
                                );
                            }
                            for exit in &finally_exit_points {
                                self.report(
                                    label.to_string(),
                                    format!(
                                        "The overwriting {} is on this line.",
                                        exit_point_description(exit)
                                    ),
                                    exit.span,
                                    4,
                                    "finally.exitPoint",
                                );
                            }
                        }
                    }
                }
            }
            StmtKind::Throw(e) => {
                // Finding 57: `throw` only accepts a real Throwable - a
                // real, unconditional PHP fatal error otherwise,
                // independent of anything about the thrown value at
                // runtime. `thrown_value_not_throwable` (`checker/expr.rs`)
                // handles the "don't guess" leniency for anything not
                // concrete enough to judge (Mixed, a dynamic expression),
                // and now also a `Union`/`Intersection`-typed value - a
                // real bug found and fixed this session, this call site
                // used to require literally `Type::Object` too, matching
                // `ExprKind::Throw`'s own identical gap just fixed
                // alongside it.
                let thrown_ty = self.check_expr(e, locals, label);
                if let Some(class) = self.thrown_value_not_throwable(&thrown_ty) {
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
                    // Phase 17 follow-up: real, lexically-visible binding
                    // syntax - a `global $x;` declares `$x` here, even
                    // though this checker deliberately doesn't try to
                    // link it back to any specific top-level assignment
                    // (see this arm's own doc comment above) - `stmt.span`
                    // is as precise an answer as the type side already
                    // gives (`Mixed`, not a real inferred type either).
                    // `or_insert`'s own "only if not already bound"
                    // semantics carry over: `.declare` unconditionally
                    // overwrites, so it's only called when there's
                    // nothing here yet to overwrite.
                    if !locals.contains_key(name) {
                        locals.declare(name.clone(), Type::Mixed, stmt.span);
                    }
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
                    // Phase 17 follow-up: real declaration - no per-name
                    // span exists on `StaticVar` (only the whole
                    // statement's), same as `Global` just above.
                    locals.declare(name.clone(), ty, stmt.span);
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
                self.check_attribute_targets(&c.attrs, ATTRIBUTE_TARGET_CLASS, "class", &fqn);
                if c.kind == ClassKind::Trait {
                    self.check_trait_is_not_an_attribute_class(&c.attrs, &fqn);
                }
                for p in &c.properties {
                    self.check_attributes(&p.attrs, &fqn);
                    self.check_attribute_targets(
                        &p.attrs,
                        ATTRIBUTE_TARGET_PROPERTY,
                        "property",
                        &fqn,
                    );
                }
                for cc in &c.consts {
                    self.check_attributes(&cc.attrs, &fqn);
                    self.check_attribute_targets(
                        &cc.attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                        &fqn,
                    );
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
                self.check_attribute_targets(&e.attrs, ATTRIBUTE_TARGET_CLASS, "class", &fqn);
                for case in &e.cases {
                    self.check_attributes(&case.attrs, &fqn);
                    self.check_attribute_targets(
                        &case.attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                        &fqn,
                    );
                }
                for cc in &e.consts {
                    self.check_attributes(&cc.attrs, &fqn);
                    self.check_attribute_targets(
                        &cc.attrs,
                        ATTRIBUTE_TARGET_CLASS_CONSTANT,
                        "class constant",
                        &fqn,
                    );
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
            // Phase 55: the `continue` counterpart of `Break` just above -
            // see `loop_continue_states`'s own doc comment on `Checker`
            // for why this exists (feeding a `continue`-reached state
            // into the *next* convergence pass, not just past-the-loop
            // exit state the way `break`'s states are used).
            StmtKind::Continue(level) => {
                let depth = level.unwrap_or(1).max(1) as usize;
                if let Some(idx) = self.loop_continue_states.len().checked_sub(depth) {
                    self.loop_continue_states[idx].push(locals.clone());
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

    /// `StmtKind::If`'s own real body, extracted so a nested `elseif`
    /// (which the parser already desugars into a nested `If` inside
    /// `else_branch` - confirmed directly in `parser/stmt.rs::parse_if`:
    /// `self.tokens[self.pos] = Token::Keyword(Keyword::If); Some(Box::new
    /// (self.parse_if()?))`) can recurse into `check_if` itself
    /// (`is_elseif: true`) instead of the generic `check_stmt` dispatch,
    /// carrying its own chain-position context along - `check_stmt` never
    /// gains an `is_elseif` parameter, so its ~dozens of other call sites
    /// need no change at all.
    ///
    /// Phase 15 continuation, `IfConstantConditionRule`/
    /// `ElseIfConstantConditionRule`: the original type-inference-trusting
    /// design is reinstated (`inferred_constant_condition`, see its own
    /// doc comment) now that both real reasons it was reverted are fixed
    /// (Phase 55's convergence work; `widen_coalesce_result`'s own
    /// general-case bug, fixed this session). A genuine `elseif` gets its
    /// own real identifier/message (not `if.*`) and real PHPStan's own
    /// "last condition, no trailing else" exemption for `alwaysTrue`
    /// only, confirmed against `vendor/phpstan-src`'s own
    /// `reportAlwaysTrueInLastCondition: false` default - a trailing
    /// `elseif (true)` with nothing after it is a real, common "acts as
    /// an implicit else" idiom; `alwaysFalse` is never exempted this way,
    /// dead code is dead code regardless of position. A bare, non-elseif
    /// `If` has no such chain-position concept at all and is never
    /// exempted.
    #[allow(clippy::too_many_arguments)]
    fn check_if(
        &mut self,
        cond: &Expr,
        then_branch: &Stmt,
        else_branch: &Option<Box<Stmt>>,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
        is_elseif: bool,
    ) {
        let is_last_without_else = else_branch.is_none();
        self.in_condition_position = true;
        self.in_exempt_last_condition = is_elseif && is_last_without_else;
        self.check_expr(cond, locals, label);
        if let Some(value) = self.inferred_constant_condition(cond, locals) {
            if is_elseif {
                if !(value && is_last_without_else) {
                    self.report(
                        label.to_string(),
                        format!(
                            "Elseif condition is always {}.",
                            if value { "true" } else { "false" }
                        ),
                        cond.span,
                        4,
                        if value {
                            "elseif.alwaysTrue"
                        } else {
                            "elseif.alwaysFalse"
                        },
                    );
                }
            } else {
                self.report(
                    label.to_string(),
                    format!(
                        "If condition is always {}.",
                        if value { "true" } else { "false" }
                    ),
                    cond.span,
                    4,
                    if value {
                        "if.alwaysTrue"
                    } else {
                        "if.alwaysFalse"
                    },
                );
            }
        }

        let mut then_locals = locals.clone();
        self.apply_narrowing(cond, &mut then_locals, true);
        self.check_stmt(then_branch, &mut then_locals, declared_return, label);

        let mut else_locals = locals.clone();
        self.apply_narrowing(cond, &mut else_locals, false);
        if let Some(e) = else_branch {
            if let StmtKind::If {
                cond: nested_cond,
                then_branch: nested_then,
                else_branch: nested_else,
            } = &e.kind
            {
                self.check_if(
                    nested_cond,
                    nested_then,
                    nested_else,
                    &mut else_locals,
                    declared_return,
                    label,
                    true,
                );
            } else {
                self.check_stmt(e, &mut else_locals, declared_return, label);
            }
        }

        // The classic guard clause - `if ($x === null) { return; }` -
        // depends on this: a branch that unconditionally
        // returns/throws/breaks/continues never reaches the merge point,
        // so only the surviving branch's (narrowed) scope should
        // propagate past the if. Without this, the terminated branch's
        // narrowing (here, "$x is null") would incorrectly leak into the
        // code after the guard.
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
        let ty = self.resolve_type_hint(Some(&ty_str));
        // `resolve_type_hint` alone has no idea this text came from a
        // docblock rather than a native hint, so it can't tell "Options"
        // apart from a genuine, unresolvable class reference - a
        // `@phpstan-type` alias declared on the current class needs the
        // same expansion here that `ClassInfo::from_decl` already gives
        // every member's own `@param`/`@return`/`@var` (real corpus
        // shape: `league/uri-interfaces`'s own `UriString::parse()`,
        // `/** @var ComponentMap $components */`). Silently a no-op
        // outside any class, or one with no `@phpstan-type` tags at all.
        let ty = match &self.current_class {
            Some(class) => match self.registry.classes.get(class) {
                Some(info) if !info.type_aliases.is_empty() => {
                    crate::registry::substitute_type_aliases(ty, &info.type_aliases)
                }
                _ => ty,
            },
            None => ty,
        };
        Some((name, ty))
    }

    /// `@var Type $name` / bare `@var Type`'s own error-reporting half -
    /// real PHPStan's `VarTagTypeRuleHelper`/`VarTagChangedExpressionTypeRule`,
    /// scoped down to the single highest-value, most common real shape: a
    /// plain assignment statement (`/** @var Type */ $x = expr;`) whose
    /// own RHS's naturally-inferred type the declared tag can't possibly
    /// be describing (not even a supertype of it) - a real, catchable
    /// mistake (a typo'd type, a stale tag after a refactor). Real
    /// PHPStan's own version covers substantially more (list/array
    /// destructuring targets, a native-type-vs-docblock-type split with
    /// its own separate identifier and config toggle, a
    /// `containsPhpStanType`-specific "this exact type, don't widen"
    /// assumption check) - deliberately not replicated, a disclosed scope
    /// trim matching this project's own simpler type system (no native/
    /// docblock type split at this granularity) and "favor false
    /// negatives" precedent throughout. `Type::Mixed` on either side is
    /// never flagged - it means "no type declared at all" as often as
    /// real `mixed` in this project's own overloaded convention (see
    /// this file's own module doc comment), so a mismatch there is
    /// usually not a real one.
    fn check_inline_var_annotation_type(
        &mut self,
        stmt: &Stmt,
        name: &str,
        declared_ty: &Type,
        locals: &Locals,
        label: &str,
    ) {
        let StmtKind::Expression(e) = &stmt.kind else {
            return;
        };
        let ExprKind::Assign(lhs, rhs) = &e.kind else {
            return;
        };
        let ExprKind::Variable(lhs_name) = &lhs.kind else {
            return;
        };
        if lhs_name != name {
            return;
        }
        if matches!(declared_ty, Type::Mixed) {
            return;
        }
        let rhs_ty = self.infer_expr(rhs, locals);
        if matches!(rhs_ty, Type::Mixed) {
            return;
        }
        // Real corpus find: `is_assignable(&rhs_ty, declared_ty)` alone
        // flags the single most common real reason anyone writes an
        // inline `@var` in the first place - *refining* a wide, less-
        // precise inferred type (Doctrine's own bare `array`-returning
        // `getArrayResult()`, real Piwigo shape) into a narrower,
        // perfectly compatible one (`list<array<string, mixed>>`) - a
        // list is unconditionally still an array, not a genuine
        // mismatch. `declared_ty` only ever needs to be *some* plausible
        // relative of `rhs_ty` - a subtype (the refining case above) or
        // a supertype (widening, rarer but equally legitimate) - so both
        // directions have to fail before this is a real, catchable
        // mistake (a typo'd type, a stale tag after a refactor), matching
        // this function's own doc comment's "not even a supertype of it"
        // phrasing exactly (the code just didn't implement it yet).
        if !self.registry.is_assignable(&rhs_ty, declared_ty)
            && !self.registry.is_assignable(declared_ty, &rhs_ty)
        {
            self.report(
                label.to_string(),
                format!(
                    "PHPDoc tag @var with type {declared_ty} is not compatible with the assigned expression's own type {rhs_ty}."
                ),
                lhs.span,
                2,
                "varTag.type",
            );
        }
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
/// The condition to narrow for, if `e` is a bare `assert(...)` call
/// (`\assert` included), or a top-level `&&`-chain whose *leftmost*
/// operand is one - `None` otherwise. Only the leftmost position is
/// safe: `&&` evaluates left to right and short-circuits on the first
/// falsy operand, so the leftmost operand is unconditionally evaluated
/// the moment this whole statement is reached at all, and (being an
/// `assert()` call) throws rather than let execution continue if its
/// own condition were false - meaning reaching *past* this statement
/// proves it held, exactly like the bare, whole-statement case. Any
/// operand *after* the first has no such guarantee (if an earlier
/// operand was falsy, `&&` short-circuits before ever reaching it at
/// all, and the statement still completes normally either way) - not
/// walked past the leftmost position, even for a real, common `assert(
/// $a) && assert($b);` double-assertion, a real, disclosed scope trim
/// rather than a wrong answer.
fn bare_assert_condition(e: &Expr) -> Option<&Expr> {
    fn as_bare_assert_call(e: &Expr) -> Option<&Expr> {
        let ExprKind::Call(callee, args) = &e.kind else {
            return None;
        };
        let ExprKind::Name(name) = &callee.kind else {
            return None;
        };
        if name.trim_start_matches('\\') != "assert" {
            return None;
        }
        args.first().map(|a| &a.value)
    }
    match &e.kind {
        ExprKind::Binary(BinOp::And, l, _) => as_bare_assert_call(l),
        _ => as_bare_assert_call(e),
    }
}

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
