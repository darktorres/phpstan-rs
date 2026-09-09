//! Phase 62's own remaining, harder half: real, per-constructor "was this
//! property definitely assigned *by this exact point*" dataflow -
//! `checker/uninitialized.rs`'s own existing check answers only "is this
//! property assigned *anywhere at all*", deliberately (see that module's
//! own doc comment for why). This module is purely additive to it: a
//! property this module ever reports on is always one the existing check
//! already knows genuinely gets initialized somewhere - the question here
//! is only ever "was it read too early on some real path."
//!
//! Modeled as a small, standalone dataflow over a single new domain -
//! "which of this class's own directly-declared, qualifying properties are
//! known-initialized by this point" - deliberately *not* built on the main
//! checker's own `Locals`/`merge_scopes`/`converge_loop_body` (see this
//! module's own git history/the Phase 62 plan for why an earlier attempt
//! at reusing `Locals` directly was reverted: this walker is intentionally
//! separate from the main checker's own `check_stmt`/`check_expr`
//! traversal, so coupling this state onto the shared struct would only
//! have forced every one of its existing field-literals to account for a
//! field they never use, for no real benefit).
//!
//! **`unset($this->prop)` is modeled**, and genuinely removes `prop` from
//! the current state - a real corpus find (guzzlehttp/psr7's own
//! `LazyOpenStream::__construct`, which unsets a just-declared property
//! specifically to force `__get()` on next access) disproved an earlier,
//! untested assumption here that this shape was "vanishingly rare."
//! `unset` itself is never a read either, matching `checker/expr.rs`'s
//! own identical exemption for ordinary narrowing.
//!
//! **Loops, and why `unset` complicates them**: without `unset`, this
//! domain only ever *grows* (a monotonic union), which would make the
//! *worst case* reachable through a `while`/`for`/`foreach` loop (which
//! might run zero times) always exactly the state *before* the loop -
//! every state reachable after 1, 2, ... iterations would be a superset
//! of it. With `unset` now real, that's no longer quite true in general
//! (a property could be unset on some iteration), so each of these three
//! loop kinds intersects the zero-iteration state with the state after
//! walking the body *once* - exact for the overwhelmingly common
//! monotonic case (the intersection changes nothing there), and still
//! sound for a *single* iteration unsetting something. A disclosed,
//! deliberate simplification for the narrower, compound case of a
//! *second or later* iteration unsetting something the first iteration
//! didn't (a real fixed point, `converge_loop_body`-style, would be
//! needed to close that gap) - out of this whole feature's own "don't
//! guess past the common, provable case" scope, and errs in this
//! project's own already-established safe direction (could only miss a
//! real premature-access finding in that one narrow shape, never invent
//! a false one). `do`/`while` needs none of this: its body runs at least
//! once, so the state *after* walking it exactly once (no zero-iteration
//! case exists to intersect against) is the correct, exact answer
//! regardless.
//!
//! **A disclosed simplification, unlike the above**: `switch`/`match`
//! arms are treated as mutually-exclusive alternatives intersected
//! together (matching `merge_scopes`'s own "only what every branch
//! guarantees" rule), *not* real PHP `switch` fallthrough between
//! adjacent cases with no `break` - a real, rarer shape this module
//! doesn't attempt, erring in this project's own already-established safe
//! direction (fewer flags, not more; see `checker/uninitialized.rs`'s own
//! doc comment for the identical precedent).
//!
//! **A genuine, disclosed, un-fixed limitation** (real corpus find:
//! Symfony `Process::__construct` -> `setInput()` -> `isRunning()`):
//! `isRunning()` guards its own real logic behind `if (self::
//! STATUS_STARTED !== $this->status) { return false; }` - a value-level
//! fact about an *unrelated* property this module has no way to know
//! (it tracks property *initialization*, never arbitrary property
//! *values* - building that would mean re-implementing the main
//! checker's own `Locals`/narrowing engine inside this deliberately
//! standalone module, Design ##1's own architectural boundary). Reached
//! from the constructor before the process has actually started,
//! `$this->status` can't really *be* `STATUS_STARTED` yet, so the guard
//! always takes the safe branch in practice - but this module,
//! correctly conservative about a value it can't evaluate, considers
//! both branches possible and flags a read past the guard that's
//! genuinely never reachable at runtime. A depth cap on absorption was
//! tried and reverted: it also broke a *different*, definitely-safe real
//! shape found in the very same investigation (Monolog's own
//! `RotatingFileHandler::__construct` -> `setFilenameFormat()` ->
//! `setDateFormat()`, an ordinary two-level unconditional-initialization
//! chain with nothing value-gated about it at all) - trading one real
//! false-positive class for another isn't a real fix, so this one stays
//! open, disclosed, rather than band-aided.

