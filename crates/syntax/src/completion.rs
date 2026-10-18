//! How a statement completes, from how the statements inside it complete
//! and what is known of its conditions: whether control can pass out of its
//! end to the statement after it, and whether it leaves the function or a
//! loop. The rules are pure functions; a caller applies them with what it
//! knows of the conditions, by their types or by what the syntax shows.

/// What checking a statement found of how it ends.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Completion {
    /// No path through the statement reaches the statement after it.
    pub terminates: bool,
    /// It terminates in a way that is not a `break`, or a `continue` of one
    /// level: out of the function (`return`, `throw`, a call that never
    /// returns), or out of an enclosing loop or switch (`continue 2`). What
    /// a `switch` asks of its last case: whether control can reach the code
    /// after the switch from it.
    pub exits: bool,
    /// The `break` and `continue` statements whose jump leaves this
    /// statement, by how many enclosing loops or switches beyond the
    /// statement it goes: bit 2k is a `break` and bit 2k + 1 a `continue`
    /// that is k levels beyond the innermost enclosing one, saturating at
    /// 15. A `break` of this statement's own loop is bit 0.
    pub jumps: u32,
}

/// The `break` and the `continue` of the innermost loop or switch.
pub const BREAK_HERE: u32 = 1;
pub const CONTINUE_HERE: u32 = 2;

impl Completion {
    /// The jump a `break` or `continue` of `levels` levels makes: bit 2k for
    /// a `break` and 2k + 1 for a `continue`, k the levels past the first.
    pub fn jump(is_continue: bool, levels: i64) -> u32 {
        let k = u32::try_from(levels.clamp(1, 16) - 1).unwrap_or(0);
        1 << (2 * k + u32::from(is_continue))
    }

    /// The jumps as seen from outside the loop or switch they are in: this
    /// statement's own `break` and `continue` are gone, the rest go one level
    /// less far.
    pub fn jumps_past_one_level(jumps: u32) -> u32 {
        jumps >> 2
    }

    /// A statement that completes and lets control pass to the next.
    pub const FALLS_THROUGH: Completion = Completion {
        terminates: false,
        exits: false,
        jumps: 0,
    };
    /// `return`, `throw`, a call that never returns, `continue 2`.
    pub const EXITS: Completion = Completion {
        terminates: true,
        exits: true,
        jumps: 0,
    };
    /// `break`, `continue`: ends the path, and goes no further than the
    /// switch or loop it is in.
    pub const TERMINATES: Completion = Completion {
        terminates: true,
        exits: false,
        jumps: 0,
    };
}

/// The completion rules of the statements: pure functions over the
/// completions of what is inside them and what is known of their
/// conditions.
impl Completion {
    /// `break` or `continue` of `levels` levels: ends the path and jumps. A
    /// `continue` of more than one level leaves the loop it names.
    pub fn break_(levels: i64) -> Completion {
        Completion {
            jumps: Completion::jump(false, levels),
            ..Completion::TERMINATES
        }
    }

    pub fn continue_(levels: i64) -> Completion {
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
    pub fn if_chain(
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
    pub fn while_loop(condition_always_true: bool, body: Completion) -> Completion {
        Completion {
            terminates: condition_always_true && body.jumps & BREAK_HERE == 0,
            exits: false,
            jumps: Completion::jumps_past_one_level(body.jumps),
        }
    }

    /// `do`-`while`: its body runs once, so it ends the loop when it ends
    /// without a `break` or `continue` of its own, or the condition is
    /// always true and nothing breaks out.
    pub fn do_while(condition_always_true: bool, body: Completion) -> Completion {
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
    pub fn foreach(at_least_once: bool, body: Completion) -> Completion {
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
    pub fn switch(has_default: bool, cases: &[(Completion, bool)]) -> Completion {
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
    pub fn try_catch(
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
pub struct Reach {
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
    pub fn reachable(&self) -> bool {
        self.reachable
    }

    /// A label a `goto` lands on: what follows can be reached.
    pub fn label(&mut self) {
        self.reachable = true;
        self.ends_with = None;
    }

    /// The next statement completes as `completion`.
    pub fn add(&mut self, completion: Completion) {
        self.last = completion;
        if self.reachable {
            self.jumps |= completion.jumps;
            if completion.terminates {
                self.ends_with = Some(completion);
                self.reachable = false;
            }
        }
    }

    pub fn finish(&self) -> BlockCompletion {
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
pub struct BlockCompletion {
    /// The list terminates when any statement in it does, and exits as its
    /// last statement does.
    pub completion: Completion,
    /// The last statement; none for an empty list.
    pub last: Completion,
}
