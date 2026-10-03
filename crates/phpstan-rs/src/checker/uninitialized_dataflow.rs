//! Real, per-constructor "was this property definitely assigned *by this
//! exact point*" dataflow - `checker/uninitialized.rs`'s existing check
//! answers only "is this property assigned *anywhere at all*". This
//! module is purely additive to it: a property this module ever reports
//! on is always one the existing check already knows genuinely gets
//! initialized somewhere - the question here is only "was it read too
//! early on some real path."
//!
//! Modeled as a small, standalone dataflow over a single new domain -
//! "which of this class's own directly-declared, qualifying properties
//! are known-initialized by this point" - deliberately not built on the
//! main checker's own `Locals`/`merge_scopes`/`converge_loop_body`: this
//! walker is intentionally separate from the main checker's traversal,
//! so coupling this state onto the shared struct would only force every
//! existing field-literal to account for a field it never uses.
//!
//! **`unset($this->prop)` is modeled**, and genuinely removes `prop`
//! from the current state (e.g. a constructor unsetting a just-declared
//! property specifically to force `__get()` on next access). `unset`
//! itself is never a read either, matching `checker/expr.rs`'s identical
//! exemption for ordinary narrowing.
//!
//! **Loops**: what is true at the head of a `while`/`for`/`foreach` is what
//! is true on entry and at the end of every pass, since the body may run
//! zero times or many. A pass can only lose properties (an `unset`, or a
//! branch that does not initialize one), so passes run until nothing more is
//! lost, at most one more than there are properties. `do`/`while` runs its
//! body at least once, so its body is walked once.
//!
//! **`switch`**: a case that does not end with `break` or `return` passes
//! its state into the next, as PHP does; what is true after the `switch` is
//! what is true at the end of each case that leaves it by a `break`, or by
//! falling off the end of the last. `match` arms are alternatives.
//!
//! **A genuine, un-fixed limitation**: a guard like `if (self::
//! STATUS_STARTED !== $this->status) { return false; }` depends on a
//! value-level fact about an unrelated property this module has no way
//! to know (it tracks property *initialization*, never arbitrary
//! property *values*). Reached before the guarded state can actually
//! hold, the guard always takes the safe branch in practice, but this
//! module, correctly conservative about a value it can't evaluate,
//! considers both branches possible and can flag a read past the guard
//! that's never reachable at runtime. A depth cap on absorption was
//! tried and reverted: it also broke an ordinary two-level unconditional-
//! initialization chain with nothing value-gated about it - trading one
//! false-positive class for another isn't a real fix.

use rustc_hash::FxHashSet;

use crate::checker::uninitialized::property_assigned_anywhere_in_hierarchy;
use crate::checker::{BREAK_HERE, CONTINUE_HERE, Checker};
use crate::registry::FunctionReflection;
use crate::registry::init_flow::{Call, CallReceiver, Case, Event, Flow, IfBranch, InitFlow};
use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::Visitor;

/// Bare property names known-initialized on every path reaching a given
/// point. Mirrors `checker.rs`'s `ProvenanceSet` type alias (same
/// persistent-set instantiation) for O(1)-clone semantics at every
/// branch point.
type PropertyInitState =
    imbl::GenericHashSet<String, rustc_hash::FxBuildHasher, imbl::shared_ptr::DefaultSharedPtr>;

/// Threaded through one top-level constructor's own walk (including every
/// method it recursively absorbs into) - accumulates premature-read
/// findings and guards against a cyclic same-class call graph.
struct PropertyInitCtx<'a, 'b> {
    checker: &'b Checker<'a>,
    /// The original, most-derived class actually being checked - fixed
    /// for the whole walk. `$this->method()`/`static::` always resolve
    /// against this (real PHP virtual/late-static-binding dispatch),
    /// never against whatever class the currently-recursed-into method
    /// body happens to be declared on.
    checked_class: String,
    /// Qualifying property names the existing `class_ever_assigns_property`
    /// heuristic already confirms are genuinely assigned somewhere in this
    /// class's own body - the only names this walker ever reports on.
    candidates: FxHashSet<String>,
    /// Whether the class has `__get()`, which a read of an unset
    /// property goes to instead of throwing.
    unset_reads_via_get: bool,
    /// `(declaring class FQN, lowercase method name)` pairs currently
    /// being recursed into, for the whole top-level walk - a re-entrant
    /// call while already visiting contributes nothing further rather
    /// than recursing infinitely. Sound but incomplete versus a full
    /// fixed point over the whole call graph: can only under-approximate
    /// absorption, never over-approximate it.
    visiting: FxHashSet<(String, String)>,
    /// Every `(property name, read span)` found reachable from the
    /// constructor's own call graph, before it's known-initialized on
    /// that path.
    premature_reads: Vec<(String, Span)>,
}