use std::collections::HashSet as StdHashSet;
use std::sync::Arc;

use crate::ast::*;
use crate::ast_walk::expr_children;
use crate::checker::Checker;
use crate::checker::uninitialized::property_assigned_anywhere_in_hierarchy;
use crate::registry::FunctionSig;
use crate::token::Span;
use crate::types::Type;

/// Bare property names known-initialized on every path reaching a given
/// point - see this module's own doc comment for why a small, dedicated
/// type is used here rather than the main checker's own `Locals`.
/// Mirrors `checker.rs`'s own `ProvenanceSet` type alias exactly (same
/// persistent-set instantiation), for the identical reason: O(1)-clone
/// semantics at every branch point, without this module needing its own
/// copy of that reasoning.
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
    /// body happens to be declared on - see `walk_expr`'s own
    /// `MethodCall`/`StaticCall` arms, where the distinction is actually
    /// made.
    checked_class: String,
    /// Qualifying property names the *existing* `class_ever_assigns_property`
    /// heuristic already confirms are genuinely assigned somewhere in this
    /// class's own body - the only names this walker ever reports on.
    candidates: StdHashSet<String>,
    /// `(declaring class FQN, lowercase method name)` pairs currently
    /// being recursed into, for the whole top-level walk - a re-entrant
    /// call while already visiting contributes nothing further rather
    /// than recursing infinitely. A real, disclosed, sound-but-incomplete
    /// simplification versus a full fixed point over the whole call graph:
    /// can only under-approximate absorption (miss a guarantee a genuine
    /// cycle would provide), never over-approximate it - see the Phase 62
    /// plan's own "Cross-method absorption" section for the full
    /// reasoning.
    visiting: StdHashSet<(String, String)>,
    /// Every `(property name, read span)` found reachable from the
    /// constructor's own call graph, before it's known-initialized on
    /// that path.
    premature_reads: Vec<(String, Span)>,
}

impl<'a> Checker<'a> {
    /// Phase 62's premature-access half - see this module's own doc
    /// comment. Additive to `check_uninitialized_properties`: only ever
    /// reports on a property that check already confirms is genuinely
    /// assigned somewhere, never a property that check itself already
    /// flagged as globally uninitialized (this project's own deliberately
    /// lenient "never initialized" verdict, kept exactly as-is - see the
    /// Phase 62 plan's own "A real design choice, not a narrowing"
    /// section for why).
    pub(crate) fn check_premature_property_access(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.registry.check_uninitialized_properties {
            return;
        }
        if c.kind != ClassKind::Class || c.is_abstract {
            return;
        }
        let Some(constructor) = c
            .methods
            .iter()
            .find(|m| m.function.name.eq_ignore_ascii_case("__construct"))
        else {
            return;
        };
        let candidates: StdHashSet<String> = c
            .properties
            .iter()
            .filter(|p| {
                !p.is_static
                    && !p.is_readonly
                    && p.default.is_none()
                    && p.hooks.is_none()
                    && p.type_hint.is_some()
                    && property_assigned_anywhere_in_hierarchy(self, c, fqn, &p.name)
            })
            .map(|p| p.name.clone())
            .collect();
        if candidates.is_empty() {
            return;
        }
        let mut ctx = PropertyInitCtx {
            checker: &*self,
            checked_class: fqn.to_string(),
            candidates,
            visiting: StdHashSet::from([(fqn.to_ascii_lowercase(), "__construct".to_string())]),
            premature_reads: Vec::new(),
        };
        walk_stmts(
            &constructor.function.body,
            PropertyInitState::default(),
            &mut ctx,
            fqn,
        );
        let premature_reads = ctx.premature_reads;
        for (prop, span) in premature_reads {
            self.report(
                fqn.to_string(),
                format!("Access to an uninitialized property {fqn}::${prop}."),
                span,
                2,
                "property.uninitialized",
            );
        }
    }
}

