//! What the uninitialized-property dataflow reads of a method body: its
//! control structure - blocks, branches, loops, `switch`, `try` - holding
//! only the events the dataflow follows, in the order PHP evaluates them:
//! reads, writes and `unset`s of `$this` properties, and calls on `$this`,
//! `self`, `static` and `parent` with the name of the method they call.
//!
//! The flow is made once for every method of a file, by one walk of the
//! method (on whichever thread parses the file), so that following a call
//! into another method needs the flow of that method and not its statements.
//! A part of a body with no event in it is left out: it changes nothing the
//! dataflow tracks.

use super::completion::{Completion, Reach, is_truthy_literal};
use syntax::ast::*;
use syntax::name::Name;
use syntax::span::Span;
use syntax::visit::arena::{self, Visitor};

/// One thing a body does that the dataflow follows.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Event {
    /// A read of `$this->prop`, which is a premature one when the property
    /// is not initialized yet.
    Read(Name, Span),
    /// An assignment to `$this->prop`.
    Write(Name),
    /// `unset($this->prop)`.
    Unset(Name),
    /// A call that goes into the body of a method of the class being
    /// checked.
    Call(Call),
}

/// A call into another method of the same object or class.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Call {
    pub(crate) receiver: CallReceiver,
    pub(crate) name: Name,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum CallReceiver {
    /// `$this->name()`, or the end of a chain `$this->a()->b()->name()`:
    /// the names of the calls before it, which each have to return `self`
    /// or `static` for the chain to still be `$this`.
    This(Box<[Name]>),
    /// `self::name()`.
    SelfKeyword,
    /// `static::name()`.
    StaticKeyword,
    /// `parent::name()`.
    ParentKeyword,
}

/// One part of a body, in the order it runs.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Flow {
    Events(Box<[Event]>),
    /// An `if`, its `elseif`s and its `else`.
    If(IfChain),
    /// `while`, whose body may run zero times.
    While {
        cond: Box<[Event]>,
        body: Box<[Flow]>,
    },
    /// `for`: what runs before the first test, the body, and the step after
    /// it.
    For {
        before: Box<[Event]>,
        body: Box<[Flow]>,
        step: Box<[Event]>,
    },
    Foreach {
        subject: Box<[Event]>,
        body: Box<[Flow]>,
    },
    Switch {
        subject: Box<[Event]>,
        cases: Box<[Case]>,
    },
    Try {
        body: Box<[Flow]>,
        catches: Box<[Box<[Flow]>]>,
        finally: Option<Box<[Flow]>>,
    },
}

/// A condition and what it guards, with whether the body unconditionally
/// leaves the method: a branch that does not vote on what is true after
/// the chain.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct IfBranch {
    pub(crate) cond: Box<[Event]>,
    pub(crate) body: Box<[Flow]>,
    pub(crate) terminates: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct IfChain {
    pub(crate) branches: Box<[IfBranch]>,
    pub(crate) else_body: Option<(Box<[Flow]>, bool)>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Case {
    pub(crate) test: Box<[Event]>,
    pub(crate) body: Box<[Flow]>,
    /// How the case's statements complete: whether control can pass from
    /// the end of the body into the next case, and whether it leaves the
    /// switch by a `break`.
    pub(crate) completion: Completion,
}

/// The flow of one body.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InitFlow {
    pub(crate) nodes: Box<[Flow]>,
}

impl InitFlow {
    /// The flow of the statements of one body, whose `goto`s name `labels`.
    pub fn of(arenas: &NodeArenas, body: &[StmtId], labels: &[GotoLabel]) -> InitFlow {
        InitFlow {
            nodes: flow_of_stmts(arenas, body, labels).into_boxed_slice(),
        }
    }
}

fn is_this(arenas: &NodeArenas, id: ExprId) -> bool {
    arenas
        .expr(id)
        .is_some_and(|e| matches!(&e.kind, ExprKind::Variable(v) if **v == *"this"))
}