impl<'a> Checker<'a> {
    /// Additive to `check_uninitialized_properties`: only ever reports on
    /// a property that check already confirms is genuinely assigned
    /// somewhere, never a property that check itself already flagged as
    /// globally uninitialized. The constructor is walked from an empty
    /// state (defaults and promoted parameters aside); every other method
    /// starts with everything initialized, so only its own `unset()`s
    /// can make a read premature.
    pub(crate) fn check_premature_property_access(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.settings.rules.check_uninitialized_properties || c.def.kind != ClassKind::Class {
            return;
        }
        let constructor = c.def.constructor();
        // A property with a default or promoted from the constructor is
        // initialized before any code runs; one without either needs an
        // assignment somewhere for a premature read to mean anything.
        let mut initialized_up_front: Vec<String> = c
            .def
            .properties
            .iter()
            .filter(|p| trackable(&c.def, p) && p.default.is_some())
            .map(|p| p.name.to_string())
            .collect();
        initialized_up_front.extend(
            constructor
                .into_iter()
                .flat_map(|m| m.function.params.iter())
                .filter(|p| {
                    p.promotion.is_some()
                        && !p.promotion.is_some_and(|promotion| promotion.readonly)
                        && !c.def.is_readonly
                        && p.type_hint.is_some()
                })
                .map(|p| p.name.to_string()),
        );
        let assigned_later: Vec<String> = c
            .def
            .properties
            .iter()
            .filter(|p| {
                trackable(&c.def, p)
                    && p.default.is_none()
                    && property_assigned_anywhere_in_hierarchy(self, c, fqn, &p.name)
            })
            .map(|p| p.name.to_string())
            .collect();
        let candidates: FxHashSet<String> = initialized_up_front
            .iter()
            .chain(&assigned_later)
            .cloned()
            .collect();
        if candidates.is_empty() {
            return;
        }
        // After `unset()`, a read of a typed property goes to `__get()`
        // instead of throwing.
        let unset_reads_via_get = self.registry.lookup_method(fqn, "__get").is_some();
        let mut premature_reads: Vec<(String, Span)> = Vec::new();
        let mut walk = |checker: &Checker,
                        method: &str,
                        body: &[Stmt],
                        labels: &[GotoLabel],
                        entry: PropertyInitState| {
            let mut ctx = PropertyInitCtx {
                checker,
                checked_class: fqn.to_string(),
                candidates: candidates.clone(),
                unset_reads_via_get,
                visiting: FxHashSet::from_iter([(
                    fqn.to_ascii_lowercase(),
                    method.to_ascii_lowercase(),
                )]),
                premature_reads: Vec::new(),
            };
            walk_nodes(&InitFlow::of(body, labels).nodes, entry, &mut ctx, fqn);
            for read in ctx.premature_reads {
                if !premature_reads.contains(&read) {
                    premature_reads.push(read);
                }
            }
        };
        // An abstract class's constructor may rely on a subclass having
        // initialized a property first.
        if let Some(constructor) = constructor
            && !c.def.is_abstract
        {
            walk(
                &*self,
                "__construct",
                constructor.function.body_stmts(),
                goto_labels(&constructor.function),
                initialized_up_front.iter().cloned().collect(),
            );
        }
        for method in &c.def.methods {
            if method.is_static
                || method.is_abstract
                || method.function.is_constructor()
                || !unsets_this_property(method.function.body_stmts())
            {
                continue;
            }
            walk(
                &*self,
                &method.function.name,
                method.function.body_stmts(),
                goto_labels(&method.function),
                candidates.iter().cloned().collect(),
            );
        }
        for (prop, span) in premature_reads {
            self.report(
                format!("Access to an uninitialized property {fqn}::${prop}."),
                span,
                2,
                "property.uninitialized",
            );
        }
    }
}

/// The labels the `goto`s of a function body land on.
fn goto_labels(function: &FunctionDecl) -> &[GotoLabel] {
    function.body.as_ref().map_or(&[], |body| &body.goto_labels)
}

/// A property whose initialization state is tracked: typed (an untyped
/// one is `null` until assigned), per-instance, not readonly (unsetting
/// an initialized one throws) and without hooks (it has no backing store
/// to leave uninitialized, or its hooks decide what a read does).
fn trackable(c: &ClassDef, p: &PropertyDecl) -> bool {
    !p.is_static && !p.is_readonly && !c.is_readonly && p.hooks.is_none() && p.type_hint.is_some()
}

/// Whether `body` contains `unset($this->prop)` anywhere.
fn unsets_this_property(body: &[Stmt]) -> bool {
    struct Finder(bool);
    impl<'ast> Visitor<'ast> for Finder {
        fn visit_stmt(&mut self, stmt: &'ast Stmt) {
            if let StmtKind::Unset(targets) = &stmt.kind
                && targets.iter().any(
                    |t| matches!(&t.kind, ExprKind::PropertyAccess(base, _, _) if is_this(base)),
                )
            {
                self.0 = true;
            }
            if !self.0 {
                syntax::visit::walk_stmt(self, stmt);
            }
        }
    }
    let mut finder = Finder(false);
    for stmt in body {
        finder.visit_stmt(stmt);
    }
    finder.0
}

