use crate::checker::{Checker, ExitPoint, Locals, merge_scopes, purge_stale_narrowing_for_var};
use syntax::ast::*;
use syntax::completion::Completion;
use syntax::span::Span;
use syntax::visit::{Visitor, any_expr_in_scope, walk_expr_in_scope, walk_stmt, walk_stmts};
use types::Type;

impl<'a> Checker<'a> {
    /// Whether some value of type `thrown` can be an instance of one of
    /// `caught`. An unknown class on either side may be.
    fn may_catch(&self, thrown: &Type, caught: &[Type]) -> bool {
        thrown.union_members().iter().any(|t| {
            let Some((t, _)) = t.as_object() else {
                return true;
            };
            caught.iter().any(|c| {
                let Some((c, _)) = c.as_object() else {
                    return true;
                };
                t.eq_ignore_ascii_case(c)
                    || self.registry.is_subclass_of(t, c)
                    || self.registry.is_subclass_of(c, t)
                    || !self.class_name_exists(t)
                    || !self.class_name_exists(c)
            })
        })
    }

    /// Whether every value of type `thrown` is an instance of one of
    /// `caught`.
    fn surely_caught(&self, thrown: &Type, caught: &[Type]) -> bool {
        thrown.union_members().iter().all(|t| {
            let Some((t, _)) = t.as_object() else {
                return false;
            };
            caught.iter().any(|c| {
                c.as_object().is_some_and(|(c, _)| {
                    t.eq_ignore_ascii_case(c) || self.registry.is_subclass_of(t, c)
                })
            })
        })
    }