fn is_unset_call(callee: &Expr) -> bool {
    matches!(&callee.kind, ExprKind::Name(name) if name == "unset")
}

fn is_this(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Variable(v) if v == "this")
}

/// Whether `e` evaluates to the very same object `$this` refers to -
/// either literally `$this`, or a fluent method call chain rooted in
/// `$this` where every link's own declared return type is `self`/
/// `static` (real PHP's own fluent-setter convention: `public function
/// setX(...): self { $this->x = ...; return $this; }`). Recurses
/// through any chain depth (`$this->a()->b()->c()`), matching the same
/// "don't stop at one level" instinct `checker/assignment.rs`'s own
/// destructuring recursion already has for a different shape. A method
/// whose return type resolves to anything else (or isn't found at all)
/// stops the chain - the call itself is still walked normally by the
/// caller, just never treated as a further `$this`-rooted absorption
/// target beyond that point.
fn resolves_to_this(e: &Expr, ctx: &PropertyInitCtx) -> bool {
    match &e.kind {
        ExprKind::Variable(v) => v == "this",
        ExprKind::MethodCall(recv, name, _, _) if resolves_to_this(recv, ctx) => ctx
            .checker
            .registry
            .lookup_method(&ctx.checked_class, name)
            .is_some_and(|sig| matches!(sig.return_type_native, Type::SelfRef | Type::StaticRef)),
        _ => false,
    }
}

fn intersect(a: PropertyInitState, b: &PropertyInitState) -> PropertyInitState {
    a.into_iter().filter(|k| b.contains(k)).collect()
}

fn is_probe_call(callee: &Expr) -> bool {
    matches!(&callee.kind, ExprKind::Name(name) if name == "isset" || name == "empty")
}

/// Walks an `isset()`/`empty()`/bare-`??`-quieted expression - real PHP
/// never throws the "must not be accessed before initialization" error
/// for a property found anywhere in the chain being *tested* this way,
/// however deep (`checker/expr.rs`'s own `check_expr_quiet_root` is the
/// established precedent for this same real-PHP semantics; not reused
/// directly, since this module doesn't share that function's own
/// `Locals`-based traversal - mirrors its *effect* only). Recurses
/// through `PropertyAccess`/`DynamicPropertyAccess`/`Index` - the shapes
/// that make up a probed chain (`$this->a->b['c']`) - without ever
/// registering a premature-read finding for a `$this->prop` found along
/// it; an index/key expression *embedded* inside the chain
/// (`$this->arr[$this->k]`) still gets the ordinary, non-quiet walk via
/// `walk_expr` - only the chain actually being tested is quiet, not
/// every expression reachable from it.
fn walk_quiet_probe(
    e: &Expr,
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    match &e.kind {
        ExprKind::PropertyAccess(base, _, _) => walk_quiet_probe(base, state, ctx, declaring_class),
        ExprKind::DynamicPropertyAccess(base, name_expr) => {
            state = walk_quiet_probe(base, state, ctx, declaring_class);
            walk_expr(name_expr, state, ctx, declaring_class)
        }
        ExprKind::Index(base, idx) => {
            state = walk_quiet_probe(base, state, ctx, declaring_class);
            match idx {
                Some(idx) => walk_expr(idx, state, ctx, declaring_class),
                None => state,
            }
        }
        _ => walk_expr(e, state, ctx, declaring_class),
    }
}

fn walk_stmts(
    stmts: &[Stmt],
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    for s in stmts {
        state = walk_stmt(s, state, ctx, declaring_class);
    }
    state
}