/// How a statement completes, as far as the syntax shows: the rules the
/// checker applies, with what can be told without inferring a type - a
/// literal condition, `return`, `throw`, `exit`, `break`, `continue`.
/// `labels` are the labels the `goto`s of the body land on.
fn completion_of(arenas: &NodeArenas, id: StmtId, labels: &[GotoLabel]) -> Completion {
    let Some(stmt) = arenas.stmt(id) else {
        return Completion::FALLS_THROUGH;
    };
    syntax::stack::guard(|| match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) => Completion::EXITS,
        StmtKind::Break(level) => Completion::break_(level.unwrap_or(1)),
        StmtKind::Continue(level) => Completion::continue_(level.unwrap_or(1)),
        // Nothing after a `goto` is reached by falling through.
        StmtKind::Goto(_) => Completion::TERMINATES,
        StmtKind::Expression(e)
            if arenas
                .peeled(*e)
                .and_then(|id| arenas.expr(id))
                .is_some_and(|e| matches!(&e.kind, ExprKind::Exit(..) | ExprKind::Throw(_))) =>
        {
            Completion::EXITS
        }
        StmtKind::Block(stmts) => block_completion(arenas, arenas.stmt_ids(stmts), labels),
        StmtKind::Declare {
            body: Some(stmts), ..
        } => block_completion(arenas, arenas.stmt_ids(stmts), labels),
        StmtKind::If(if_stmt) => {
            let otherwise = if_stmt.else_branch.map_or(Completion::FALLS_THROUGH, |e| {
                completion_of(arenas, e, labels)
            });
            let branches: Vec<(ExprId, StmtId)> =
                std::iter::once((if_stmt.cond, if_stmt.then_branch))
                    .chain(
                        arenas
                            .elseifs(if_stmt.elseifs.clone())
                            .iter()
                            .map(|e| (e.cond, e.body)),
                    )
                    .collect();
            branches
                .into_iter()
                .rev()
                .fold(otherwise, |rest, (cond, body)| {
                    Completion::if_chain(
                        completion_of(arenas, body, labels),
                        rest,
                        known_condition(arenas, cond),
                    )
                })
        }
        StmtKind::While { cond, body } => Completion::while_loop(
            arenas.expr(*cond).is_some_and(is_truthy_literal),
            completion_of(arenas, *body, labels),
        ),
        StmtKind::DoWhile { body, cond } => Completion::do_while(
            arenas.expr(*cond).is_some_and(is_truthy_literal),
            completion_of(arenas, *body, labels),
        ),
        StmtKind::For { cond, body, .. } => Completion::while_loop(
            arenas
                .expr_ids(cond)
                .last()
                .is_none_or(|id| arenas.expr(*id).is_some_and(is_truthy_literal)),
            completion_of(arenas, *body, labels),
        ),
        StmtKind::Foreach { body, .. } => {
            Completion::foreach(false, completion_of(arenas, *body, labels))
        }
        StmtKind::Switch { cases, .. } => {
            let cases = arenas.switch_cases(cases.clone());
            let completions: Vec<(Completion, bool)> = cases
                .iter()
                .map(|case| {
                    (
                        block_completion(arenas, arenas.stmt_ids(&case.body), labels),
                        arenas.stmt_ids(&case.body).is_empty(),
                    )
                })
                .collect();
            Completion::switch(cases.iter().any(|c| c.test.is_none()), &completions)
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            let catches: Vec<Completion> = arenas
                .catch_clauses(catches.clone())
                .iter()
                .map(|c| block_completion(arenas, arenas.stmt_ids(&c.body), labels))
                .collect();
            let finally = finally
                .as_ref()
                .map(|f| block_completion(arenas, arenas.stmt_ids(f), labels));
            Completion::try_catch(
                block_completion(arenas, arenas.stmt_ids(body), labels),
                &catches,
                finally,
            )
        }
        _ => Completion::FALLS_THROUGH,
    })
}