    pub(crate) fn check_try(
        &mut self,
        body: &[Stmt],
        catches: &[CatchClause],
        finally: &Option<Vec<Stmt>>,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) -> Completion {
        // `finally`'s own real entry scope is never just the try body's
        // full-completion state (merged with each non-terminating
        // catch's own end state, computed below into `locals` before
        // `finally`'s own `check_block` runs) - real PHP can jump into
        // `finally` from any point inside the try body, the moment an
        // exception is thrown, including one thrown before the try
        // body's first assignment ever ran. `check_try` has no real
        // per-statement throw-point model, so a materially cheaper,
        // still-sound approximation is used instead: merge this
        // snapshot (the state genuinely true the instant the try body
        // starts, before anything in it could have run at all) with the
        // try/catch's own normal-completion state right before
        // `finally` runs, rather than trusting only the latter.
        let pre_try_locals = locals.clone();
        // `match.unhandled`'s own real PHPStan `isUnhandledMatchErrorCaught`
        // exemption - pushed before the protected region's own body is
        // checked (so a `match` expression anywhere inside it, at any
        // nesting depth, can see every currently-enclosing `try`'s own
        // catch types), truncated back right after, mirroring this same
        // function's own later `previously_caught`/`class_name_exists`
        // per-catch-type resolution (a malformed/unresolvable catch type
        // is simply skipped here too, not guessed at).
        let catch_types_mark = self.current_catch_types.len();
        let new_catch_types: Vec<Type> = catches
            .iter()
            .flat_map(|c| &c.types)
            .map(|t| self.resolve_name_expr(t))
            .filter(|t| t.as_object().is_some())
            .collect();
        self.current_catch_types.extend(new_catch_types);
        self.try_throw_states.push(Vec::new());
        let body_exits_from = self.current_function_exit_points.len();
        let body_block = self.check_block(body, locals, declared_return, label);
        let throw_states = self.try_throw_states.pop().unwrap_or_default();
        self.current_catch_types.truncate(catch_types_mark);
        let all_catch_types: Vec<Type> = catches
            .iter()
            .flat_map(|c| &c.types)
            .map(|t| self.resolve_name_expr(t))
            .collect();
        // What no catch is sure to catch reaches an enclosing `try`.
        let escaping: Vec<(Type, Locals)> = throw_states
            .iter()
            .filter(|(thrown, _)| !self.surely_caught(thrown, &all_catch_types))
            .cloned()
            .collect();
        if let Some(outer) = self.try_throw_states.last_mut() {
            outer.extend(escaping);
        }
        let try_end_locals = locals.clone();
        let try_terminates = body_block.completion.terminates;
        // How the whole statement completes: the body's last statement and
        // every catch's last must terminate (`finally` runs before either).
        let mut catch_completions: Vec<Completion> = Vec::with_capacity(catches.len());
        // `OverwrittenExitPointByFinallyRule`: gathers every statement the
        // try body and each catch clause leave by - if the `finally` block
        // below also always exits, every one of these gets silently
        // discarded at runtime (real PHP's own well-known gotcha: a
        // `finally`'s own `return`/`throw`/`break`/`continue` always
        // wins, even over an in-flight exception).
        let mut try_catch_exit_points: Vec<ExitPoint> = self.statement_exit_points(body_exits_from);
        // Every reachable state after the whole try/catch - the
        // try body's own full-completion state, plus each catch
        // clause's own ending state, matching every other
        // control-flow join this checker already does for
        // if/else - a branch that always terminates is excluded
        // from the merge exactly the same way there too.
        let mut end_states: Vec<Locals> = if try_terminates {
            Vec::new()
        } else {
            vec![try_end_locals]
        };
        // `CatchWithUnthrownExceptionRule`'s "already caught" half
        // (`catch.alreadyCaught`) - a real, unconditional PHP-level
        // dead-code shape (PHP tries each `catch` clause in source
        // order and stops at the first match, so a later clause every
        // one of whose types is already a subtype of something an
        // earlier clause caught can never actually run) that needs no
        // throw-type tracking at all, only the registry's existing
        // `is_subclass_of` - unlike this same rule's own "never thrown
        // in the try block" half, which needs a real per-callee
        // throw-type model this project doesn't have yet.
        let mut previously_caught: Vec<Type> = Vec::new();
        for c in catches {
            // Scoped to this catch block only, in its own clone
            // rather than the shared `locals` - matches real PHP
            // semantics (the caught variable is unset at the end of
            // its catch block) and keeps one catch's binding from
            // leaking into a sibling catch or the code after the
            // whole try.
            // Every type in a multi-type catch must implement
            // Throwable, independently of whatever the union-binding
            // below does with them - a real, unconditional PHP fatal
            // error at declaration time otherwise ("Cannot catch X
            // because it does not implement Throwable").
            for t in &c.types {
                let resolved = self.resolve_name_expr(t);
                if let Some((class, _)) = resolved.as_object() {
                    // `CaughtExceptionExistenceRule`'s own existence
                    // half, checked first - a typo'd/missing exception
                    // class would otherwise fall straight through to
                    // `is_subclass_of`, which silently treats a name
                    // the registry has no entry for as "not a subclass
                    // of Throwable" (indistinguishable from a real,
                    // existing, non-Throwable class) and reports the
                    // confusingly wrong "does not implement Throwable"
                    // - the `else` here reports at most one diagnostic
                    // per catch type instead of both.
                    if !self.class_name_exists(class) {
                        self.report(
                            format!("caught class {class} not found"),
                            t.span,
                            0,
                            // Real PHPStan's own
                            // `CaughtExceptionExistenceRule` reuses
                            // the generic `class.notFound`
                            // identifier here too - `catch.notThrowable`
                            // just below is the one genuinely
                            // catch-specific identifier this rule has.
                            "class.notFound",
                        );
                    } else {
                        if !self.registry.is_subclass_of(class, "Throwable") {
                            self.report(
                                format!("cannot catch {class}: it does not implement Throwable"),
                                t.span,
                                0,
                                "catch.notThrowable",
                            );
                        }
                        self.check_class_name_case(t);
                        self.check_internal_catch(class, t.span);
                        self.check_restricted_class_name_usage(
                            class,
                            "Catching",
                            t.span,
                            crate::checker::restricted_symbol_usage::ClassUsageLocation::Other,
                        );
                    }
                }
            }
            let clause_types: Vec<Type> =
                c.types.iter().map(|t| self.resolve_name_expr(t)).collect();
            if !clause_types.is_empty()
                && clause_types.iter().all(|ct| {
                    let Some((class, _)) = ct.as_object() else {
                        return false;
                    };
                    previously_caught.iter().any(|pt| {
                        let Some((prev, _)) = pt.as_object() else {
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
                    format!("Dead catch - {desc} is already caught above."),
                    c.span,
                    4,
                    "catch.alreadyCaught",
                );
            }
            previously_caught.extend(clause_types.iter().cloned());
            // Entered from every point in the body throwing something this
            // clause can catch; with none, it cannot run.
            let catch_entry = throw_states
                .iter()
                .filter(|(thrown, _)| self.may_catch(thrown, &clause_types))
                .map(|(_, state)| state)
                .fold(None, |acc: Option<Locals>, state| {
                    Some(match acc {
                        Some(acc) => merge_scopes(&self.relation(), &acc, state),
                        None => state.clone(),
                    })
                });
            let catch_live = catch_entry.is_some();
            let mut catch_locals = catch_entry.unwrap_or_else(|| locals.clone());
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
                    [] => Type::mixed(),
                    [single] => self.resolve_name_expr(single),
                    multiple => self
                        .relation()
                        .union_of(multiple.iter().map(|t| self.resolve_name_expr(t)).collect()),
                };
                purge_stale_narrowing_for_var(&mut catch_locals, var);
                self.probe_hit_at(var.span, &ty);
                catch_locals.declare_var(var, ty, var.span);
            }
            let catch_exits_from = self.current_function_exit_points.len();
            let catch_block = self.check_block(&c.body, &mut catch_locals, declared_return, label);
            try_catch_exit_points.extend(self.statement_exit_points(catch_exits_from));
            if self.settings.extras.check_exception_hygiene {
                self.check_catch_hygiene(c);
            }
            let catch_terminates = catch_block.completion.terminates;
            catch_completions.push(catch_block.completion);
            if !catch_terminates && catch_live {
                end_states.push(catch_locals);
            }
        }
        if let Some(first) = end_states.first().cloned() {
            *locals = end_states
                .into_iter()
                .skip(1)
                .fold(first, |acc, s| merge_scopes(&self.relation(), &acc, &s));
        }
        let mut finally_completion: Option<Completion> = None;
        if let Some(f) = finally {
            // `finally`'s own body is checked against the widened
            // entry scope (see this function's doc comment above), in
            // its own clone - not against `*locals` directly, which
            // stays the real, narrower post-try/catch completion state
            // real code reaching after the whole try/finally statement
            // should see (the widening only exists to make `finally`'s
            // own body type-check soundly against every point it could
            // really be entered from, not to leak imprecision past
            // it). Only a key `finally`'s own body actually reassigns
            // to something different than its own widened entry
            // carries forward onto the narrower state - everything it
            // never touches keeps its narrower, more precise value.
            // (`unset()`-ing a variable inside `finally` won't remove
            // it from the narrower state afterward - a disclosed edge
            // case of this same materially-cheaper approximation.)
            let finally_entry = merge_scopes(&self.relation(), &pre_try_locals, locals);
            let mut finally_locals = finally_entry.clone();
            let finally_exits_from = self.current_function_exit_points.len();
            let finally_block = self.check_block(f, &mut finally_locals, declared_return, label);
            finally_completion = Some(finally_block.completion);
            for (key, ty) in finally_locals.iter() {
                if finally_entry.get(key) != Some(ty) {
                    let provenance = finally_locals
                        .get_provenance(key)
                        .cloned()
                        .unwrap_or_default();
                    locals.insert_with_provenance(key, ty.clone(), provenance);
                }
            }
            // Only worth naming the finally block's own exit point(s)
            // if there's actually something real on the try/catch side
            // for them to overwrite - mirrors
            // `OverwrittenExitPointByFinallyRule`'s own
            // `count($node->getTryCatchExitPoints()) === 0` early
            // return exactly.
            let finally_exit_points = self.statement_exit_points(finally_exits_from);
            if !try_catch_exit_points.is_empty()
                && !finally_exit_points.is_empty()
                && finally_block.completion.terminates
            {
                for exit in &try_catch_exit_points {
                    self.report(
                        format!(
                            "This {} is overwritten by a different one in the finally block below.",
                            exit.describe()
                        ),
                        exit.span,
                        4,
                        "finally.exitPoint",
                    );
                }
                for exit in &finally_exit_points {
                    self.report(
                        format!("The overwriting {} is on this line.", exit.describe()),
                        exit.span,
                        4,
                        "finally.exitPoint",
                    );
                }
            }
        }
        // A `finally` that terminates ends the statement whatever the body
        // and the catches do.
        Completion::try_catch(
            body_block.completion,
            &catch_completions,
            finally_completion,
        )
    }

    pub(crate) fn check_throw(&mut self, e: &Expr, locals: &mut Locals, label: &str) {
        // `throw` only accepts a real Throwable - a real,
        // unconditional PHP fatal error otherwise, independent of
        // anything about the thrown value at runtime.
        // `thrown_value_not_throwable` (`checker/expr.rs`) handles the
        // "don't guess" leniency for anything not concrete enough to
        // judge (Mixed, a dynamic expression), including a
        // `Union`/`Intersection`-typed value.
        let thrown_ty = self.check_expr(e, locals, label);
        if self.settings.extras.check_throw_without_message
            && let ExprKind::New(_, args) = &e.kind
            && args.is_empty()
        {
            self.report(
                "Exception thrown with no message argument.".to_string(),
                e.span,
                5,
                "throw.missingExceptionMessage",
            );
        }
        if self.settings.extras.check_throw_base_class
            && let ExprKind::New(_, _) = &e.kind
            && let Some((class, _)) = thrown_ty.as_object()
            && (class.eq_ignore_ascii_case("Exception") || class.eq_ignore_ascii_case("Error"))
        {
            self.report(format!("Throwing the base {class} class is not recommended, use a specific subclass instead."),
                e.span,
                5,
                "throw.baseClass",
            );
        }
        if let Some(class) = self.thrown_value_not_throwable(&thrown_ty) {
            self.report(
                format!("cannot throw {class}: it does not implement Throwable"),
                e.span,
                0,
                "throw.notThrowable",
            );
        }
        // A real, explicit throw point - pushed unconditionally (even a
        // not-provably-Throwable value, already flagged above in its
        // own right) rather than trying to guess "should this count."
        self.current_function_explicit_throw_points
            .push((thrown_ty.clone(), e.span));
        self.current_function_throw_points.push(thrown_ty);
    }
}

/// Every `throw` in `stmts` with whether it's a direct statement of
/// `stmts` itself, not reaching into nested functions, classes, or
/// nested `catch` clauses (checked on their own).
fn collect_throws(stmts: &[Stmt]) -> Vec<(&Expr, Span, bool)> {
    let mut throws = Throws {
        out: Vec::new(),
        top: true,
    };
    walk_stmts(&mut throws, stmts);
    throws.out
}

struct Throws<'s> {
    out: Vec<(&'s Expr, Span, bool)>,
    // Whether the statement being visited is directly in the list.
    top: bool,
}

impl<'s> Visitor<'s> for Throws<'s> {
    fn visit_stmt(&mut self, stmt: &'s Stmt) {
        let top = std::mem::replace(&mut self.top, false);
        match &stmt.kind {
            StmtKind::Throw(e) => {
                self.out.push((e, stmt.span, top));
                self.visit_expr(e);
            }
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            StmtKind::Try { body, finally, .. } => {
                walk_stmts(self, body);
                if let Some(finally) = finally {
                    walk_stmts(self, finally);
                }
            }
            _ => walk_stmt(self, stmt),
        }
        self.top = top;
    }

