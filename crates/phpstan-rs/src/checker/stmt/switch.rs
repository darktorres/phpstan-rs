use crate::checker::{BREAK_HERE, CONTINUE_HERE, Checker, Completion, Locals, merge_scopes};
use syntax::ast::*;
use types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_switch(
        &mut self,
        subject: &Expr,
        cases: &[SwitchCase],
        span: syntax::span::Span,
        locals: &mut Locals,
        declared_return: Option<&Type>,
        label: &str,
    ) -> Completion {
        self.check_expr(subject, locals, label);
        if self.settings.extras.check_switch_missing_default
            && !cases.iter().any(|c| c.test.is_none())
        {
            self.report(
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
        // How each case body's last statement completes, for the switch's
        // own completion.
        let mut case_lasts: Vec<Completion> = Vec::with_capacity(case_count);
        let mut case_jumps = 0;
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
            // A `case` compares loosely (`case 0:` also runs for `'0'`).
            if let Some(t) = &case.test {
                self.apply_loose_equality_narrowing(subject, t, &mut direct, true);
            }
            let mut case_locals = if previous_terminates {
                direct
            } else {
                merge_scopes(&self.relation(), &running, &direct)
            };
            let block = self.check_block(&case.body, &mut case_locals, declared_return, label);
            let last_stmt = case.body.last();
            let case_terminates = block.completion.terminates;
            case_lasts.push(block.completion);
            case_jumps |= block.completion.jumps;
            if let Some(s) = last_stmt {
                self.compare_completion(s, &case_locals, block.last);
            }
            if let Some(t) = &case.test {
                self.apply_loose_equality_narrowing(subject, t, &mut narrowed_locals, false);
            }
            let is_last = i + 1 == case_count;
            // Control passes the switch from a case that breaks, or that is
            // the last and falls off its end.
            let reaches_after_switch = block.completion.jumps & (BREAK_HERE | CONTINUE_HERE) != 0
                || (is_last && !block.completion.terminates);
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
                .fold(first, |acc, s| merge_scopes(&self.relation(), &acc, &s));
        }
        // A `switch` guarantees ending control flow only with a `default`
        // and when every non-empty case body's last statement terminates by
        // something other than a `break` (an empty body falls into the next
        // non-empty one, and a trailing empty one falls out of the bottom).
        let ends = |at: usize| {
            !cases[at].body.is_empty()
                && case_lasts[at].terminates
                && case_lasts[at].jumps & (BREAK_HERE | CONTINUE_HERE) == 0
        };
        let terminates = cases.iter().any(|c| c.test.is_none())
            && (0..case_count).all(|i| {
                if !cases[i].body.is_empty() {
                    return ends(i);
                }
                (i + 1..case_count)
                    .find(|&later| !cases[later].body.is_empty())
                    .is_some_and(ends)
            });
        Completion {
            terminates,
            exits: false,
            // A `break` or a `continue` of one level ends the switch.
            jumps: Completion::jumps_past_one_level(case_jumps),
        }
    }
}
