use crate::ast::*;
use crate::checker::{
    Checker, Locals, collect_exit_point_statements, exit_point_description, merge_scopes,
    purge_stale_narrowing_for,
};
use crate::types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_try(
        &mut self,
        body: &[Stmt],
        catches: &[CatchClause],
        finally: &Option<Vec<Stmt>>,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
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
        self.check_block(body, locals, declared_return, label);
        let try_end_locals = locals.clone();
        let try_terminates = body
            .last()
            .is_some_and(|s| self.always_terminates(s, &try_end_locals));
        // `OverwrittenExitPointByFinallyRule`: gathers every concrete
        // exit-point statement from the try body and each catch clause
        // that itself always terminates - if the `finally` block below
        // also unconditionally exits, every one of these gets silently
        // discarded at runtime (real PHP's own well-known gotcha: a
        // `finally`'s own `return`/`throw`/`break`/`continue` always
        // wins, even over an in-flight exception). See
        // `collect_exit_point_statements`'s doc comment for why this
        // only names the common `Block`/`If`-nested shapes rather than
        // every construct `always_terminates` itself recognizes.
        let mut try_catch_exit_points: Vec<&Stmt> = Vec::new();
        if try_terminates && let Some(s) = body.last() {
            try_catch_exit_points.extend(collect_exit_point_statements(s));
        }
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
            for (i, t) in c.types.iter().enumerate() {
                if let Type::Object(class, _) = self.resolve_name_expr(t) {
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
                    if !self.class_name_exists(&class) {
                        self.report(
                            label.to_string(),
                            format!("caught class {class} not found"),
                            c.span,
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
                        if !self.registry.is_subclass_of(&class, "Throwable") {
                            self.report(
                                label.to_string(),
                                format!("cannot catch {class}: it does not implement Throwable"),
                                c.span,
                                0,
                                "catch.notThrowable",
                            );
                        }
                        // `class.nameCase` - `t` is the raw, as-written
                        // text; `types_spans` carries one span per
                        // `types` entry, same index (`FunctionDecl::
                        // return_type_span`'s own precedent).
                        let span = c.types_spans.get(i).copied().unwrap_or(c.span);
                        self.check_class_name_case(label, t, span);
                        self.check_internal_catch(&class, c.span, label);
                        self.check_restricted_class_name_usage(&class, "Catching", c.span, label);
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
                    multiple => {
                        Type::Union(multiple.iter().map(|t| self.resolve_name_expr(t)).collect())
                    }
                };
                purge_stale_narrowing_for(&mut catch_locals, var);
                // Real declaration - `var_span` is always `Some`
                // exactly when `var` is (the parser sets both
                // together), `c.span` is a defensive fallback only.
                let var_span = c.var_span.unwrap_or(c.span);
                self.probe_hit_at(var_span, &ty);
                catch_locals.declare(var.clone(), ty, var_span);
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
            let finally_entry = merge_scopes(&pre_try_locals, locals);
            let mut finally_locals = finally_entry.clone();
            self.check_block(f, &mut finally_locals, declared_return, label);
            for (key, ty) in finally_locals.iter() {
                if finally_entry.get(key) != Some(ty) {
                    let provenance = finally_locals
                        .get_provenance(key)
                        .cloned()
                        .unwrap_or_default();
                    locals.insert_with_provenance(key.clone(), ty.clone(), provenance);
                }
            }
            // Only worth naming the finally block's own exit point(s)
            // if there's actually something real on the try/catch side
            // for them to overwrite - mirrors
            // `OverwrittenExitPointByFinallyRule`'s own
            // `count($node->getTryCatchExitPoints()) === 0` early
            // return exactly.
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

    pub(crate) fn check_throw(&mut self, e: &Expr, locals: &mut Locals, label: &str) {
        // `throw` only accepts a real Throwable - a real,
        // unconditional PHP fatal error otherwise, independent of
        // anything about the thrown value at runtime.
        // `thrown_value_not_throwable` (`checker/expr.rs`) handles the
        // "don't guess" leniency for anything not concrete enough to
        // judge (Mixed, a dynamic expression), including a
        // `Union`/`Intersection`-typed value.
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
        // A real, explicit throw point - pushed unconditionally (even a
        // not-provably-Throwable value, already flagged above in its
        // own right) rather than trying to guess "should this count."
        self.current_function_explicit_throw_points
            .push((thrown_ty.clone(), e.span));
        self.current_function_throw_points.push(thrown_ty);
    }
}
