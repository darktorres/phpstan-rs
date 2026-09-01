use crate::ast::*;
use crate::checker::{Checker, Locals, merge_scopes};
use crate::types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_block(
        &mut self,
        stmts: &[Stmt],
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
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
        match &stmt.kind {
            StmtKind::Expression(e) => {
                self.check_expr(e, locals, label);
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
            }
            StmtKind::Return(Some(e)) => {
                let actual = self.check_expr(e, locals, label);
                if let Some(expected) = declared_return {
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    if actual != Type::Mixed && !self.registry.is_assignable(&actual, &expected) {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got {actual}"),
                            e.span,
                            3,
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
                    if expected != Type::Void
                        && expected != Type::Mixed
                        && !self.registry.is_assignable(&Type::Null, &expected)
                    {
                        self.report(
                            label.to_string(),
                            format!("return type mismatch: expected {expected}, got null"),
                            stmt.span,
                            3,
                        );
                    }
                }
            }
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.check_expr(e, locals, label);
                }
            }
            StmtKind::Block(stmts) => self.check_block(stmts, locals, declared_return, label),
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(cond, locals, label);

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
                let mut body_locals = locals.clone();
                self.apply_narrowing(cond, &mut body_locals, true);
                self.check_stmt(body, &mut body_locals, declared_return, label);
                // The loop may run zero times, so what's true after it is
                // "either the pre-loop state, or whatever one narrowed
                // iteration produces" - not a real fixed point, but sound
                // enough to avoid claiming more precision than we have.
                *locals = merge_scopes(locals, &body_locals);
                // Reaching code after the loop always means `cond` just
                // evaluated false, regardless of how many iterations ran -
                // real, additional narrowing on top of the merge above,
                // independent of it (e.g. a "loop until no longer null"
                // idiom is fully narrowed non-null afterward).
                self.apply_narrowing(cond, locals, false);
            }
            StmtKind::DoWhile { body, cond } => {
                // The condition is only evaluated after the body has
                // already run once, so there's nothing to narrow going in.
                self.check_stmt(body, locals, declared_return, label);
                self.check_expr(cond, locals, label);
                // Same as `While` - reaching code after a do-while always
                // means `cond` just evaluated false.
                self.apply_narrowing(cond, locals, false);
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
                        self.check_stmt(body, &mut body_locals, declared_return, label);
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
                        self.apply_narrowing(c, locals, false);
                    }
                    None => {
                        // `for (;;)` - no condition means an unconditional
                        // infinite loop (barring an internal break/return/
                        // etc.), so the body genuinely always executes at
                        // least once - no "may run zero times" merge is
                        // needed or correct here.
                        self.check_stmt(body, locals, declared_return, label);
                        for e in step {
                            self.check_expr(e, locals, label);
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
                let element_ty = match subject_ty {
                    Type::Array(inner) => *inner,
                    _ => Type::Mixed,
                };
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
                self.check_stmt(body, locals, declared_return, label);
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
                        if let Type::Object(class, _) = self.resolve_name_expr(t)
                            && !self.registry.is_subclass_of(&class, "Throwable")
                        {
                            self.report(
                                label.to_string(),
                                format!("cannot catch {class}: it does not implement Throwable"),
                                c.span,
                                0,
                            );
                        }
                    }
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
                        catch_locals.insert(var.clone(), ty);
                    }
                    self.check_block(&c.body, &mut catch_locals, declared_return, label);
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
                    );
                }
            }
            // `global $x;` binds $x to whatever the global scope holds,
            // which this checker doesn't track - Mixed is the honest
            // answer, but the important part is that it's bound at all, so
            // using it afterward isn't flagged as undefined.
            StmtKind::Global(names) => {
                for name in names {
                    locals.entry(name.clone()).or_insert(Type::Mixed);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    let ty = default
                        .as_ref()
                        .map(|e| self.check_expr(e, locals, label))
                        .unwrap_or(Type::Mixed);
                    locals.insert(name.clone(), ty);
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
                for m in &c.methods {
                    self.check_function(&m.function, Some(&fqn), m.is_static);
                }
            }
            StmtKind::EnumDecl(e) => {
                let fqn = self.name_ctx.declared_name(&e.name);
                for m in &e.methods {
                    self.check_function(&m.function, Some(&fqn), m.is_static);
                }
            }
            _ => {}
        }
    }
}
