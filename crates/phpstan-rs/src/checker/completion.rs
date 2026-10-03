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

/// The kind of statement a path leaves its construct by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ExitKind {
    Return,
    Throw,
    /// A `throw` that is an expression (`$x ?? throw new E()`), which no
    /// statement is left by.
    ThrowExpr,
    /// A `break` of this many levels.
    Break(u32),
    /// A `continue` of this many levels.
    Continue(u32),
}

/// A statement the path of the function body being checked leaves by, and
/// has not yet been settled: a `break` that no loop or switch has taken
/// has not left anything yet.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExitPoint {
    pub(crate) kind: ExitKind,
    pub(crate) span: syntax::span::Span,
}

impl ExitPoint {
    /// Whether the statement is the `break` of this many levels, or a
    /// `continue` past the innermost loop, a `return` or a `throw`: what
    /// lets a loop whose condition is true end otherwise than by the
    /// condition.
    pub(crate) fn lets_a_loop_end(&self) -> bool {
        !matches!(self.kind, ExitKind::Continue(1) | ExitKind::ThrowExpr)
    }

    /// The statement as a message names it.
    pub(crate) fn describe(&self) -> &'static str {
        match self.kind {
            ExitKind::Return => "return",
            ExitKind::Throw | ExitKind::ThrowExpr => "throw",
            ExitKind::Break(_) => "break",
            ExitKind::Continue(_) => "continue",
        }
    }
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
    /// Where the current function body first throws, by a statement or an
    /// expression, once it has been checked.
    pub(crate) fn first_explicit_throw(&self) -> Option<syntax::span::Span> {
        self.current_function_exit_points
            .iter()
            .find(|point| matches!(point.kind, ExitKind::Throw | ExitKind::ThrowExpr))
            .map(|point| point.span)
    }

    /// The statements the path leaves by since `from`: not a `throw`
    /// that is an expression.
    pub(crate) fn statement_exit_points(&self, from: usize) -> Vec<ExitPoint> {
        self.current_function_exit_points[from..]
            .iter()
            .filter(|point| point.kind != ExitKind::ThrowExpr)
            .copied()
            .collect()
    }

    /// Notes that the path through `span` leaves by `kind`.
    pub(crate) fn push_exit_point(&mut self, kind: ExitKind, span: syntax::span::Span) {
        self.current_function_exit_points
            .push(ExitPoint { kind, span });
    }

    /// A loop or a switch has been checked, and was the one that the
    /// `break`s and `continue`s of one level from `from` on went to: they
    /// leave nothing, and the deeper ones go one level less far.
    pub(crate) fn settle_exit_points(&mut self, from: usize) {
        let mut at = from;
        while at < self.current_function_exit_points.len() {
            let kind = self.current_function_exit_points[at].kind;
            let one_level_less = match kind {
                ExitKind::Break(1) | ExitKind::Continue(1) => {
                    self.current_function_exit_points.remove(at);
                    continue;
                }
                ExitKind::Break(levels) => Some(ExitKind::Break(levels - 1)),
                ExitKind::Continue(levels) => Some(ExitKind::Continue(levels - 1)),
                ExitKind::Return | ExitKind::Throw | ExitKind::ThrowExpr => None,
            };
            if let Some(kind) = one_level_less {
                self.current_function_exit_points[at].kind = kind;
            }
            at += 1;
        }
    }

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
}
