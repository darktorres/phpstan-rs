use crate::ast::*;
use crate::checker::{Checker, Locals, is_switch_break_equivalent, merge_scopes};
use crate::types::Type;

impl<'a> Checker<'a> {
    #[hotpath::measure]
    pub(crate) fn check_switch(
        &mut self,
        subject: &Expr,
        cases: &[SwitchCase],
        span: crate::token::Span,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        self.check_expr(subject, locals, label);
        if self.registry.check_switch_missing_default && !cases.iter().any(|c| c.test.is_none()) {
            self.report(
                label.to_string(),
                "Switch has no default branch.".to_string(),
                span,
                5,
                "switch.missingDefault",
            );
        }
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
        // Every case is reachable by direct dispatch, checked
        // against `narrowed_locals` plus its own positive "subject
        // === test" narrowing. A case whose predecessor doesn't
        // terminate is also reachable by falling through, so its
        // entry state is the merge of both paths - stacked labels
        // (`case 'a': case 'b':`) see the subject as `'a'|'b'`.
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
        // A bare `break;` written directly inside a case targets
        // this switch, not any enclosing loop - pushing a frame
        // here (discarded below, never read) only exists to
        // intercept it, since `loop_break_states`'s depth-counting
        // would otherwise attribute it to whichever loop happens to
        // be open around this switch. This switch's own
        // `exit_states` mechanism above already handles `break`
        // completely on its own.
        //
        // `loop_continue_states` gets the identical push/pop, for
        // the identical reason - real PHP's own `continue`/`break`
        // both count a `switch` as one level (the well-known "bare
        // `continue;` directly inside a `switch` behaves like
        // `break;`" gotcha), so the two stacks must stay
        // depth-aligned or `continue N`'s own frame-index math
        // would attribute a level to the wrong enclosing loop.
        self.loop_break_states.push(Vec::new());
        self.loop_continue_states.push(Vec::new());
        for (i, case) in cases.iter().enumerate() {
            if let Some(t) = &case.test {
                self.check_expr(t, locals, label);
            }
            let mut direct = narrowed_locals.clone();
            if let Some(t) = &case.test {
                self.apply_equality_narrowing(subject, t, &mut direct, true);
            }
            let mut case_locals = if previous_terminates {
                direct
            } else {
                merge_scopes(&running, &direct)
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
}
