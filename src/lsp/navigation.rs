//! Phase 17 Stage A: "what symbol is at this (file, position)" - the
//! shared first step both hover and go-to-definition need. Walks a
//! file's AST (already retained by `Project`, see `Project::ast`)
//! looking for the innermost name reference or declaration whose span
//! contains the cursor, resolving it against the file's own namespace/
//! `use`-imports (`NameContext`, the exact machinery `Checker`/
//! `Project::declared_names` already use) and, where the AST only has an
//! *enclosing* span to offer (`Foo::CONST`, `?Foo|Bar`), recovering the
//! real per-name sub-span via `text_split::split_identifier_spans`.
//!
//! Structured after (not sharing code with) `deps.rs`'s own
//! `collect_stmt`/`collect_expr`/`collect_function`/`collect_class`/
//! `collect_enum` - the same proven-exhaustive traversal shape (including
//! `ast_walk`'s own documented `stmt_children`/`expr_children`
//! exclusions: `FunctionDecl`/`ClassDecl`/`EnumDecl` bodies and
//! `Closure`/`ArrowFn`/`NewAnonymousClass`), reused here for a different
//! purpose (checking spans against a cursor, not collecting names into a
//! `HashSet`) rather than generalized, to avoid risking `deps.rs`'s own
//! existing, load-bearing dependency-tracking correctness for an
//! unrelated new consumer.
//!
//! Deliberately does **not** resolve an instance receiver's own type
//! (`$x->method()`, `$x->prop`) - `self`/`static`/`parent`/a literal
//! class name need no type inference at all (this module handles all of
//! those), but a real variable's type needs the checker's own machinery,
//! which Stage B (not yet built) adds as a separate, additive capability.

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::project::Project;
use crate::registry::{NameContext, Registry};
use crate::token::{Position, Span};
use std::path::Path;

use super::text_split::{split_identifier_spans, type_hint_identifiers};

#[derive(Debug, Clone)]
pub(crate) enum Resolved {
    Class(String),
    Function(String),
    /// A class member - method, property, or class-const, all looked up
    /// the same way (`Registry::lookup_method`/`lookup_property`/
    /// `lookup_const`, tried in that order by the caller) since a plain
    /// name reference alone doesn't say which kind it is.
    Member(String, String),
    /// Phase 17 Stage B: an instance receiver's own member access
    /// (`$x->method()`/`$x->prop`, `$x` not `$this`) - resolving *which*
    /// member this is needs `$x`'s inferred type, which needs the
    /// checker's own machinery, not just a name lookup. Everything
    /// `lsp.rs`'s own orchestration needs to run that probe: the
    /// receiver's own span (`Checker::probe_span`'s target - matched by
    /// exact span, not position, so a chained call's own inner/outer
    /// receivers are never ambiguous), which member name, whether it's a
    /// method call or a property read, and a clone of the enclosing
    /// function/method to re-check (owned, not borrowed - keeps this
    /// enum free of `Project`'s own lifetime, at the cost of one clone of
    /// a typically-small function body per request).
    Instance {
        receiver_span: Span,
        member: String,
        function: Box<FunctionDecl>,
        is_static: bool,
        class: Option<String>,
    },
    /// A local variable read, resolved to every declaration site it could
    /// really have come from (a parameter, an assignment's left-hand
    /// side, a `foreach` key/value binding, a `catch (...) $e` binding, or
    /// a `global $x;` linked back to a top-level assignment) - always
    /// same-file, so no registry lookup is needed, unlike every other
    /// variant here. Usually exactly one entry; more than one when the
    /// read is only reachable through mutually exclusive branches that
    /// each assign the name differently (see `Walker::var_decls`'s own
    /// doc comment) - offered as real alternatives (an LSP definition
    /// response can hand the editor more than one location), never
    /// silently collapsed to an arbitrary single guess.
    Variable(Vec<Span>),
}

/// The entry point: what's declared/referenced at `pos` in the file
/// loaded under `path`, or `None` for nothing resolvable there (dead
/// space, a plain variable, a receiver-typed call Stage B would need).
pub(crate) fn resolve_at(project: &Project, path: &Path, pos: Position) -> Option<Resolved> {
    let stmts = project.ast(path)?;
    let source = project.source(path)?;
    let registry = project.registry();
    let mut ctx = NameContext::default();
    let mut best = None;
    let mut w = Walker {
        registry,
        source,
        pos,
        current_function: None,
        current_is_static: false,
        var_decls: Vec::new(),
        conditional_depth: 0,
        // A dedicated, order-independent pre-scan (not the position-aware
        // walk below) - a `global $x;` inside a function can reference a
        // top-level assignment that appears *later* in the file than the
        // function's own declaration, which a single forward pass
        // wouldn't have seen yet.
        top_level_vars: collect_top_level_vars(stmts),
    };
    w.stmts(stmts, &mut ctx, None, &mut best);
    best
}