fn is_this(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Variable(v) if **v == *"this")
}

fn intersect(a: PropertyInitState, b: &PropertyInitState) -> PropertyInitState {
    a.into_iter().filter(|k| b.contains(k)).collect()
}

fn walk_nodes(
    nodes: &[Flow],
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    for node in nodes {
        state = walk_node(node, state, ctx, declaring_class);
    }
    state
}

fn walk_events(
    events: &[Event],
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    for event in events {
        state = apply(event, state, ctx, declaring_class);
    }
    state
}

/// An `if` chain from `branches[0]` on: the condition, its body, then the
/// rest of the chain (`elseif`s) or the `else`. A branch that
/// unconditionally exits the method (a guard clause) doesn't vote on what's
/// true afterward.
fn walk_if_chain(
    branches: &[IfBranch],
    else_body: Option<&(Box<[Flow]>, bool)>,
    state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    let [branch, rest @ ..] = branches else {
        return match else_body {
            Some((flow, _)) => walk_nodes(flow, state, ctx, declaring_class),
            None => state,
        };
    };
    let state_after_cond = walk_events(&branch.cond, state, ctx, declaring_class);
    let then_terminates = branch.terminates;
    let then_state = walk_nodes(&branch.body, state_after_cond.clone(), ctx, declaring_class);
    if rest.is_empty() && else_body.is_none() {
        return if then_terminates {
            state_after_cond
        } else {
            intersect(then_state, &state_after_cond)
        };
    }
    let else_terminates = else_body.is_some_and(|(_, terminates)| *terminates)
        && rest.iter().all(|branch| branch.terminates);
    let else_state = walk_if_chain(
        rest,
        else_body,
        state_after_cond.clone(),
        ctx,
        declaring_class,
    );
    match (then_terminates, else_terminates) {
        (true, true) => state_after_cond,
        (true, false) => else_state,
        (false, true) => then_state,
        (false, false) => intersect(then_state, &else_state),
    }
}

fn walk_node(
    node: &Flow,
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    match node {
        Flow::Events(events) => walk_events(events, state, ctx, declaring_class),
        Flow::If(chain) => walk_if_chain(
            &chain.branches,
            chain.else_body.as_ref(),
            state,
            ctx,
            declaring_class,
        ),
        // `while`/`for`/`foreach` may run their own body zero times, and
        // more than once: what is true at the head of the loop is what is
        // true on entry and at the end of every pass, found by passes until
        // nothing more is lost.
        Flow::While { cond, body } => {
            let state_after_cond = walk_events(cond, state, ctx, declaring_class);
            loop_head(state_after_cond, ctx, |head, ctx| {
                walk_nodes(body, head, ctx, declaring_class)
            })
        }
        Flow::For { before, body, step } => {
            state = walk_events(before, state, ctx, declaring_class);
            loop_head(state, ctx, |head, ctx| {
                let body_end = walk_nodes(body, head, ctx, declaring_class);
                walk_events(step, body_end, ctx, declaring_class)
            })
        }
        Flow::Foreach { subject, body } => {
            let state_after_subject = walk_events(subject, state, ctx, declaring_class);
            loop_head(state_after_subject, ctx, |head, ctx| {
                walk_nodes(body, head, ctx, declaring_class)
            })
        }
        // A case that does not end with a `break` or a `return` passes its
        // state into the next, as PHP does. What is true after the switch is
        // what is true at the end of each case that leaves it: by a `break`,
        // or by falling off the end of the last.
        Flow::Switch { subject, cases } => {
            let base = walk_events(subject, state, ctx, declaring_class);
            let mut exits: Option<PropertyInitState> = None;
            let mut running: Option<PropertyInitState> = None;
            for (
                at,
                Case {
                    test,
                    body,
                    completion,
                },
            ) in cases.iter().enumerate()
            {
                let direct = walk_events(test, base.clone(), ctx, declaring_class);
                let entry = match running.take() {
                    Some(passed) => intersect(direct, &passed),
                    None => direct,
                };
                let case_end = walk_nodes(body, entry, ctx, declaring_class);
                let leaves_by_break = completion.jumps & (BREAK_HERE | CONTINUE_HERE) != 0;
                let is_last = at + 1 == cases.len();
                if leaves_by_break || (is_last && !completion.terminates) {
                    exits = Some(match exits {
                        Some(acc) => intersect(acc, &case_end),
                        None => case_end.clone(),
                    });
                }
                if !completion.terminates {
                    running = Some(case_end);
                }
            }
            exits.unwrap_or(base)
        }
        Flow::Try {
            body,
            catches,
            finally,
        } => {
            // Matches `checker/stmt.rs`'s existing `StmtKind::Try`
            // handling: each catch clause starts from a clone of the try
            // body's completed end state, not a conservative "could have
            // thrown before any statement" entry.
            let try_end = walk_nodes(body, state, ctx, declaring_class);
            let mut branch_states = vec![try_end.clone()];
            for catch in catches.iter() {
                branch_states.push(walk_nodes(catch, try_end.clone(), ctx, declaring_class));
            }
            let mut merged = branch_states
                .into_iter()
                .reduce(|acc, s| intersect(acc, &s))
                .unwrap_or_default();
            if let Some(finally_body) = finally {
                merged = walk_nodes(finally_body, merged, ctx, declaring_class);
            }
            merged
        }
    }
}