    fn visit_expr(&mut self, e: &'s Expr) {
        if let ExprKind::Throw(inner) = &e.kind {
            self.out.push((inner, e.span, false));
        }
        walk_expr_in_scope(self, e);
    }
}

fn mentions_variable(e: &Expr, name: &str) -> bool {
    any_expr_in_scope(
        e,
        |e| matches!(&e.kind, ExprKind::Variable(v) if v.as_ref() == name),
    )
}

const BASE_EXCEPTIONS: [&str; 3] = ["Throwable", "Exception", "Error"];

impl<'a> Checker<'a> {
    /// A `throw new X(...)` inside the catch that doesn't pass the caught
    /// exception on as `X`'s previous exception, and a catch of a base
    /// exception type with no unconditional rethrow.
    fn check_catch_hygiene(&mut self, c: &CatchClause) {
        let throws = collect_throws(&c.body);
        if let Some(var) = &c.var {
            for (thrown, span, _) in &throws {
                let ExprKind::New(class, args) = &thrown.kind else {
                    continue;
                };
                if args.iter().any(|a| mentions_variable(&a.value, var)) {
                    continue;
                }
                let ExprKind::Name(raw) = &class.kind else {
                    continue;
                };
                let resolved = self.resolve_name_expr(raw);
                let Some((class, _)) = resolved.as_object() else {
                    continue;
                };
                let accepts_previous = self
                    .registry
                    .lookup_method(class, "__construct")
                    .is_some_and(|ctor| {
                        ctor.params().iter().any(|p| {
                            p.name().eq_ignore_ascii_case("previous")
                                || p.ty().object_class_names().into_iter().any(|n| {
                                    self.registry.is_subclass_of(n, "Throwable") || n == "Throwable"
                                })
                        })
                    });
                if accepts_previous {
                    self.report(format!(
                            "{class} is thrown from the catch of ${var} without ${var} as its previous exception; the original cause and trace are lost."
                        ),
                        *span,
                        4,
                        "catch.previousNotPassed",
                    );
                }
            }
        }
        let base = c.types.iter().find_map(|t| {
            let resolved = self.resolve_name_expr(t);
            let (class, _) = resolved.as_object()?;
            BASE_EXCEPTIONS
                .iter()
                .any(|b| class.eq_ignore_ascii_case(b))
                .then(|| class.clone())
        });
        if let Some(base) = base
            && !throws.iter().any(|(_, _, top)| *top)
        {
            self.report(format!("Catching {base} swallows every such exception unless the catch rethrows; add an unconditional throw."),
                c.span,
                4,
                "catch.baseWithoutRethrow",
            );
        }
    }
}
