//! How a statement completes, built while it is checked, from how the
//! statements inside it complete: whether control can pass out of its end
//! to the statement after it, and whether it leaves the function or loop.

use syntax::ast::Stmt;

use super::{Checker, Locals};

/// What checking a statement found of how it ends.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct Completion {
    /// No path through the statement reaches the statement after it.
    pub(crate) terminates: bool,
    /// It terminates in a way that is not a `break`, or a `continue` of one
    /// level: out of the function (`return`, `throw`, a call that never
    /// returns), or out of an enclosing loop or switch (`continue 2`). What
    /// a `switch` asks of its last case: whether control can reach the code
    /// after the switch from it.
    pub(crate) exits: bool,
}

impl Completion {
    /// A statement that completes and lets control pass to the next.
    pub(crate) const FALLS_THROUGH: Completion = Completion {
        terminates: false,
        exits: false,
    };
    /// `return`, `throw`, a call that never returns, `continue 2`.
    pub(crate) const EXITS: Completion = Completion {
        terminates: true,
        exits: true,
    };
    /// `break`, `continue`: ends the path, and goes no further than the
    /// switch or loop it is in.
    pub(crate) const TERMINATES: Completion = Completion {
        terminates: true,
        exits: false,
    };
}

/// What checking a list of statements found: how the list as a whole
/// completes, and how its last statement does.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct BlockCompletion {
    /// The list terminates when any statement in it does, and exits as its
    /// last statement does.
    pub(crate) completion: Completion,
    /// The last statement; none for an empty list.
    pub(crate) last: Completion,
}

impl Checker<'_> {
    /// Compares a statement's completion, built while it was checked, with
    /// what `always_terminates` and `exits_function_or_loop` say of it after:
    /// a difference is reported as `oracle.completion`. Only when the oracle
    /// is on (`PHPSTAN_RS_COMPLETION_ORACLE`), as a test build of the change.
    pub(crate) fn compare_completion(&mut self, stmt: &Stmt, locals: &Locals, new: Completion) {
        if !self.completion_oracle {
            return;
        }
        let old_terminates = self.always_terminates(stmt, locals);
        let old_exits = self.exits_function_or_loop(stmt, locals);
        self.note_completion_difference(stmt.span, "terminates", old_terminates, new.terminates);
        self.note_completion_difference(stmt.span, "exits", old_exits, new.exits);
    }

    /// `compare_completion` for a body: whether any of its statements
    /// terminates, as `always_terminates` says of them in `locals`.
    pub(crate) fn compare_body_completion(
        &mut self,
        body: &[Stmt],
        locals: &Locals,
        span: syntax::token::Span,
        new_terminates: bool,
    ) {
        if self.completion_oracle {
            let old = body.iter().any(|s| self.always_terminates(s, locals));
            self.note_completion_difference(span, "body terminates", old, new_terminates);
        }
    }

    pub(crate) fn note_completion_difference(
        &mut self,
        span: syntax::token::Span,
        what: &str,
        old: bool,
        new: bool,
    ) {
        if old != new {
            self.report(
                format!("completion oracle: {what} was {old}, is {new}"),
                span,
                0,
                "oracle.completion",
            );
        }
    }
}