/// Every variable bound directly by `e` - a plain `$var`, or (recursing)
/// each leaf of a list-destructuring target (`[$a, $b] = ...`,
/// `foreach ($rows as [$a, $b])`). Shared by `Walker::bind_var` (the
/// position-aware walk) and `collect_top_level_vars` (the order-
/// independent pre-scan) below - the same "what names does this left-hand
/// side declare" question, asked in two different contexts.
fn collect_bound_names(e: &Expr, out: &mut Vec<(String, Span)>) {
    match &e.kind {
        ExprKind::Variable(name) => out.push((name.clone(), e.span)),
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                collect_bound_names(&item.value, out);
            }
        }
        _ => {}
    }
}

/// Every top-level (script-scope) variable declaration in the file, in
/// document order - the same shapes `Walker::declare_var`/`bind_var`
/// recognize (an assignment, a `foreach` binding, a `catch` binding, a
/// `static` declaration), but collected up front, order-independently,
/// purely so `global $x;` can link back to one even when it's declared
/// later in the file than the function using it. Recurses into ordinary
/// control-flow bodies (an `if`, a loop, ...) - real top-level code - but
/// never into a `FunctionDecl`/`ClassDecl`/`EnumDecl`/`Closure`/`ArrowFn`
/// body, each its own separate variable scope.
fn collect_top_level_vars(stmts: &[Stmt]) -> Vec<(String, Span)> {
    fn walk_stmt(stmt: &Stmt, out: &mut Vec<(String, Span)>) {
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) | StmtKind::EnumDecl(_) => return,
            StmtKind::Foreach { key, value, .. } => {
                if let Some(k) = key {
                    collect_bound_names(k, out);
                }
                collect_bound_names(value, out);
            }
            StmtKind::Try { catches, .. } => {
                for c in catches {
                    if let (Some(var), Some(span)) = (&c.var, c.var_span) {
                        out.push((var.clone(), span));
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, _) in vars {
                    out.push((name.clone(), stmt.span));
                }
            }
            _ => {}
        }
        walk_expr_for_assigns(stmt, out);
        for child in stmt_children(stmt) {
            walk_stmt(child, out);
        }
    }
    // Assignments can appear inside any expression position (a condition,
    // an argument, ...), not just a standalone `Expression` statement -
    // reusing `ast_walk::expr_children` for a full expression-tree walk
    // rather than hand-enumerating every `StmtKind` field that might hold
    // one.
    fn walk_expr(e: &Expr, out: &mut Vec<(String, Span)>) {
        if let ExprKind::Assign(lhs, _) | ExprKind::CompoundAssign(_, lhs, _) = &e.kind {
            collect_bound_names(lhs, out);
        }
        // A closure/arrow-fn body is its own separate scope - never
        // descended into here, matching `walk_stmt`'s own exclusion of
        // `FunctionDecl`/`ClassDecl`/`EnumDecl` above.
        if matches!(e.kind, ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }) {
            return;
        }
        for child in expr_children(e) {
            walk_expr(child, out);
        }
    }
    fn walk_expr_for_assigns(stmt: &Stmt, out: &mut Vec<(String, Span)>) {
        match &stmt.kind {
            StmtKind::Expression(e) | StmtKind::Throw(e) => walk_expr(e, out),
            StmtKind::Return(Some(e)) => walk_expr(e, out),
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    walk_expr(e, out);
                }
            }
            StmtKind::If { cond, .. }
            | StmtKind::While { cond, .. }
            | StmtKind::DoWhile { cond, .. } => walk_expr(cond, out),
            StmtKind::For {
                init, cond, step, ..
            } => {
                for e in init.iter().chain(cond).chain(step) {
                    walk_expr(e, out);
                }
            }
            StmtKind::Foreach { subject, .. } => walk_expr(subject, out),
            StmtKind::Switch { subject, cases } => {
                walk_expr(subject, out);
                for case in cases {
                    if let Some(t) = &case.test {
                        walk_expr(t, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for stmt in stmts {
        walk_stmt(stmt, &mut out);
    }
    out
}

/// 1-indexed, half-open (`span.end` is one past the last real character -
/// confirmed against the lexer's own token-span construction) containment
/// check.
fn contains(span: Span, pos: Position) -> bool {
    let after_start =
        span.start.line < pos.line || (span.start.line == pos.line && span.start.col <= pos.col);
    let before_end =
        pos.line < span.end.line || (pos.line == span.end.line && pos.col < span.end.col);
    after_start && before_end
}

/// Resolves a bare class-name-shaped reference (`self`/`static`/`parent`,
/// or an ordinary/namespaced name) to a real FQN - the same combined
/// logic `Checker::resolve_name_expr`/`resolve_static_class_name`/
/// `resolve_pseudo` provide together, minus the `Type`-wrapping and
/// union/template recursion neither of those own extra cases apply to a
/// bare class-name site.
fn resolve_class_ref(
    name: &str,
    current_class: Option<&str>,
    ctx: &NameContext,
    registry: &Registry,
) -> Option<String> {
    match name {
        "?" | "{dynamic}" => None,
        "self" | "static" => current_class.map(str::to_string),
        "parent" => registry
            .classes
            .get(current_class?)
            .and_then(|info| info.extends.first())
            .cloned(),
        _ => Some(ctx.resolve(name)),
    }
}

/// Every declaration `name` could really resolve to, scanning `entries`
/// (a `var_decls`-shaped `(name, span, conditional)` list) in reverse:
/// the most recent *unconditional* declaration is collected and scanning
/// stops there - nothing further back could still be live, since an
/// unconditional write is guaranteed to have happened on every path that
/// reaches this point. Every *conditional* declaration found along the
/// way (a loop body that may run zero times, or one arm of a branch that
/// may not be the one that ran) is collected too, as a genuine
/// alternative rather than being silently dropped - see `Resolved::
/// Variable`'s own doc comment for why a caller then offers all of them.
/// Empty when `name` was never declared among `entries` at all.
fn lookup_in(entries: &[(String, Span, bool)], name: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    for (n, span, conditional) in entries.iter().rev() {
        if n != name {
            continue;
        }
        spans.push(*span);
        if !conditional {
            break;
        }
    }
    spans
}

struct Walker<'a> {
    registry: &'a Registry,
    source: &'a str,
    pos: Position,
    // Phase 17 Stage B: an owned clone (not a borrow - see `Resolved::
    // Instance`'s own doc comment for why) of whichever *named*
    // `FunctionDecl`/`MethodDecl` currently encloses the statement being
    // walked, updated on entry to `function` and restored on exit - `None`
    // at top-level script scope (Stage B doesn't support probing there; a
    // real, disclosed, low-impact gap, see ROADMAP). Deliberately *not*
    // updated for a `Closure`/`ArrowFn` body: those aren't a separate
    // `Checker` entry point at all - `check_expr`'s own `Closure`/`ArrowFn`
    // arms already recurse into a closure's body as part of ordinarily
    // checking whatever named function contains the closure literal, so
    // leaving `current_function` pointing at that enclosing named function
    // (not resetting it) is exactly what lets `probe_receiver_type` find a
    // receiver inside a closure body too - re-running `check_function` on
    // the *named* function reaches the nested closure the same way a real
    // `check-project` pass already does. Confirmed empirically this
    // follow-up (a closure-body `$captured->method()` and an arrow-fn body
    // already resolved correctly with no further change needed here - the
    // original disclosed gap was overly conservative).
    current_function: Option<FunctionDecl>,
    current_is_static: bool,
    // Phase 17 follow-up: local-variable-declaration navigation. Every
    // `(name, span, conditional)` binding seen so far in the *current*
    // function/closure/top-level scope, in document order.
    // `conditional` (true whenever `conditional_depth > 0` at the moment
    // it's pushed) marks a declaration made somewhere that might not have
    // executed at all by the time a later read is reached - a loop body
    // (may run zero times), or one arm of an `if`/`switch`/`try` (only
    // one of several sibling arms ever really runs). `lookup_var` uses
    // this to decide how far back it's safe to stop looking: the most
    // recent *unconditional* declaration is always a real answer (nothing
    // before it could still be "the" value), but every conditional
    // declaration found on the way there is also a real, live possibility
    // and gets offered as an alternative rather than silently dropped -
    // see `Resolved::Variable`'s own doc comment. Reset to a fresh, empty
    // `Vec` (and `conditional_depth` reset to 0) on entry to a named
    // function/method or a `Closure` (PHP's own two real variable-scope
    // boundaries - a `Closure` only sees its own params plus whatever its
    // `use (...)` clause explicitly names, copied in from the *outer*
    // scope's own `var_decls` at that point). An `ArrowFn` does *not* get
    // a fresh scope (it implicitly captures the whole enclosing scope by
    // value, the real PHP semantics `checker/expr.rs`'s own `ArrowFn` arm
    // already relies on) - its own params are pushed and popped around
    // just its one body expression instead.
    var_decls: Vec<(String, Span, bool)>,
    // Incremented while walking a loop body or a branch arm (`if`/
    // `switch`/`try`'s own then/else/case/catch bodies), decremented on
    // exit - see `var_decls`'s own doc comment for what this drives.
    conditional_depth: u32,
    // Phase 17 follow-up: `global $x;` linking. Every top-level (script-
    // scope) variable declaration in the whole file, order-independently
    // pre-scanned once by `collect_top_level_vars` - `global` needs this
    // rather than `var_decls` itself because the top-level assignment it
    // refers to is very often declared *after* the function using it,
    // textually, which a single forward walk wouldn't have reached yet.
    top_level_vars: Vec<(String, Span)>,
}

impl<'a> Walker<'a> {
    /// Records `span` as `name`'s own declaration site, and - if the
    /// cursor lands directly on the declaration itself (clicking an
    /// assignment's own left-hand side, a parameter, ...) - resolves it
    /// to itself right here, same as every other declaration-site `record`
    /// call in this module does.
    fn declare_var(&mut self, name: &str, span: Span, best: &mut Option<Resolved>) {
        if contains(span, self.pos) {
            *best = Some(Resolved::Variable(vec![span]));
        }
        self.var_decls
            .push((name.to_string(), span, self.conditional_depth > 0));
    }

    /// Every declaration `name` could really resolve to in the current
    /// scope - see `lookup_in`'s own doc comment for the algorithm.
    fn lookup_var(&self, name: &str) -> Vec<Span> {
        lookup_in(&self.var_decls, name)
    }

    /// One branch arm's own newly-added declarations (`var_decls[base..]`
    /// after walking it), collapsed to each name's own *last* write
    /// within just this arm - an arm reassigning the same name twice in a
    /// row should only ever contribute its final value as a merge
    /// candidate, not every intermediate one. Used by `If`/`Switch`/
    /// `Try`'s own arm-by-arm walk: truncate back to `base`, walk one
    /// arm, call this, repeat for the next arm, then merge every arm's
    /// own result back together (see each call site's own comment).
    fn take_arm_result(&self, base: usize) -> Vec<(String, Span, bool)> {
        let mut per_name_last: Vec<(String, Span, bool)> = Vec::new();
        for (n, s, c) in &self.var_decls[base..] {
            if let Some(existing) = per_name_last.iter_mut().find(|(en, _, _)| en == n) {
                *existing = (n.clone(), *s, *c);
            } else {
                per_name_last.push((n.clone(), *s, *c));
            }
        }
        per_name_last
    }

    fn record(&self, best: &mut Option<Resolved>, span: Span, value: Resolved) {
        if contains(span, self.pos) {
            *best = Some(value);
        }
    }

    /// Resolves a compound type-hint's own span (`?Foo`, `Foo|Bar`) to
    /// whichever single class name inside it the cursor actually lands
    /// on, if any - via `split_identifier_spans` (section 3's text-split)
    /// paired positionally with `type_hint_identifiers`' own ordered
    /// extraction of the same hint text.
    fn record_type_hint(
        &self,
        best: &mut Option<Resolved>,
        hint: &str,
        span: Span,
        current_class: Option<&str>,
        ctx: &NameContext,
    ) {
        if !contains(span, self.pos) {
            return;
        }
        let names = type_hint_identifiers(hint);
        let expected: Vec<&str> = names.iter().map(String::as_str).collect();
        let Some(spans) = split_identifier_spans(self.source, span, &expected) else {
            return;
        };
        for (name, name_span) in names.iter().zip(spans.iter()) {
            if contains(*name_span, self.pos)
                && let Some(fqn) = resolve_class_ref(name, current_class, ctx, self.registry)
            {
                *best = Some(Resolved::Class(fqn));
            }
        }
    }

    fn stmts(
        &mut self,
        stmts: &[Stmt],
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<Resolved>,
    ) {
        for stmt in stmts {
            self.stmt(stmt, ctx, current_class, best);
        }
    }

    fn stmt(
        &mut self,
        stmt: &Stmt,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<Resolved>,
    ) {
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(imports),
            StmtKind::Expression(e) | StmtKind::Throw(e) => self.expr(e, ctx, current_class, best),
            StmtKind::Return(Some(e)) => self.expr(e, ctx, current_class, best),
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.expr(e, ctx, current_class, best);
                }
            }
            // Phase 17 follow-up: `if`/`else` are mutually exclusive arms
            // - at most one of them ever really runs. Each is walked from
            // the same starting point (`base`, truncated back to between
            // arms so one arm's own declarations can't leak into the
            // other), then their own results are merged back together:
            // a name reassigned differently by each arm keeps *both*
            // candidates (a later ambiguous read can't know which one
            // ran - see `Resolved::Variable`'s own doc comment), while a
            // name only one arm touches simply adds that one candidate
            // alongside whatever was already declared before the `if`
            // (still present, untouched, behind `base`).
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(cond, ctx, current_class, best);
                let base = self.var_decls.len();
                self.conditional_depth += 1;
                self.stmt(then_branch, ctx, current_class, best);
                let mut merged = self.take_arm_result(base);
                if let Some(else_branch) = else_branch {
                    self.var_decls.truncate(base);
                    self.stmt(else_branch, ctx, current_class, best);
                    merged.extend(self.take_arm_result(base));
                }
                self.conditional_depth -= 1;
                self.var_decls.truncate(base);
                self.var_decls.extend(merged);
                return;
            }
            // A `while`/`for`/`foreach` body may run zero times - no
            // arm-merging needed (there's no sibling "else"), just a
            // single conditional region wrapping the body/step, so a read
            // after the loop still sees whatever was declared before it
            // as a live alternative alongside anything the body itself
            // declared.
            StmtKind::While { cond, body } => {
                self.expr(cond, ctx, current_class, best);
                self.conditional_depth += 1;
                self.stmt(body, ctx, current_class, best);
                self.conditional_depth -= 1;
                return;
            }
            // Unlike every other loop shape, `do { ... } while (...);`
            // guarantees its body runs at least once - genuinely
            // unconditional relative to code after the loop (`break`
            // aside, not modeled - the same disclosed approximation every
            // other construct here makes), so no conditional region at
            // all, straight-line the same as ordinary sequential code.
            StmtKind::DoWhile { body, cond } => {
                self.stmt(body, ctx, current_class, best);
                self.expr(cond, ctx, current_class, best);
                return;
            }
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                for e in init.iter().chain(cond) {
                    self.expr(e, ctx, current_class, best);
                }
                self.conditional_depth += 1;
                self.stmt(body, ctx, current_class, best);
                for e in step {
                    self.expr(e, ctx, current_class, best);
                }
                self.conditional_depth -= 1;
                return;
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                body,
                ..
            } => {
                self.expr(subject, ctx, current_class, best);
                self.conditional_depth += 1;
                // `foreach`'s own key/value are *bindings*, not reads -
                // recorded as declaration sites (`bind_var`), not
                // resolved as if they were references to some earlier
                // declaration.
                if let Some(k) = key {
                    self.bind_var(k, ctx, current_class, best);
                }
                self.bind_var(value, ctx, current_class, best);
                self.stmt(body, ctx, current_class, best);
                self.conditional_depth -= 1;
                return;
            }
            // A `global $x;` inside a function links back to the
            // top-level (script-scope) declaration `collect_top_level_
            // vars` already found, order-independently, for the whole
            // file - the single most recent one, the same "one
            // representative answer" convention `global`'s own coarse,
            // non-block-scoped semantics already accept elsewhere. No
            // per-name span exists on this statement to pin an exact
            // click-here-in-the-`global`-line target to (only `stmt.span`
            // covers the whole `global $a, $b;`), so this only makes a
            // later *read* of the name resolve, not a click directly on
            // its mention in the `global` statement itself.
            StmtKind::Global(names) => {
                for name in names {
                    if let Some(span) = self
                        .top_level_vars
                        .iter()
                        .rev()
                        .find(|(n, _)| n == name)
                        .map(|(_, s)| *s)
                    {
                        self.declare_var(name, span, best);
                    }
                }
            }
            StmtKind::FunctionDecl(f) => {
                self.record(
                    best,
                    f.name_span,
                    Resolved::Function(ctx.declared_name(&f.name)),
                );
                self.function(f, &mut ctx.clone(), current_class, false, best);
            }
            StmtKind::ClassDecl(c) => self.class(c, ctx, best),
            StmtKind::EnumDecl(e) => self.enum_decl(e, ctx, best),
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.expr(&c.value, ctx, current_class, best);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, d) in vars {
                    if let Some(e) = d {
                        self.expr(e, ctx, current_class, best);
                    }
                    // `static $x = ...;` declares `$x` in the enclosing
                    // function's own scope - no per-name span exists for
                    // it (only the whole statement's), so the statement's
                    // own span stands in; imprecise (definition lands on
                    // the whole `static $x = 1, $y;` line rather than just
                    // `$x`) but never wrong-file.
                    self.declare_var(name, stmt.span, best);
                }
            }
            // Each `case` is its own exclusive arm (real PHP fallthrough
            // between cases without a `break` isn't modeled - a real,
            // disclosed approximation, not this construct's main
            // purpose), merged back the same way `If`'s then/else are.
            StmtKind::Switch { subject, cases } => {
                self.expr(subject, ctx, current_class, best);
                let base = self.var_decls.len();
                self.conditional_depth += 1;
                let mut merged: Vec<(String, Span, bool)> = Vec::new();
                for case in cases {
                    if let Some(t) = &case.test {
                        self.expr(t, ctx, current_class, best);
                    }
                    self.var_decls.truncate(base);
                    self.stmts(&case.body, ctx, current_class, best);
                    merged.extend(self.take_arm_result(base));
                }
                self.conditional_depth -= 1;
                self.var_decls.truncate(base);
                self.var_decls.extend(merged);
                return;
            }
            // The `try` body and each `catch` are mutually exclusive arms
            // (approximating: real PHP semantics are subtler - an
            // exception can interrupt the `try` body partway through,
            // making even *its own* declarations before that point real,
            // but modeling exactly where is its own control-flow problem
            // this heuristic doesn't attempt), merged the same way. A
            // `finally` block always runs after either, so it's walked
            // *after* the merge, back at the outer (non-conditional)
            // depth, seeing the merged, possibly-ambiguous result.
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                let base = self.var_decls.len();
                self.conditional_depth += 1;
                self.stmts(body, ctx, current_class, best);
                let mut merged = self.take_arm_result(base);
                for c in catches {
                    for (name, span) in c.types.iter().zip(c.types_spans.iter()) {
                        if contains(*span, self.pos)
                            && let Some(fqn) =
                                resolve_class_ref(name, current_class, ctx, self.registry)
                        {
                            *best = Some(Resolved::Class(fqn));
                        }
                    }
                    self.var_decls.truncate(base);
                    // The catch's own bound variable (`catch (Foo $e)`) is
                    // a declaration site too - recorded before the catch
                    // body itself is walked, so a read inside it already
                    // sees it.
                    if let (Some(var), Some(span)) = (&c.var, c.var_span) {
                        self.declare_var(var, span, best);
                    }
                    self.stmts(&c.body, ctx, current_class, best);
                    merged.extend(self.take_arm_result(base));
                }
                self.conditional_depth -= 1;
                self.var_decls.truncate(base);
                self.var_decls.extend(merged);
                if let Some(finally_body) = finally {
                    self.stmts(finally_body, ctx, current_class, best);
                }
                return;
            }
            _ => {}
        }
        for child in stmt_children(stmt) {
            self.stmt(child, ctx, current_class, best);
        }
    }

    fn expr(
        &mut self,
        expr: &Expr,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<Resolved>,
    ) {
        match &expr.kind {
            // `foo()`'s callee - a bare `Name` here is always a function
            // reference (never a class), unlike a bare `Name` reached
            // some other way (see the plain `ExprKind::Name` arm below).
            // Handled here, before generic recursion, specifically so
            // that arm never has to guess which one a given `Name` node
            // means.
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind
                    && let Some(fqn) = resolve_class_ref(n, current_class, ctx, self.registry)
                {
                    self.record(best, callee.span, Resolved::Function(fqn));
                } else {
                    self.expr(callee, ctx, current_class, best);
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            // `new Foo(...)`'s callee - always a class reference, the
            // mirror image of `Call` just above.
            ExprKind::New(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind
                    && let Some(fqn) = resolve_class_ref(n, current_class, ctx, self.registry)
                {
                    self.record(best, callee.span, Resolved::Class(fqn));
                } else {
                    self.expr(callee, ctx, current_class, best);
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            // A bare `Name` reached any *other* way (a first-class
            // callable target `foo(...)`, ...) - neither `Call` nor
            // `New` disambiguated it, so ask the registry which one
            // actually exists, preferring a function (the far more
            // common shape for a standalone name reference).
            ExprKind::Name(n) => {
                if let Some(fqn) = resolve_class_ref(n, current_class, ctx, self.registry) {
                    let resolved = if self.registry.lookup_function(&fqn).is_some() {
                        Resolved::Function(fqn)
                    } else {
                        Resolved::Class(fqn)
                    };
                    self.record(best, expr.span, resolved);
                }
            }
            ExprKind::StaticAccess(class, member) | ExprKind::StaticCall(class, member, _) => {
                if contains(expr.span, self.pos)
                    && let Some(fqn) = resolve_class_ref(class, current_class, ctx, self.registry)
                    && let Some(spans) = split_identifier_spans(
                        self.source,
                        expr.span,
                        &[class.as_str(), member.as_str()],
                    )
                {
                    if contains(spans[0], self.pos) {
                        *best = Some(Resolved::Class(fqn));
                    } else if contains(spans[1], self.pos)
                        && member != "class"
                        && member != "{dynamic}"
                    {
                        let member = member.strip_prefix('$').unwrap_or(member);
                        *best = Some(Resolved::Member(fqn, member.to_string()));
                    }
                }
            }
            ExprKind::Instanceof(_, InstanceofTarget::Name(name, span)) => {
                if contains(*span, self.pos)
                    && let Some(fqn) = resolve_class_ref(name, current_class, ctx, self.registry)
                {
                    *best = Some(Resolved::Class(fqn));
                }
            }
            // Phase 17 follow-up: local-variable-declaration navigation.
            // The right-hand side is a read (of whatever the variable held
            // *before* this assignment) and is walked first; the left-hand
            // side, when it's a plain `$var` (not a destructuring target -
            // see `bind_var`'s own doc comment for that residual gap), is
            // then recorded as a fresh declaration site, not walked as an
            // ordinary read - so `$x = $x + 1;`'s own right-hand `$x`
            // still resolves to whichever declaration preceded this
            // statement, not to itself.
            ExprKind::Assign(lhs, rhs) | ExprKind::CompoundAssign(_, lhs, rhs) => {
                self.expr(rhs, ctx, current_class, best);
                self.bind_var(lhs, ctx, current_class, best);
                return;
            }
            // A plain variable read (`$this` excluded - it's never a
            // locally-declared variable, and is resolved separately via
            // `current_class` wherever it appears as a receiver).
            ExprKind::Variable(name) if name != "this" => {
                if contains(expr.span, self.pos) {
                    let spans = self.lookup_var(name);
                    if !spans.is_empty() {
                        *best = Some(Resolved::Variable(spans));
                    }
                }
            }
            // `$this->method()`/`$this->prop` resolve directly (only
            // `current_class` is needed, already in hand); any *other*
            // receiver needs Stage B's own type-inference probe - this
            // records enough (the receiver's own span, the member name,
            // and a clone of the enclosing function/method to re-check)
            // for `lsp.rs`'s own orchestration to run that probe.
            ExprKind::MethodCall(recv, method, _, _)
            | ExprKind::PropertyAccess(recv, method, _) => {
                self.expr(recv, ctx, current_class, best);
                if contains(expr.span, self.pos)
                    && let Some(spans) =
                        split_identifier_spans(self.source, expr.span, &[method.as_str()])
                    && contains(spans[0], self.pos)
                {
                    if matches!(&recv.kind, ExprKind::Variable(v) if v == "this")
                        && let Some(owner) = current_class
                    {
                        *best = Some(Resolved::Member(owner.to_string(), method.clone()));
                    } else if let Some(function) = self.current_function.clone() {
                        *best = Some(Resolved::Instance {
                            receiver_span: recv.span,
                            member: method.clone(),
                            function: Box::new(function),
                            is_static: self.current_is_static,
                            class: current_class.map(str::to_string),
                        });
                    }
                }
                for a in expr_children(expr) {
                    if !std::ptr::eq(a, recv.as_ref()) {
                        self.expr(a, ctx, current_class, best);
                    }
                }
                return;
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                self.class(decl, &mut ctx.clone(), best);
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::Closure {
                params, uses, body, ..
            } => {
                // A `Closure` only ever sees its own params plus whatever
                // its `use (...)` clause explicitly names (the real PHP
                // rule `checker/expr.rs`'s own `Closure` arm already
                // relies on) - copied in from the *outer* scope's own
                // `var_decls` at this exact point, not the whole outer
                // scope wholesale the way an `ArrowFn` gets below.
                let outer_var_decls = std::mem::take(&mut self.var_decls);
                let outer_conditional_depth = std::mem::replace(&mut self.conditional_depth, 0);
                for u in uses {
                    // Every real candidate the outer scope has for this
                    // captured name becomes the closure's own starting
                    // point for it too - a captured variable can be just
                    // as ambiguous at the closure literal's own position
                    // as an ordinary read would be there.
                    for span in lookup_in(&outer_var_decls, &u.name) {
                        self.var_decls.push((u.name.clone(), span, true));
                    }
                }
                for p in params {
                    self.param(p, ctx, current_class, best);
                }
                self.stmts(body, &mut ctx.clone(), current_class, best);
                self.var_decls = outer_var_decls;
                self.conditional_depth = outer_conditional_depth;
                return;
            }
            ExprKind::ArrowFn { params, body, .. } => {
                // An `ArrowFn` implicitly captures the whole enclosing
                // scope by value - no fresh scope, just its own params
                // pushed and popped around its one body expression.
                let scope_mark = self.var_decls.len();
                for p in params {
                    self.param(p, ctx, current_class, best);
                }
                self.expr(body, &mut ctx.clone(), current_class, best);
                self.var_decls.truncate(scope_mark);
                return;
            }
            _ => {}
        }
        for child in expr_children(expr) {
            self.expr(child, ctx, current_class, best);
        }
    }

    fn param(
        &mut self,
        p: &Param,
        ctx: &NameContext,
        current_class: Option<&str>,
        best: &mut Option<Resolved>,
    ) {
        if let (Some(hint), Some(span)) = (&p.type_hint, p.type_hint_span) {
            self.record_type_hint(best, hint, span, current_class, ctx);
        }
        self.declare_var(&p.name, p.name_span, best);
    }

    /// A binding target - an assignment's left-hand side, or a `foreach`
    /// key/value - is a declaration site, not a read, including every
    /// leaf of a list-destructuring target (`[$a, $b] = ...`, `foreach
    /// ($rows as [$a, $b])`). Anything else (a property/index write
    /// target, `$this->x = ...`/`$arr[$i] = ...`) falls back to an
    /// ordinary read-walk - correct, not a fallback-of-convenience: that
    /// shape doesn't declare a new local variable at all.
    fn bind_var(
        &mut self,
        e: &Expr,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<Resolved>,
    ) {
        match &e.kind {
            ExprKind::Variable(name) => self.declare_var(name, e.span, best),
            // List-destructuring (`[$a, $b] = ...`, `foreach ($rows as
            // [$a, $b])`) - each leaf is its own declaration site; a
            // key expression (`['k' => $v]`) is an ordinary read, not a
            // binding.
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    if let Some(k) = &item.key {
                        self.expr(k, ctx, current_class, best);
                    }
                    self.bind_var(&item.value, ctx, current_class, best);
                }
            }
            _ => self.expr(e, ctx, current_class, best),
        }
    }

    fn function(
        &mut self,
        f: &FunctionDecl,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        is_static: bool,
        best: &mut Option<Resolved>,
    ) {
        // A named function/method is one of PHP's two real variable-scope
        // boundaries (the other is a `Closure` literal, see `var_decls`'s
        // own doc comment) - fresh scope, seeded below by its own params,
        // and a fresh (unconditional) `conditional_depth` too: whatever
        // branch/loop nesting textually surrounds this declaration is
        // irrelevant to what's unconditionally true *inside* its own body.
        let previous_var_decls = std::mem::take(&mut self.var_decls);
        let previous_conditional_depth = std::mem::replace(&mut self.conditional_depth, 0);
        for p in &f.params {
            self.param(p, ctx, current_class, best);
        }
        if let (Some(rt), Some(span)) = (&f.return_type, f.return_type_span) {
            self.record_type_hint(best, rt, span, current_class, ctx);
        }
        let previous_function = self.current_function.replace(f.clone());
        let previous_is_static = std::mem::replace(&mut self.current_is_static, is_static);
        self.stmts(&f.body, ctx, current_class, best);
        self.current_function = previous_function;
        self.current_is_static = previous_is_static;
        self.var_decls = previous_var_decls;
        self.conditional_depth = previous_conditional_depth;
    }

    fn class(&mut self, c: &ClassDecl, ctx: &mut NameContext, best: &mut Option<Resolved>) {
        let fqn = ctx.declared_name(&c.name);
        self.record(best, c.name_span, Resolved::Class(fqn.clone()));
        for (name, span) in c.extends.iter().zip(c.extends_spans.iter()) {
            if contains(*span, self.pos)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                *best = Some(Resolved::Class(target));
            }
        }
        for (name, span) in c.implements.iter().zip(c.implements_spans.iter()) {
            if contains(*span, self.pos)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                *best = Some(Resolved::Class(target));
            }
        }
        for (name, span) in c.uses.iter().zip(c.uses_spans.iter()) {
            if contains(*span, self.pos)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                *best = Some(Resolved::Class(target));
            }
        }
        for p in &c.properties {
            self.record(
                best,
                p.name_span,
                Resolved::Member(fqn.clone(), p.name.clone()),
            );
            if let (Some(hint), Some(span)) = (&p.type_hint, p.type_hint_span) {
                self.record_type_hint(best, hint, span, Some(&fqn), ctx);
            }
            if let Some(d) = &p.default {
                self.expr(d, &mut ctx.clone(), Some(&fqn), best);
            }
        }
        for m in &c.methods {
            self.record(
                best,
                m.function.name_span,
                Resolved::Member(fqn.clone(), m.function.name.clone()),
            );
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, best);
        }
        for cst in &c.consts {
            self.record(
                best,
                cst.name_span,
                Resolved::Member(fqn.clone(), cst.name.clone()),
            );
            if let (Some(hint), Some(span)) = (&cst.type_hint, cst.type_hint_span) {
                self.record_type_hint(best, hint, span, Some(&fqn), ctx);
            }
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), best);
        }
    }

    fn enum_decl(&mut self, e: &EnumDecl, ctx: &mut NameContext, best: &mut Option<Resolved>) {
        let fqn = ctx.declared_name(&e.name);
        self.record(best, e.name_span, Resolved::Class(fqn.clone()));
        for (name, span) in e.implements.iter().zip(e.implements_spans.iter()) {
            if contains(*span, self.pos)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                *best = Some(Resolved::Class(target));
            }
        }
        for (name, span) in e.uses.iter().zip(e.uses_spans.iter()) {
            if contains(*span, self.pos)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                *best = Some(Resolved::Class(target));
            }
        }
        for m in &e.methods {
            self.record(
                best,
                m.function.name_span,
                Resolved::Member(fqn.clone(), m.function.name.clone()),
            );
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, best);
        }
        for cst in &e.consts {
            self.record(
                best,
                cst.name_span,
                Resolved::Member(fqn.clone(), cst.name.clone()),
            );
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), best);
        }
        for case in &e.cases {
            self.record(
                best,
                case.name_span,
                Resolved::Member(fqn.clone(), case.name.clone()),
            );
            if let Some(v) = &case.value {
                self.expr(v, &mut ctx.clone(), Some(&fqn), best);
            }
        }
    }
}