/// The completion of a statement list: what follows a statement that
/// terminates is unreachable until a label a `goto` lands on.
fn block_completion(arenas: &NodeArenas, stmts: &[StmtId], labels: &[GotoLabel]) -> Completion {
    let mut reach = Reach::default();
    for &id in stmts {
        let Some(stmt) = arenas.stmt(id) else {
            continue;
        };
        if matches!(&stmt.kind, StmtKind::Label(name) if labels.iter().any(|l| l.name == name.name))
        {
            reach.label();
        }
        reach.add(completion_of(arenas, id, labels));
    }
    reach.finish().completion
}

/// A condition that is a literal: always true, always false, or neither
/// known.
fn known_condition(arenas: &NodeArenas, id: ExprId) -> Option<bool> {
    let cond = arenas.expr(id)?;
    match &cond.kind {
        _ if is_truthy_literal(cond) => Some(true),
        ExprKind::Literal(Literal::Bool(false)) | ExprKind::Literal(Literal::Int(0)) => Some(false),
        _ => None,
    }
}

/// The flow of a statement list: a nested block is part of it, and what has
/// no event in it is not.
fn flow_of_stmts(arenas: &NodeArenas, stmts: &[StmtId], labels: &[GotoLabel]) -> Vec<Flow> {
    let mut out = Vec::new();
    for &stmt in stmts {
        flow_of_stmt(arenas, stmt, labels, &mut out);
    }
    out
}

fn events_of_each(arenas: &NodeArenas, exprs: impl IntoIterator<Item = ExprId>) -> Vec<Event> {
    let mut events = Vec::new();
    for e in exprs {
        events_of(arenas, e, &mut events);
    }
    events
}

/// `events` as a flow, when there are any.
fn push_events(out: &mut Vec<Flow>, events: Vec<Event>) {
    if !events.is_empty() {
        out.push(Flow::Events(events.into_boxed_slice()));
    }
}