/// A conservative, syntax-only check (no type inference, unlike the main
/// checker's own `always_terminates`) for whether a statement
/// unconditionally exits the enclosing method - `Return`/`Throw`/
/// `Break`/`Continue` directly, a `Block` containing one anywhere (not
/// just as its last statement, matching `control_flow.rs`'s own
/// `always_terminates` "any statement, however early" logic), or an
/// `If` where *both* branches do (a bare `if` with no `else` can never
/// qualify - the "condition false" path always falls through). Used
/// only to decide whether a branch's own resulting state should be
/// excluded from a merge, matching `checker/stmt.rs`'s own established
/// "a branch that always terminates doesn't get to vote on what's true
/// afterward" precedent for ordinary narrowing. Deliberately narrower
/// than a full `always_terminates` (no `Type::Never`-returning-call
/// detection, no `Switch`/`Try` exhaustiveness, since either needs a
/// real `Locals`/`infer_expr` this standalone module doesn't have) -
/// this module's own established "don't guess past the common, provable
/// case" scope; a call proven to never return, or a switch/try that
/// exhaustively terminates every arm, is a real, rarer gap this
/// simplification accepts, in the project's own safe direction (could
/// only miss a real finding downstream of one, never invent a false
/// one).
fn stmt_definitely_terminates(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Break(_) | StmtKind::Continue(_) => {
            true
        }
        StmtKind::Block(stmts) => stmts.iter().any(stmt_definitely_terminates),
        StmtKind::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => stmt_definitely_terminates(then_branch) && stmt_definitely_terminates(else_branch),
        _ => false,
    }
}

fn walk_stmt(
    stmt: &Stmt,
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => walk_expr(e, state, ctx, declaring_class),
        StmtKind::Return(Some(e)) => walk_expr(e, state, ctx, declaring_class),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                state = walk_expr(e, state, ctx, declaring_class);
            }
            state
        }
        StmtKind::StaticVar(vars) => {
            for (_, init) in vars {
                if let Some(e) = init {
                    state = walk_expr(e, state, ctx, declaring_class);
                }
            }
            state
        }
        StmtKind::Block(stmts) => walk_stmts(stmts, state, ctx, declaring_class),
        StmtKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            // A branch that unconditionally exits the method (a real
            // corpus find: Symfony `Process::isRunning`/`updateStatus`,
            // both an `if (...) { return ...; }` guard-clause with no
            // `else`) doesn't get to vote on what's true afterward -
            // matching `checker/stmt.rs`'s own established precedent for
            // ordinary narrowing (see `stmt_definitely_terminates`'s own
            // doc comment). Without this, code placed *after* such a
            // guard was wrongly analyzed as if it could also run on the
            // path the guard itself already ruled out.
            let state_after_cond = walk_expr(cond, state, ctx, declaring_class);
            let then_terminates = stmt_definitely_terminates(then_branch);
            let then_state = walk_stmt(then_branch, state_after_cond.clone(), ctx, declaring_class);
            match else_branch {
                Some(else_stmt) => {
                    let else_terminates = stmt_definitely_terminates(else_stmt);
                    let else_state =
                        walk_stmt(else_stmt, state_after_cond.clone(), ctx, declaring_class);
                    match (then_terminates, else_terminates) {
                        (true, true) => state_after_cond, // unreachable past here either way
                        (true, false) => else_state,
                        (false, true) => then_state,
                        (false, false) => intersect(then_state, &else_state),
                    }
                }
                None if then_terminates => state_after_cond,
                None => intersect(then_state, &state_after_cond),
            }
        }
        // `while`/`for`/`foreach` may run their own body zero times - see
        // this module's own doc comment for why intersecting the
        // zero-iteration state with the one-iteration state is the
        // exact, sound answer (not a fixed point, and not simply
        // "whatever's true before the loop" outright, now that `unset`
        // is modeled - see that doc comment for why).
        StmtKind::While { cond, body } => {
            let state_after_cond = walk_expr(cond, state, ctx, declaring_class);
            let one_pass = walk_stmt(body, state_after_cond.clone(), ctx, declaring_class);
            intersect(state_after_cond, &one_pass)
        }
        StmtKind::For {
            init,
            cond,
            step,
            body,
        } => {
            for e in init {
                state = walk_expr(e, state, ctx, declaring_class);
            }
            for e in cond {
                state = walk_expr(e, state, ctx, declaring_class);
            }
            // `body`/`step` may never run at all (`cond` false on the
            // very first check).
            let mut body_state = walk_stmt(body, state.clone(), ctx, declaring_class);
            for e in step {
                body_state = walk_expr(e, body_state, ctx, declaring_class);
            }
            intersect(state, &body_state)
        }
        StmtKind::Foreach { subject, body, .. } => {
            let state_after_subject = walk_expr(subject, state, ctx, declaring_class);
            let one_pass = walk_stmt(body, state_after_subject.clone(), ctx, declaring_class);
            intersect(state_after_subject, &one_pass)
        }
        // `do ... while` runs its own body at least once - unlike the
        // three loop kinds above, the state after walking it exactly
        // once is the real, exact answer (see this module's own doc
        // comment), not a lower bound that a fixed point could improve.
        StmtKind::DoWhile { body, cond } => {
            state = walk_stmt(body, state, ctx, declaring_class);
            walk_expr(cond, state, ctx, declaring_class)
        }
        StmtKind::Switch { subject, cases } => {
            let base = walk_expr(subject, state, ctx, declaring_class);
            let mut merged: Option<PropertyInitState> = None;
            for case in cases {
                let mut case_state = base.clone();
                if let Some(test) = &case.test {
                    case_state = walk_expr(test, case_state, ctx, declaring_class);
                }
                let case_end = walk_stmts(&case.body, case_state, ctx, declaring_class);
                merged = Some(match merged {
                    Some(acc) => intersect(acc, &case_end),
                    None => case_end,
                });
            }
            merged.unwrap_or(base)
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            // Matches `checker/stmt.rs`'s own existing `StmtKind::Try`
            // handling exactly (verified directly, not assumed): each
            // catch clause starts from a *clone of the try body's own
            // completed end state*, not a conservative "could have
            // thrown before any statement" entry - a real, deliberate,
            // already-shipped choice this project makes for ordinary
            // narrowing too.
            let try_end = walk_stmts(body, state, ctx, declaring_class);
            let mut branch_states = vec![try_end.clone()];
            for c in catches {
                branch_states.push(walk_stmts(&c.body, try_end.clone(), ctx, declaring_class));
            }
            let mut merged = branch_states
                .into_iter()
                .reduce(|acc, s| intersect(acc, &s))
                .unwrap_or_default();
            if let Some(finally_body) = finally {
                merged = walk_stmts(finally_body, merged, ctx, declaring_class);
            }
            merged
        }
        // Every other statement kind either carries no expression this
        // module cares about (`Break`/`Continue`/`Global`/`Goto`/`Label`/
        // `InlineHtml`/`Return(None)`) or is a nested declaration this
        // module deliberately doesn't descend into, matching
        // `ast_walk::stmt_children`'s own documented boundary
        // (`FunctionDecl`/`ClassDecl`/`EnumDecl`/`Namespace`/`Use`/
        // `GlobalConst`).
        _ => state,
    }
}

