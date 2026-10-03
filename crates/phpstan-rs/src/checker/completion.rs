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
    /// The `break` and `continue` statements whose jump leaves this
    /// statement, by how many enclosing loops or switches beyond the
    /// statement it goes: bit 2k is a `break` and bit 2k + 1 a `continue`
    /// that is k levels beyond the innermost enclosing one, saturating at
    /// 15. A `break` of this statement's own loop is bit 0.
    pub(crate) jumps: u32,
}

/// The `break` and the `continue` of the innermost loop or switch.
pub(crate) const BREAK_HERE: u32 = 1;
pub(crate) const CONTINUE_HERE: u32 = 2;

impl Completion {
    /// The jump a `break` or `continue` of `levels` levels makes: bit 2k for
    /// a `break` and 2k + 1 for a `continue`, k the levels past the first.
    pub(crate) fn jump(is_continue: bool, levels: i64) -> u32 {
        let k = u32::try_from(levels.clamp(1, 16) - 1).unwrap_or(0);
        1 << (2 * k + u32::from(is_continue))
    }

    /// The jumps as seen from outside the loop or switch they are in: this
    /// statement's own `break` and `continue` are gone, the rest go one level
    /// less far.
    pub(crate) fn jumps_past_one_level(jumps: u32) -> u32 {
        jumps >> 2
    }

    /// A statement that completes and lets control pass to the next.
    pub(crate) const FALLS_THROUGH: Completion = Completion {
        terminates: false,
        exits: false,
        jumps: 0,
    };
    /// `return`, `throw`, a call that never returns, `continue 2`.
    pub(crate) const EXITS: Completion = Completion {
        terminates: true,
        exits: true,
        jumps: 0,
    };
    /// `break`, `continue`: ends the path, and goes no further than the
    /// switch or loop it is in.
    pub(crate) const TERMINATES: Completion = Completion {
        terminates: true,
        exits: false,
        jumps: 0,
    };
}

/// The `goto`s of the function body being checked: the labels they name,
/// and each one that has been checked and not yet landed, with the scope it
/// jumped from, for the label to merge.
#[derive(Default)]
pub(crate) struct GotoState {
    pub(crate) labels: Box<[syntax::ast::GotoLabel]>,
    pub(crate) points: Vec<(syntax::name::Name, Locals)>,
}

impl GotoState {
    pub(crate) fn new(labels: Box<[syntax::ast::GotoLabel]>) -> GotoState {
        GotoState {
            labels,
            points: Vec::new(),
        }
    }

    /// Whether some `goto` of the body names `label`.
    pub(crate) fn is_target(&self, label: &syntax::name::Name) -> bool {
        self.labels.iter().any(|l| l.name == *label)
    }
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
    /// Checks a loop's body and keeps how it completed, for the loop's own
    /// completion: the last pass's is the one that stays.
    pub(crate) fn check_loop_body(
        &mut self,
        body: &Stmt,
        locals: &mut Locals,
        declared_return: Option<&types::Type>,
        label: &str,
    ) {
        self.loop_body_completion = self.check_stmt(body, locals, declared_return, label);
    }

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
        span: syntax::span::Span,
        new_terminates: bool,
    ) {
        if self.completion_oracle {
            let old = body.iter().any(|s| self.always_terminates(s, locals));
            self.note_completion_difference(span, "body terminates", old, new_terminates);
        }
    }

    pub(crate) fn note_completion_difference(
        &mut self,
        span: syntax::span::Span,
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