fn flow_of_stmt(arenas: &NodeArenas, id: StmtId, labels: &[GotoLabel], out: &mut Vec<Flow>) {
    let Some(stmt) = arenas.stmt(id) else {
        return;
    };
    syntax::stack::guard(|| match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => {
            push_events(out, events_of_each(arenas, [*e]))
        }
        StmtKind::Return(Some(e)) => push_events(out, events_of_each(arenas, [*e])),
        StmtKind::Echo(exprs) => push_events(
            out,
            events_of_each(arenas, arenas.expr_ids(exprs).iter().copied()),
        ),
        StmtKind::StaticVar(vars) => push_events(
            out,
            events_of_each(
                arenas,
                arenas
                    .static_vars(vars.clone())
                    .iter()
                    .filter_map(|variable| variable.default),
            ),
        ),
        StmtKind::Block(stmts) => {
            for &stmt in arenas.stmt_ids(stmts) {
                flow_of_stmt(arenas, stmt, labels, out);
            }
        }
        StmtKind::Declare {
            body: Some(stmts), ..
        } => {
            for &stmt in arenas.stmt_ids(stmts) {
                flow_of_stmt(arenas, stmt, labels, out);
            }
        }
        StmtKind::Unset(targets) => {
            let mut events = Vec::new();
            for &target in arenas.expr_ids(targets) {
                if let Some(ExprNode {
                    kind: ExprKind::PropertyAccess(base, name, _),
                    ..
                }) = arenas.peeled(target).and_then(|id| arenas.expr(id))
                    && is_this(arenas, *base)
                {
                    events.push(Event::Unset(name.name.clone()));
                } else {
                    events_of(arenas, target, &mut events);
                }
            }
            push_events(out, events);
        }
        StmtKind::If(if_stmt) => {
            let branches: Vec<IfBranch> = std::iter::once((if_stmt.cond, if_stmt.then_branch))
                .chain(
                    arenas
                        .elseifs(if_stmt.elseifs.clone())
                        .iter()
                        .map(|e| (e.cond, e.body)),
                )
                .map(|(cond, body)| {
                    let mut flow = Vec::new();
                    flow_of_stmt(arenas, body, labels, &mut flow);
                    IfBranch {
                        cond: events_of_each(arenas, [cond]).into_boxed_slice(),
                        body: flow.into_boxed_slice(),
                        terminates: completion_of(arenas, body, labels).terminates,
                    }
                })
                .collect();
            let else_body = if_stmt.else_branch.map(|e| {
                let mut flow = Vec::new();
                flow_of_stmt(arenas, e, labels, &mut flow);
                (
                    flow.into_boxed_slice(),
                    completion_of(arenas, e, labels).terminates,
                )
            });
            let silent = branches
                .iter()
                .all(|b| b.cond.is_empty() && b.body.is_empty())
                && else_body.as_ref().is_none_or(|(flow, _)| flow.is_empty());
            if !silent {
                out.push(Flow::If(IfChain {
                    branches: branches.into_boxed_slice(),
                    else_body,
                }));
            }
        }
        StmtKind::While { cond, body } => {
            let cond = events_of_each(arenas, [*cond]);
            let body = flow_of_stmts(arenas, &[*body], labels);
            if body.is_empty() {
                push_events(out, cond);
            } else {
                out.push(Flow::While {
                    cond: cond.into_boxed_slice(),
                    body: body.into_boxed_slice(),
                });
            }
        }
        StmtKind::For {
            init,
            cond,
            step,
            body,
        } => {
            let before = events_of_each(
                arenas,
                arenas
                    .expr_ids(init)
                    .iter()
                    .chain(arenas.expr_ids(cond))
                    .copied(),
            );
            let step = events_of_each(arenas, arenas.expr_ids(step).iter().copied());
            let body = flow_of_stmts(arenas, &[*body], labels);
            if body.is_empty() && step.is_empty() {
                push_events(out, before);
            } else {
                out.push(Flow::For {
                    before: before.into_boxed_slice(),
                    body: body.into_boxed_slice(),
                    step: step.into_boxed_slice(),
                });
            }
        }
        StmtKind::Foreach { subject, body, .. } => {
            let subject = events_of_each(arenas, [*subject]);
            let body = flow_of_stmts(arenas, &[*body], labels);
            if body.is_empty() {
                push_events(out, subject);
            } else {
                out.push(Flow::Foreach {
                    subject: subject.into_boxed_slice(),
                    body: body.into_boxed_slice(),
                });
            }
        }
        StmtKind::DoWhile { body, cond } => {
            let body = flow_of_stmts(arenas, &[*body], labels);
            let cond = events_of_each(arenas, [*cond]);
            out.extend(body);
            push_events(out, cond);
        }
        StmtKind::Switch { subject, cases } => {
            let subject = events_of_each(arenas, [*subject]);
            let cases: Vec<Case> = arenas
                .switch_cases(cases.clone())
                .iter()
                .map(|case| Case {
                    test: case
                        .test
                        .map_or_else(Vec::new, |test| events_of_each(arenas, [test]))
                        .into_boxed_slice(),
                    body: flow_of_stmts(arenas, arenas.stmt_ids(&case.body), labels)
                        .into_boxed_slice(),
                    completion: block_completion(arenas, arenas.stmt_ids(&case.body), labels),
                })
                .collect();
            if cases
                .iter()
                .all(|case| case.test.is_empty() && case.body.is_empty())
            {
                push_events(out, subject);
            } else {
                out.push(Flow::Switch {
                    subject: subject.into_boxed_slice(),
                    cases: cases.into_boxed_slice(),
                });
            }
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            let body = flow_of_stmts(arenas, arenas.stmt_ids(body), labels);
            let catches: Vec<Box<[Flow]>> = arenas
                .catch_clauses(catches.clone())
                .iter()
                .map(|c| flow_of_stmts(arenas, arenas.stmt_ids(&c.body), labels).into_boxed_slice())
                .collect();
            let finally = finally
                .as_ref()
                .map(|f| flow_of_stmts(arenas, arenas.stmt_ids(f), labels));
            if body.is_empty()
                && catches.iter().all(|c| c.is_empty())
                && finally.as_ref().is_none_or(Vec::is_empty)
            {
                return;
            }
            out.push(Flow::Try {
                body: body.into_boxed_slice(),
                catches: catches.into_boxed_slice(),
                finally: finally.map(Vec::into_boxed_slice),
            });
        }
        // Every other statement kind either carries no expression the
        // dataflow follows or is a nested declaration it does not enter.
        _ => {}
    })
}