fn walk_expr(
    e: &Expr,
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    match &e.kind {
        ExprKind::PropertyAccess(base, name, _) => {
            state = walk_expr(base, state, ctx, declaring_class);
            if is_this(base) && ctx.candidates.contains(name) && !state.contains(name) {
                ctx.premature_reads.push((name.clone(), e.span));
            }
            state
        }
        ExprKind::Assign(lhs, rhs) => {
            state = walk_expr(rhs, state, ctx, declaring_class);
            assign_target(lhs, state, ctx, declaring_class)
        }
        // `??=`'s own isset-based left check never throws even when
        // genuinely uninitialized (`checker/uninitialized.rs`'s own doc
        // comment has the real-PHP verification) - never a premature-read
        // site (quiet-probed, exactly like bare `??`/`isset()` below, not
        // an ordinary read), and always leaves the property initialized
        // afterward, exactly like a plain assignment.
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, rhs) => {
            state = walk_quiet_probe(lhs, state, ctx, declaring_class);
            state = walk_expr(rhs, state, ctx, declaring_class);
            assign_target(lhs, state, ctx, declaring_class)
        }
        // Any other compound operator (`.=`, `+=`, ...) reads its own
        // left side first, exactly like a bare read - real PHP throws for
        // these the same way it would for a plain read.
        ExprKind::CompoundAssign(_, lhs, rhs) => {
            state = walk_expr(lhs, state, ctx, declaring_class);
            walk_expr(rhs, state, ctx, declaring_class)
        }
        // Bare `??` (not `??=`): a real corpus find (Symfony Process's
        // own `updateStatus()`, `$this->processInformation['running'] ??
        // true`) - its own left side is exactly as quiet as `isset()`'s
        // own argument (real PHP: `??`'s left-hand check is defined in
        // terms of `isset()`), for the *whole* probed chain, not just a
        // bare top-level property.
        ExprKind::Binary(BinOp::Coalesce, lhs, rhs) => {
            state = walk_quiet_probe(lhs, state, ctx, declaring_class);
            walk_expr(rhs, state, ctx, declaring_class)
        }
        // `isset($this->prop)`/`empty($this->prop)` - same quiet-probe
        // exemption as `??`'s own left side (real PHP defines `??` in
        // terms of `isset()`), and neither one ever initializes anything.
        ExprKind::Call(callee, args) if is_probe_call(callee) => {
            for a in args {
                state = walk_quiet_probe(&a.value, state, ctx, declaring_class);
            }
            state
        }
        // `unset($this->prop)` is neither a read nor an initializer -
        // real PHP semantics (confirmed against a real corpus shape,
        // guzzlehttp/psr7's own `LazyOpenStream::__construct`, which
        // does exactly this to force `__get()` on next access): `unset`
        // never throws regardless of the property's current init state,
        // the same established exemption `check_expr`'s own identical
        // `unset`-recognition (`checker/expr.rs`) already carries for
        // ordinary narrowing. Genuinely removes the property afterward
        // (matching real semantics: a typed property, once unset, is
        // uninitialized again) - `state.without` rather than leaving it
        // untouched.
        ExprKind::Call(callee, args) if is_unset_call(callee) => {
            for a in args {
                if let ExprKind::PropertyAccess(base, name, _) = &a.value.kind
                    && is_this(base)
                {
                    state = state.without(name);
                    continue;
                }
                state = walk_expr(&a.value, state, ctx, declaring_class);
            }
            state
        }
        ExprKind::MethodCall(recv, name, args, _) => {
            state = walk_expr(recv, state, ctx, declaring_class);
            for a in args {
                state = walk_expr(&a.value, state, ctx, declaring_class);
            }
            // `resolves_to_this`, not a bare `is_this` receiver check: a
            // real corpus find (Monolog's own `SlackRecord::__construct`)
            // chains fluent setters (`$this->setChannel(...)->setUsername(...)
            // ->includeContextAndExtra(...)`) - every call after the first
            // has a `MethodCall` as its own receiver, not literally `$this`,
            // even though it evaluates to the very same object (a
            // `: self`/`: static`-returning fluent setter). Missing this
            // meant every assignment past the first link in the chain was
            // never absorbed at all.
            if resolves_to_this(recv, ctx) {
                // Virtual dispatch: always against `checked_class` (the
                // original, most-derived class), never `declaring_class`
                // (whatever class the currently-recursed-into method body
                // happens to live on) - see `PropertyInitCtx::checked_class`'s
                // own doc comment.
                let checked_class = ctx.checked_class.clone();
                state = absorb(&checked_class, name, state, ctx);
            }
            state
        }
        ExprKind::StaticCall(class_name, name, args) => {
            for a in args {
                state = walk_expr(&a.value, state, ctx, declaring_class);
            }
            // Only the three pseudo-keywords are resolved - a literal
            // class name (`Foo::bar()`) is out of this module's own
            // disclosed scope (see the Phase 62 plan's "Cross-method
            // absorption" section: "resolving to a real method on the
            // *same class*").
            let target = if class_name.eq_ignore_ascii_case("parent") {
                // `parent::` is lexical, not virtual: it always means
                // "the immediate parent of whichever class the *current*
                // code is written in" - `declaring_class`'s own parent,
                // never `checked_class`'s.
                ctx.checker
                    .registry
                    .lookup_ancestor_method_with_owner(declaring_class, name)
            } else if class_name.eq_ignore_ascii_case("self") {
                // `self::` is also lexical (no late static binding),
                // unlike `static::` below - real PHP resolves it against
                // the class the code is written in, walking that class's
                // own ancestors from there, not `checked_class`'s.
                // Deliberately not distinguishing an actual subclass
                // override further down `checked_class`'s own chain (a
                // rare, disclosed simplification - see this module's own
                // module-level doc comment for the project's established
                // "safe direction" precedent).
                ctx.checker
                    .registry
                    .lookup_method_with_owner(declaring_class, name)
            } else if class_name.eq_ignore_ascii_case("static") {
                // Late static binding: virtual, exactly like `$this->`.
                let checked_class = ctx.checked_class.clone();
                ctx.checker
                    .registry
                    .lookup_method_with_owner(&checked_class, name)
            } else {
                None
            };
            absorb_resolved(target, name, state, ctx)
        }
        _ => {
            for child in expr_children(e) {
                state = walk_expr(child, state, ctx, declaring_class);
            }
            state
        }
    }
}

