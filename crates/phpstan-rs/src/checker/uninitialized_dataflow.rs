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
//! **Loops, and why `unset` complicates them**: without `unset`, this
//! domain only ever *grows* (a monotonic union), which would make the
//! worst case reachable through a `while`/`for`/`foreach` loop (which
//! might run zero times) always exactly the state before the loop. With
//! `unset` now real, that's no longer quite true in general (a property
//! could be unset on some iteration), so each of these three loop kinds
//! intersects the zero-iteration state with the state after walking the
//! body once - exact for the common monotonic case, and still sound for
//! a single iteration unsetting something. A second-or-later iteration
//! unsetting something the first iteration didn't would need a real
//! fixed point to close that gap; this is out of scope and errs safe
//! (could only miss a real premature-access finding, never invent a
//! false one). `do`/`while` needs none of this: its body runs at least
//! once, so the state after walking it exactly once is the exact answer.
//!
//! **A disclosed simplification**: `switch`/`match` arms are treated as
//! mutually-exclusive alternatives intersected together (matching
//! `merge_scopes`'s "only what every branch guarantees" rule), not real
//! PHP `switch` fallthrough between adjacent cases with no `break`.
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
use std::sync::Arc;

use crate::checker::Checker;
use crate::checker::uninitialized::property_assigned_anywhere_in_hierarchy;
use crate::registry::FunctionSig;
use crate::types::Type;
use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::child_exprs;

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
    /// globally uninitialized.
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
        let candidates: FxHashSet<String> = c
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
            .map(|p| p.name.to_string())
            .collect();
        if candidates.is_empty() {
            return;
        }
        let mut ctx = PropertyInitCtx {
            checker: &*self,
            checked_class: fqn.to_string(),
            candidates,
            visiting: FxHashSet::from_iter([(fqn.to_ascii_lowercase(), "__construct".to_string())]),
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
    matches!(&e.kind, ExprKind::Variable(v) if **v == *"this")
}