/// The events of evaluating `e`, in order.
fn events_of(arenas: &NodeArenas, id: ExprId, out: &mut Vec<Event>) {
    let Some(e) = arenas.expr(id) else {
        return;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::PropertyAccess(base, name, _) => {
            events_of(arenas, *base, out);
            if is_this(arenas, *base) {
                out.push(Event::Read(
                    name.name.clone(),
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                ));
            }
        }
        ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
            events_of(arenas, *rhs, out);
            assignment_events(arenas, *lhs, out);
        }
        // `??=`'s isset-based left check never throws even when genuinely
        // uninitialized: quiet, like bare `??` and `isset()`, and it always
        // leaves the property initialized afterward.
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, rhs) => {
            quiet_probe_events(arenas, *lhs, out);
            events_of(arenas, *rhs, out);
            assignment_events(arenas, *lhs, out);
        }
        // Any other compound operator reads its own left side first, like
        // a bare read.
        ExprKind::CompoundAssign(_, lhs, rhs) => {
            events_of(arenas, *lhs, out);
            events_of(arenas, *rhs, out);
        }
        // Bare `??`'s left side is exactly as quiet as `isset()`'s argument.
        ExprKind::Binary(BinOp::Coalesce, lhs, rhs) => {
            quiet_probe_events(arenas, *lhs, out);
            events_of(arenas, *rhs, out);
        }
        ExprKind::Isset(operands) => {
            for operand in arenas.expr_ids(operands) {
                quiet_probe_events(arenas, *operand, out);
            }
        }
        ExprKind::Empty(operand) => quiet_probe_events(arenas, *operand, out),
        ExprKind::MethodCall(recv, name, args, _) => {
            events_of(arenas, *recv, out);
            for a in arenas.args(args.clone()) {
                events_of(arenas, a.value, out);
            }
            if let Some(chain) = this_chain(arenas, *recv) {
                out.push(Event::Call(Call {
                    receiver: CallReceiver::This(chain.into_boxed_slice()),
                    name: name.name.clone(),
                }));
            }
        }
        ExprKind::StaticCall(receiver, name, args) => {
            for a in arenas.args(args.clone()) {
                events_of(arenas, a.value, out);
            }
            let named = |keyword: &str| {
                receiver
                    .as_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case(keyword))
            };
            let receiver = if named("parent") {
                Some(CallReceiver::ParentKeyword)
            } else if named("self") {
                Some(CallReceiver::SelfKeyword)
            } else if named("static") {
                Some(CallReceiver::StaticKeyword)
            } else {
                None
            };
            if let Some(receiver) = receiver {
                out.push(Event::Call(Call {
                    receiver,
                    name: name.name.clone(),
                }));
            }
        }
        // Creating a closure does not execute its body. Likewise an
        // anonymous class's methods are declarations, though its arguments
        // are evaluated at the creation site.
        ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
        ExprKind::NewAnonymousClass(class_id) => {
            let Some(class) = arenas.anonymous_class(*class_id) else {
                return;
            };
            for arg in arenas.args(class.args.clone()) {
                events_of(arenas, arg.value, out);
            }
        }
        _ => {
            struct Children<'a>(&'a mut Vec<Event>);
            impl Visitor for Children<'_> {
                fn visit_expr(&mut self, arenas: &NodeArenas, child: ExprId) {
                    events_of(arenas, child, self.0);
                }
            }
            let mut children = Children(out);
            arena::walk_expr(&mut children, arenas, id);
        }
    })
}

