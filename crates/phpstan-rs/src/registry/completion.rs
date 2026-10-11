//! The completion of a statement as the registry derives it from the
//! syntax alone (`init_flow`) and as the checker builds it: whether control
//! can pass out of a statement's end to the one after it, and whether it
//! leaves the function or loop. These are plain values, so they live with
//! the reflection layer that also uses them, and the checker re-exports them.

use syntax::ast::{ExprKind, ExprNode, Literal};

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

/// Real PHP's own two spellings for "this condition is always true":
/// the literal `true`, and any non-zero integer literal (`while (1)`,
/// a real, common pre-`true`-keyword-idiom carryover). Deliberately not
/// attempting anything symbolic beyond a bare literal - a variable or
/// expression that merely *evaluates* to a truthy constant isn't
/// something this checker tracks precisely enough to trust here.
pub(crate) fn is_truthy_literal(e: &ExprNode) -> bool {
    match &e.kind {
        ExprKind::Literal(Literal::Bool(true)) => true,
        ExprKind::Literal(Literal::Int(n)) => *n != 0,
        _ => false,
    }
}

syntax::canonical_struct!(Completion {
    terminates,
    exits,
    jumps,
});