/// A `$this->prop = ...`-shaped assignment target marks `prop` as
/// known-initialized from here on and is never itself a "read" (matching
/// `checker/assignment.rs`'s own `check_assignment_target`, which the
/// same distinction already exists for, for ordinary narrowing - same
/// source, mirrored deliberately rather than reused, per this module's
/// own "standalone, not built on the main checker's own traversal"
/// design). Also mirrors that function's own `ArrayLiteral` handling: a
/// real corpus find (Symfony's own `WrappedListener::__construct`,
/// `[$this->name, $this->callableRef] = $this->parseListener(...)`)
/// list-destructures directly into two properties - treating the whole
/// left side as an ordinary expression (this module's original shape)
/// wrongly read both as still-uninitialized at the very point they were
/// being assigned. Recurses through any nesting depth, matching
/// `check_assignment_target`'s own identical recursion; an item's own
/// `key` expression (`['k' => $x] = ...`) is a real read, not a target,
/// and is left to fall through to the ordinary expression walk exactly
/// as `check_assignment_target` also leaves it untouched. Any other
/// shape (`$x = ...`, `$this->arr[$k] = ...`, ...) isn't a tracked
/// property write, but its own base/index subexpressions may still
/// contain reads/calls worth walking - handled by falling through to the
/// ordinary expression walk.
fn assign_target(
    lhs: &Expr,
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    if let ExprKind::PropertyAccess(base, name, _) = &lhs.kind
        && is_this(base)
    {
        state.insert(name.clone());
        return state;
    }
    if let ExprKind::ArrayLiteral(items) = &lhs.kind {
        for item in items {
            state = assign_target(&item.value, state, ctx, declaring_class);
        }
        return state;
    }
    walk_expr(lhs, state, ctx, declaring_class)
}