/// The names of the calls between `$this` and the end of `recv`, when `recv`
/// is `$this` or a chain of calls on it: each must return the object for the
/// chain to still be it.
fn this_chain(arenas: &NodeArenas, id: ExprId) -> Option<Vec<Name>> {
    let recv = arenas.expr(id)?;
    syntax::stack::guard(|| match &recv.kind {
        ExprKind::Variable(v) if **v == *"this" => Some(Vec::new()),
        ExprKind::MethodCall(inner, name, _, _) => {
            let mut chain = this_chain(arenas, *inner)?;
            chain.push(name.name.clone());
            Some(chain)
        }
        _ => None,
    })
}

/// An assignment target: `$this->prop` becomes initialized and is no read;
/// a list assigns each of its targets, and its keys are read; anything else
/// is read as an expression.
fn assignment_events(arenas: &NodeArenas, id: ExprId, out: &mut Vec<Event>) {
    let Some(lhs) = arenas.expr(id) else {
        return;
    };
    syntax::stack::guard(|| {
        if let ExprKind::PropertyAccess(base, name, _) = &lhs.kind
            && is_this(arenas, *base)
        {
            out.push(Event::Write(name.name.clone()));
            return;
        }
        if let ExprKind::Destructure(items, _) = &lhs.kind {
            for item in arenas.destructure_items(items) {
                if let Some(key) = &item.key {
                    events_of(arenas, *key, out);
                }
                if let Some(target) = &item.target {
                    assignment_events(arenas, *target, out);
                }
            }
            return;
        }
        events_of(arenas, id, out);
    })
}

/// An `isset()`/`empty()`/bare-`??`-quieted expression: real PHP never throws
/// the "must not be accessed before initialization" error for a property
/// found anywhere in the chain being *tested*, however deep. An index or
/// name expression embedded in the chain is read as any expression is.
fn quiet_probe_events(arenas: &NodeArenas, id: ExprId, out: &mut Vec<Event>) {
    let Some(e) = arenas.expr(id) else {
        return;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::PropertyAccess(base, _, _) => quiet_probe_events(arenas, *base, out),
        ExprKind::DynamicPropertyAccess(base, name_expr, _) => {
            quiet_probe_events(arenas, *base, out);
            events_of(arenas, *name_expr, out);
        }
        ExprKind::Index(base, idx) => {
            quiet_probe_events(arenas, *base, out);
            if let Some(idx) = idx {
                events_of(arenas, *idx, out);
            }
        }
        _ => events_of(arenas, id, out),
    })
}

syntax::canonical_struct!(InitFlow { nodes });

syntax::canonical_enum!(Event {
    Read(name, span),
    Write(name),
    Unset(name),
    Call(call),
});

syntax::canonical_struct!(Call { receiver, name });

syntax::canonical_enum!(CallReceiver {
    This(chain),
    SelfKeyword,
    StaticKeyword,
    ParentKeyword,
});

syntax::canonical_enum!(Flow {
    Events(events),
    If(chain),
    While { cond, body },
    For { before, body, step },
    Foreach { subject, body },
    Switch { subject, cases },
    Try {
        body,
        catches,
        finally,
    },
});

syntax::canonical_struct!(IfChain {
    branches,
    else_body,
});

syntax::canonical_struct!(IfBranch {
    cond,
    body,
    terminates,
});

syntax::canonical_struct!(Case {
    test,
    body,
    completion,
});