/// The state at the head of a loop whose entry state is `entry` and whose
/// pass `pass` takes the state at its head to the state at its end: what
/// holds on entry and at the end of each pass. Each pass can only lose
/// properties, so there are at most as many passes as there are
/// properties, and the last of them finds every read that could be
/// premature.
fn loop_head<'a, 'b>(
    entry: PropertyInitState,
    ctx: &mut PropertyInitCtx<'a, 'b>,
    mut pass: impl FnMut(PropertyInitState, &mut PropertyInitCtx<'a, 'b>) -> PropertyInitState,
) -> PropertyInitState {
    let mut head = entry.clone();
    loop {
        let end = pass(head.clone(), ctx);
        let next = intersect(entry.clone(), &end);
        if next.len() == head.len() {
            return head;
        }
        head = next;
    }
}

/// One event: a read before the property is initialized is a premature
/// one, a write initializes it, an `unset` leaves it uninitialized again
/// (unless `__get()` answers a read of it), and a call goes into the body
/// of the method it reaches.
fn apply(
    event: &Event,
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    match event {
        Event::Read(name, span) => {
            if ctx.candidates.contains(name.as_str()) && !state.contains(name.as_str()) {
                ctx.premature_reads.push((name.to_string(), *span));
            }
            state
        }
        Event::Write(name) => {
            state.insert(name.to_string());
            state
        }
        Event::Unset(name) => {
            if !ctx.unset_reads_via_get {
                state = state.without(name.as_str());
            }
            state
        }
        Event::Call(Call { receiver, name }) => {
            let registry = ctx.checker.registry;
            let target = match receiver {
                // A fluent setter chain (`$this->setChannel(...)
                // ->setUsername(...)`) is still `$this` while every link
                // returns `self` or `static`. Virtual dispatch: always
                // against `checked_class` (the original, most-derived
                // class), never `declaring_class`.
                CallReceiver::This(chain) => {
                    let still_this = chain.iter().all(|link| {
                        registry
                            .lookup_method(&ctx.checked_class, link)
                            .is_some_and(|sig| sig.return_type_native().is_self_reference())
                    });
                    if !still_this {
                        return state;
                    }
                    registry.lookup_method_with_owner(&ctx.checked_class, name)
                }
                // `parent::` is lexical, not virtual: the immediate parent
                // of whichever class the *current* code is written in.
                CallReceiver::ParentKeyword => {
                    registry.lookup_ancestor_method_with_owner(declaring_class, name)
                }
                // `self::` is lexical too (no late static binding), unlike
                // `static::`: the class the code is written in, walking
                // that class's own ancestors from there.
                CallReceiver::SelfKeyword => {
                    registry.lookup_method_with_owner(declaring_class, name)
                }
                // Late static binding: virtual, exactly like `$this->`.
                CallReceiver::StaticKeyword => {
                    registry.lookup_method_with_owner(&ctx.checked_class, name)
                }
            };
            absorb_resolved(target, name, state, ctx)
        }
    }
}

/// Recursively walks into an already-resolved call target's flow, seeded
/// with the caller's current state as its entry set, sharing the same
/// `ctx` - calling into a method absorbs whatever it's proven to
/// guarantee. A target with no real body to walk (abstract, an interface
/// method, or simply unresolvable) contributes nothing: the caller's
/// state passes through unchanged.
fn absorb_resolved(
    resolved: Option<(types::Name, &FunctionReflection)>,
    method_name: &str,
    state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
) -> PropertyInitState {
    let Some((owner, sig)) = resolved else {
        return state;
    };
    let Some(flow) = ctx.checker.bodies.init_flow(sig.provenance()) else {
        return state;
    };
    let key = (owner.to_ascii_lowercase(), method_name.to_ascii_lowercase());
    if !ctx.visiting.insert(key.clone()) {
        return state;
    }
    let result = walk_nodes(&flow.nodes, state, ctx, &owner);
    ctx.visiting.remove(&key);
    result
}