fn absorb(
    target_class: &str,
    method_name: &str,
    state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
) -> PropertyInitState {
    let resolved = ctx
        .checker
        .registry
        .lookup_method_with_owner(target_class, method_name);
    absorb_resolved(resolved, method_name, state, ctx)
}

/// Recursively walks into an already-resolved call target's own body,
/// seeded with the caller's current state as its own entry set, sharing
/// the same `ctx` - the call site's own state becomes the called method's
/// own exit state, i.e. calling into a method absorbs whatever it's
/// proven to guarantee. A target with no real body to walk (abstract, an
/// interface method, or simply unresolvable - `resolved` is `None`)
/// contributes nothing: the caller's own state passes through unchanged,
/// the same conservative "don't know, don't guess" default this project
/// uses everywhere else.
fn absorb_resolved(
    resolved: Option<(String, &FunctionSig)>,
    method_name: &str,
    state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
) -> PropertyInitState {
    let Some((owner, sig)) = resolved else {
        return state;
    };
    let Some(body) = sig.body.clone() else {
        return state;
    };
    let key = (owner.to_ascii_lowercase(), method_name.to_ascii_lowercase());
    if !ctx.visiting.insert(key.clone()) {
        return state;
    }
    let result = walk_stmts(body_as_ref(&body), state, ctx, &owner);
    ctx.visiting.remove(&key);
    result
}

fn body_as_ref(body: &Arc<Vec<Stmt>>) -> &[Stmt] {
    body.as_slice()
}
