//! How a statement completes, built while it is checked, from how the
//! statements inside it complete: whether control can pass out of its end
//! to the statement after it, and whether it leaves the function or loop.

use syntax::ast::StmtNode;

use super::{Checker, Locals};

/// What checking a statement found of how it ends.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
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

/// The completion rules of the statements: pure functions over the
/// completions of what is inside them and what is known of their
/// conditions. The checker applies them with what it infers, and the flow
/// of a body (`registry::init_flow`) with what the syntax shows.
impl Completion {
    /// `break` or `continue` of `levels` levels: ends the path and jumps. A
    /// `continue` of more than one level leaves the loop it names.
    pub(crate) fn break_(levels: i64) -> Completion {
        Completion {
            jumps: Completion::jump(false, levels),
            ..Completion::TERMINATES
        }
    }

    pub(crate) fn continue_(levels: i64) -> Completion {
        Completion {
            jumps: Completion::jump(true, levels),
            ..if levels > 1 {
                Completion::EXITS
            } else {
                Completion::TERMINATES
            }
        }
    }

    /// An `if` chain: its then branch and the rest of the chain, or the
    /// `else`, which is a path that falls through when there is none. A
    /// condition known to be true leaves only the branch it picks, and one
    /// known false only the rest of the chain. Otherwise the chain
    /// terminates when every branch does, an `else` among them, and exits
    /// when every branch exits.
    pub(crate) fn if_chain(
        then: Completion,
        otherwise: Completion,
        known_condition: Option<bool>,
    ) -> Completion {
        match known_condition {
            Some(true) => then,
            Some(false) => otherwise,
            None => Completion {
                terminates: then.terminates && otherwise.terminates,
                exits: then.exits && otherwise.exits,
                jumps: then.jumps | otherwise.jumps,
            },
        }
    }

    /// `while`, or `for`: a loop whose condition is always true ends only by
    /// a `break` of its own.
    pub(crate) fn while_loop(condition_always_true: bool, body: Completion) -> Completion {
        Completion {
            terminates: condition_always_true && body.jumps & BREAK_HERE == 0,
            exits: false,
            jumps: Completion::jumps_past_one_level(body.jumps),
        }
    }

    /// `do`-`while`: its body runs once, so it ends the loop when it ends
    /// without a `break` or `continue` of its own, or the condition is
    /// always true and nothing breaks out.
    pub(crate) fn do_while(condition_always_true: bool, body: Completion) -> Completion {
        Completion {
            terminates: (body.terminates && body.jumps & (BREAK_HERE | CONTINUE_HERE) == 0)
                || (condition_always_true && body.jumps & BREAK_HERE == 0),
            exits: false,
            jumps: Completion::jumps_past_one_level(body.jumps),
        }
    }

    /// `foreach`: a subject that is not empty runs its body at least once,
    /// and a body that ends every time, without a `break` or `continue` of
    /// its own, ends the loop.
    pub(crate) fn foreach(at_least_once: bool, body: Completion) -> Completion {
        Completion {
            terminates: at_least_once
                && body.terminates
                && body.jumps & (BREAK_HERE | CONTINUE_HERE) == 0,
            exits: false,
            jumps: Completion::jumps_past_one_level(body.jumps),
        }
    }

    /// `switch`: it ends the path only with a `default` and when every
    /// non-empty case body ends by something other than a `break` (an empty
    /// body falls into the next non-empty one, and a trailing empty one falls
    /// out of the bottom). `cases` are each case's completion and whether
    /// its body has no statement.
    pub(crate) fn switch(has_default: bool, cases: &[(Completion, bool)]) -> Completion {
        let ends = |at: usize| {
            let (completion, empty) = cases[at];
            !empty && completion.terminates && completion.jumps & (BREAK_HERE | CONTINUE_HERE) == 0
        };
        let terminates = has_default
            && (0..cases.len()).all(|i| {
                if !cases[i].1 {
                    return ends(i);
                }
                (i + 1..cases.len())
                    .find(|&later| !cases[later].1)
                    .is_some_and(ends)
            });
        let jumps = cases.iter().fold(0, |jumps, (c, _)| jumps | c.jumps);
        Completion {
            terminates,
            exits: false,
            // A `break` or a `continue` of one level ends the switch.
            jumps: Completion::jumps_past_one_level(jumps),
        }
    }

    /// `try`: the body and every `catch` terminating ends the path, and a
    /// `finally` that terminates ends it whatever the others do. The jumps
    /// are those of all of them.
    pub(crate) fn try_catch(
        body: Completion,
        catches: &[Completion],
        finally: Option<Completion>,
    ) -> Completion {
        let jumps = catches.iter().fold(body.jumps, |jumps, c| jumps | c.jumps)
            | finally.map_or(0, |f| f.jumps);
        match finally {
            Some(f) if f.terminates => Completion {
                terminates: true,
                exits: f.exits,
                jumps,
            },
            _ => Completion {
                terminates: body.terminates && catches.iter().all(|c| c.terminates),
                exits: body.exits && catches.iter().all(|c| c.exits),
                jumps,
            },
        }
    }
}

/// The reach of the statements of a block, as their completions are added:
/// one after a statement that terminates is unreachable, unless a label a
/// `goto` lands on starts the reachable stretch again.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reach {
    /// The first statement that terminates in the last stretch that can be
    /// reached.
    ends_with: Option<Completion>,
    last: Completion,
    /// The jumps of the statements that can be reached.
    jumps: u32,
    reachable: bool,
}

impl Default for Reach {
    fn default() -> Reach {
        Reach {
            ends_with: None,
            last: Completion::FALLS_THROUGH,
            jumps: 0,
            reachable: true,
        }
    }
}

impl Reach {
    /// Whether the next statement can be reached.
    pub(crate) fn reachable(&self) -> bool {
        self.reachable
    }

    /// A label a `goto` lands on: what follows can be reached.
    pub(crate) fn label(&mut self) {
        self.reachable = true;
        self.ends_with = None;
    }

    /// The next statement completes as `completion`.
    pub(crate) fn add(&mut self, completion: Completion) {
        self.last = completion;
        if self.reachable {
            self.jumps |= completion.jumps;
            if completion.terminates {
                self.ends_with = Some(completion);
                self.reachable = false;
            }
        }
    }

    pub(crate) fn finish(&self) -> BlockCompletion {
        BlockCompletion {
            completion: Completion {
                terminates: self.ends_with.is_some(),
                exits: self.ends_with.is_some_and(|c| c.exits),
                jumps: self.jumps,
            },
            last: self.last,
        }
    }
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
        body: &StmtNode,
        locals: &mut Locals,
        declared_return: Option<&types::Type>,
        label: &str,
    ) {
        self.loop_body_completion = self.check_stmt(body, locals, declared_return, label);
    }
}

syntax::canonical_struct!(Completion {
    terminates,
    exits,
    jumps,
});