/// Whether `e` evaluates to the very same object `$this` refers to -
/// either literally `$this`, or a fluent method call chain rooted in
/// `$this` where every link's declared return type is `self`/`static`
/// (`public function setX(...): self { $this->x = ...; return $this; }`).
/// Recurses through any chain depth (`$this->a()->b()->c()`). A method
/// whose return type resolves to anything else (or isn't found at all)
/// stops the chain - the call itself is still walked normally by the
/// caller, just never treated as a further `$this`-rooted absorption
/// target beyond that point.
fn resolves_to_this(e: &Expr, ctx: &PropertyInitCtx) -> bool {
    match &e.kind {
        ExprKind::Variable(v) => **v == *"this",
        ExprKind::MethodCall(recv, name, _, _) if resolves_to_this(recv, ctx) => ctx
            .checker
            .registry
            .lookup_method(&ctx.checked_class, name)
            .is_some_and(|sig| {
                matches!(
                    sig.return_type_native,
                    Type::SelfRef(_) | Type::StaticRef(_) | Type::This
                )
            }),
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
/// however deep. Recurses through `PropertyAccess`/`DynamicPropertyAccess`/
/// `Index`, the shapes that make up a probed chain (`$this->a->b['c']`),
/// without ever registering a premature-read finding for a `$this->prop`
/// found along it; an index/key expression embedded inside the chain
/// (`$this->arr[$this->k]`) still gets the ordinary, non-quiet walk via
/// `walk_expr` - only the chain actually being tested is quiet.
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
/// checker's `always_terminates`) for whether a statement unconditionally
/// exits the enclosing method - `Return`/`Throw`/`Break`/`Continue`
/// directly, a `Block` containing one anywhere (not just as its last
/// statement), or an `If` where both branches do (a bare `if` with no
/// `else` can never qualify - the "condition false" path always falls
/// through). Used only to decide whether a branch's resulting state
/// should be excluded from a merge. Narrower than a full
/// `always_terminates` (no `Type::Never`-returning-call detection, no
/// `Switch`/`Try` exhaustiveness, since either needs a real
/// `Locals`/`infer_expr` this standalone module doesn't have) - errs
/// safe (could only miss a real finding downstream, never invent a false
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
            // A branch that unconditionally exits the method (an
            // `if (...) { return ...; }` guard-clause with no `else`)
            // doesn't get to vote on what's true afterward - otherwise
            // code placed after such a guard would be wrongly analyzed as
            // if it could also run on the path the guard already ruled
            // out.
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
        // the module doc comment for why intersecting the zero-iteration
        // state with the one-iteration state is the exact, sound answer.
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
        // once is the exact answer, not a lower bound.
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
            // Matches `checker/stmt.rs`'s existing `StmtKind::Try`
            // handling: each catch clause starts from a clone of the try
            // body's completed end state, not a conservative "could have
            // thrown before any statement" entry.
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
        // module deliberately doesn't descend into.
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
            if is_this(base) && ctx.candidates.contains(&*name.name) && !state.contains(&*name.name)
            {
                ctx.premature_reads.push((name.to_string(), e.span));
            }
            state
        }
        ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
            state = walk_expr(rhs, state, ctx, declaring_class);
            assign_target(lhs, state, ctx, declaring_class)
        }
        // `??=`'s isset-based left check never throws even when genuinely
        // uninitialized - never a premature-read site (quiet-probed,
        // exactly like bare `??`/`isset()` below), and always leaves the
        // property initialized afterward, exactly like a plain
        // assignment.
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
        // Bare `??` (not `??=`): its left side is exactly as quiet as
        // `isset()`'s argument (real PHP: `??`'s left-hand check is
        // defined in terms of `isset()`), for the whole probed chain, not
        // just a bare top-level property.
        ExprKind::Binary(BinOp::Coalesce, lhs, rhs) => {
            state = walk_quiet_probe(lhs, state, ctx, declaring_class);
            walk_expr(rhs, state, ctx, declaring_class)
        }
        // `isset($this->prop)`/`empty($this->prop)` - same quiet-probe
        // exemption as `??`'s left side, and neither one ever initializes
        // anything.
        ExprKind::Call(callee, args) if is_probe_call(callee) => {
            for a in args {
                state = walk_quiet_probe(&a.value, state, ctx, declaring_class);
            }
            state
        }
        // `unset($this->prop)` is neither a read nor an initializer:
        // `unset` never throws regardless of the property's current init
        // state, matching `check_expr`'s identical `unset`-recognition
        // (`checker/expr.rs`). Genuinely removes the property afterward
        // (a typed property, once unset, is uninitialized again).
        ExprKind::Call(callee, args) if is_unset_call(callee) => {
            for a in args {
                if let ExprKind::PropertyAccess(base, name, _) = &a.value.kind
                    && is_this(base)
                {
                    state = state.without(&*name.name);
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
            // fluent setter chain (`$this->setChannel(...)
            // ->setUsername(...)->includeContextAndExtra(...)`) has a
            // `MethodCall` as the receiver for every call after the
            // first, not literally `$this`, even though it evaluates to
            // the same object (a `: self`/`: static`-returning setter).
            if resolves_to_this(recv, ctx) {
                // Virtual dispatch: always against `checked_class` (the
                // original, most-derived class), never `declaring_class`
                // (whatever class the currently-recursed-into method body
                // happens to live on).
                let checked_class = ctx.checked_class.clone();
                state = absorb(&checked_class, name, state, ctx);
            }
            state
        }
        ExprKind::StaticCall(receiver, name, args) => {
            for a in args {
                state = walk_expr(&a.value, state, ctx, declaring_class);
            }
            // Only the three pseudo-keywords are resolved - a literal
            // class name (`Foo::bar()`) is out of scope, since this
            // walker only resolves to a real method on the same class -
            // same for a genuinely dynamic receiver, which has no name
            // to compare against these keywords at all.
            let is_receiver = |kw: &str| {
                receiver
                    .as_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case(kw))
            };
            let target = if is_receiver("parent") {
                // `parent::` is lexical, not virtual: it always means
                // "the immediate parent of whichever class the *current*
                // code is written in" - `declaring_class`'s own parent,
                // never `checked_class`'s.
                ctx.checker
                    .registry
                    .lookup_ancestor_method_with_owner(declaring_class, name)
            } else if is_receiver("self") {
                // `self::` is also lexical (no late static binding),
                // unlike `static::` below - real PHP resolves it against
                // the class the code is written in, walking that class's
                // own ancestors from there, not `checked_class`'s. Doesn't
                // distinguish an actual subclass override further down
                // `checked_class`'s own chain.
                ctx.checker
                    .registry
                    .lookup_method_with_owner(declaring_class, name)
            } else if is_receiver("static") {
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
            for child in child_exprs(e) {
                state = walk_expr(child, state, ctx, declaring_class);
            }
            state
        }
    }
}

/// A `$this->prop = ...`-shaped assignment target marks `prop` as
/// known-initialized from here on and is never itself a "read" (mirrors
/// `checker/assignment.rs`'s `check_assignment_target`). Also mirrors
/// that function's `ArrayLiteral` handling: a list-destructure like
/// `[$this->name, $this->callableRef] = ...` assigns two properties at
/// once; treating the whole left side as an ordinary expression would
/// wrongly read both as still-uninitialized at the very point they were
/// being assigned. Recurses through any nesting depth; an item's `key`
/// expression (`['k' => $x] = ...`) is a real read, not a target, and is
/// left to fall through to the ordinary expression walk. Any other shape
/// (`$x = ...`, `$this->arr[$k] = ...`, ...) isn't a tracked property
/// write, but its base/index subexpressions may still contain reads/
/// calls worth walking.
fn assign_target(
    lhs: &Expr,
    mut state: PropertyInitState,
    ctx: &mut PropertyInitCtx,
    declaring_class: &str,
) -> PropertyInitState {
    if let ExprKind::PropertyAccess(base, name, _) = &lhs.kind
        && is_this(base)
    {
        state.insert(name.to_string());
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

/// Recursively walks into an already-resolved call target's body, seeded
/// with the caller's current state as its entry set, sharing the same
/// `ctx` - calling into a method absorbs whatever it's proven to
/// guarantee. A target with no real body to walk (abstract, an interface
/// method, or simply unresolvable) contributes nothing: the caller's
/// state passes through unchanged.
fn absorb_resolved(
    resolved: Option<(std::sync::Arc<str>, &FunctionSig)>,
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
