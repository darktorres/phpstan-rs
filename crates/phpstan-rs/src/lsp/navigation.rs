//! "What symbol is at this (file, position)" - the shared first step
//! both hover and go-to-definition need. Walks a
//! file's AST (already retained by `Project`, see `Project::ast`)
//! looking for the innermost name reference or declaration whose span
//! contains the cursor, resolving it against the file's own namespace/
//! `use`-imports (`NameContext`, the exact machinery `Checker`/
//! `Project::declared_names` already use). Each name in a compound
//! reference (`Foo::CONST`, `?Foo|Bar`) has its own span in the AST.
//!
//! Also the traversal `textDocument/documentHighlight` (`lsp::document_
//! highlight`... no such module - see `Server::document_highlight` in
//! `lsp.rs`) reuses wholesale via `collect_all`: the same walker, run
//! with `Anchor::All` instead of `Anchor::Position(pos)`, collects every
//! `(Span, Resolved)` pair in the file instead of stopping at the single
//! best match for one cursor position. `resolve_at` and `collect_all`
//! share one underlying `walk_file` - the only difference between "what
//! is at this exact position" and "everywhere in this file" is which
//! `Anchor` variant gates whether a candidate gets recorded, never the
//! traversal itself.
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
//! (`$x->method()`, `$x->prop`) or a local variable *read*'s own
//! declaration site - both need real type-inference/dataflow machinery
//! this module has none of. `lsp.rs`'s own orchestration runs a real,
//! throwaway `Checker` probe for both (`Resolved::Instance`/`Resolved::
//! VariableRead`'s own doc comments), reusing the checker's already-
//! correct flow-sensitive analysis (branch merging, loop fixed-point
//! convergence, termination-pruning) rather than a second, hand-rolled
//! approximation of it - see `Checker::probe_var_provenance`'s own doc
//! comment in `checker.rs` for the full design.

use std::sync::Arc;

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::name_resolution::NameContext;
use crate::project::Project;
use crate::registry::Registry;
use crate::token::{Position, Span};
use rustc_hash::FxHashSet;
use std::path::Path;

#[derive(Debug, Clone)]
pub(crate) enum Resolved {
    Class(String),
    Function(String),
    /// A class member - method, property, or class-const, all looked up
    /// the same way (`Registry::lookup_method`/`lookup_property`/
    /// `lookup_const`, tried in that order by the caller) since a plain
    /// name reference alone doesn't say which kind it is.
    Member(String, String),
    /// An instance receiver's own member access
    /// (`$x->method()`/`$x->prop`) - resolving *which* member this is
    /// needs `$x`'s inferred type, which needs the checker's own
    /// machinery, not just a name lookup. Everything `lsp.rs`'s own
    /// orchestration needs to run that probe: the receiver's own span
    /// (`Checker::probe_span`'s target - matched by exact span, not
    /// position, so a chained call's own inner/outer receivers are never
    /// ambiguous), which member name, whether it's a method call or a
    /// property read, and the enclosing scope to re-check (see
    /// `ProbeScope`'s own doc comment).
    Instance {
        receiver_span: Span,
        member: String,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
    },
    /// A local variable's own declaration site, clicked directly (a
    /// parameter, a `foreach`/`catch`/`static`/`global` binding's own
    /// name) - always same-file, and the click's own position already
    /// *is* the answer for go-to-definition, so no probe is needed for
    /// that. `scope`/`is_static`/`class` exist for hover's own sake only
    /// (what type does this declaration itself bind? - which genuinely
    /// needs a probe, e.g. a `foreach` binding's type depends on
    /// inferring the iterable), mirroring `VariableRead`'s identical
    /// fields below. `name`/`var_scope` exist for `document_highlight`'s
    /// own sake only (which other occurrences are the *same* local -
    /// see `var_scope`'s own doc comment on `Walker`), unused by hover/
    /// definition.
    VariableSelf {
        span: Span,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
        name: Arc<str>,
        var_scope: u32,
    },
    /// A local variable *read* - resolved via a real, checker-driven
    /// reaching-definitions probe (`Checker::probe_span`/
    /// `probe_var_provenance`, the same mechanism `Instance` above uses
    /// for a receiver's own type), not a hand-rolled approximation. Might
    /// resolve to more than one real declaration site when the read is
    /// only reachable through mutually exclusive branches that assign the
    /// name differently - `lsp.rs` offers every one as a real alternative
    /// (a `GotoDefinitionResponse::Array`), never an arbitrary guess.
    VariableRead {
        span: Span,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
        name: Arc<str>,
        var_scope: u32,
    },
}

/// What `lsp.rs`'s own throwaway `Checker` probe re-checks to answer an
/// `Instance`/`VariableRead` request - a clone of the enclosing named
/// function/method (owned, not borrowed, so `Resolved` stays free of
/// `Project`'s own lifetime, at the cost of one clone of a typically-small
/// function body per request), or, at top-level script scope (no
/// enclosing function at all), a clone of the *whole file's* own top-level
/// statement list, re-checked via `Checker::check_program` instead of
/// `check_function` - real top-level PHP code is exactly as checkable as
/// a function body, just through a different entry point.
#[derive(Debug, Clone)]
pub(crate) enum ProbeScope {
    Function(Box<FunctionDecl>),
    TopLevel(Vec<Stmt>),
}

/// What a candidate `Span` is checked against - a single cursor position
/// (`resolve_at`'s own use, `.matches()` degenerating to the original
/// `contains()` check) or unconditionally everything in the file
/// (`collect_all`'s use, for `document_highlight` - every candidate the
/// walker would otherwise gate on cursor proximity is recorded instead).
pub(crate) enum Anchor {
    Position(Position),
    All,
}

impl Anchor {
    fn matches(&self, span: Span) -> bool {
        match self {
            Anchor::Position(pos) => contains(span, *pos),
            Anchor::All => true,
        }
    }
}

/// The entry point: what's declared/referenced at `pos` in the file
/// loaded under `path`, or `None` for nothing resolvable there. The
/// walk order matches `out`'s own push order, so `.last()` is exactly
/// the old single-`best`-overwritten-on-every-match semantics: the
/// innermost (visited last) containing match wins.
pub(crate) fn resolve_at(project: &Project, path: &Path, pos: Position) -> Option<Resolved> {
    walk_file(project, path, Anchor::Position(pos))?
        .into_iter()
        .map(|(_, r)| r)
        .next_back()
}

/// Every `(Span, Resolved)` pair anywhere in the file - `document_
/// highlight`'s own source list, filtered down to whichever ones are
/// the same symbol as the target the cursor is actually on.
pub(crate) fn collect_all(project: &Project, path: &Path) -> Vec<(Span, Resolved)> {
    walk_file(project, path, Anchor::All).unwrap_or_default()
}

fn walk_file(project: &Project, path: &Path, anchor: Anchor) -> Option<Vec<(Span, Resolved)>> {
    let stmts = project.ast(path)?;
    let registry = project.registry();
    let mut ctx = NameContext::default();
    let mut out = Vec::new();
    let mut w = Walker {
        registry,
        anchor,
        top_level_stmts: stmts,
        current_function: None,
        current_is_static: false,
        next_scope_id: 0,
        base_scope_id: 0,
        scope_frames: Vec::new(),
    };
    w.stmts(stmts, &mut ctx, None, &mut out);
    Some(out)
}

/// 1-indexed, half-open (`span.end` is one past the last real character -
/// confirmed against the lexer's own token-span construction) containment
/// check.
pub(super) fn contains(span: Span, pos: Position) -> bool {
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
pub(super) fn resolve_class_ref(
    name: &str,
    current_class: Option<&str>,
    ctx: &NameContext,
    registry: &Registry,
) -> Option<String> {
    match name {
        "self" | "static" => current_class.map(str::to_string),
        "parent" => registry
            .classes
            .get(current_class?)
            .and_then(|info| info.extends.first())
            .map(|s| s.to_string()),
        _ => Some(ctx.resolve(name)),
    }
}

/// A lexically-nested `Closure`/`ArrowFn` scope, pushed on entry and
/// popped on exit - see `Walker::scope_frames`'s own doc comment for how
/// `resolve_var_scope` reads this stack.
enum ScopeFrame {
    /// A `Closure` variable-shadowing boundary: a name NOT in its own
    /// `use (...)` list belongs to the closure's own scope (`id`); a
    /// name that IS in `transparent` is a real capture of an outer
    /// variable, so resolution keeps walking outward past this frame.
    Closure {
        id: u32,
        transparent: FxHashSet<Arc<str>>,
    },
    /// An arrow function's implicit-capture-by-value boundary: PHP's
    /// `fn() => ...` auto-captures every outer variable it reads BY
    /// VALUE, so (unlike `Closure`) there is no explicit capture list to
    /// consult - only its own declared parameters (`owns`) belong to a
    /// genuinely new scope; every other name resolves as the SAME
    /// variable as whatever it refers to outside the arrow function.
    ArrowFn { id: u32, owns: FxHashSet<Arc<str>> },
}

struct Walker<'a> {
    registry: &'a Registry,
    anchor: Anchor,
    // The whole file's own top-level statement list - `ProbeScope::
    // TopLevel`'s own source when a probe request lands at script scope
    // (`current_function` is `None`), cloned only then (one clone per
    // request, the same cost `ProbeScope::Function` already accepts for
    // an ordinary function-scoped probe).
    top_level_stmts: &'a [Stmt],
    // An owned clone (not a borrow - see `ProbeScope`'s own doc comment
    // for why) of whichever *named*
    // `FunctionDecl`/`MethodDecl` currently encloses the statement being
    // walked, updated on entry to `function` and restored on exit -
    // `None` at top-level script scope, in which case a probe request
    // uses `top_level_stmts` instead (see `probe_scope`). Deliberately
    // *not* updated for a `Closure`/`ArrowFn` body: those aren't a
    // separate `Checker` entry point at all - `check_expr`'s own
    // `Closure`/`ArrowFn` arms already recurse into a closure's body as
    // part of ordinarily checking whatever named function contains the
    // closure literal, so leaving `current_function` pointing at that
    // enclosing named function (not resetting it) is exactly what lets a
    // probe find a receiver/variable inside a closure body too -
    // re-running `check_function` on the *named* function reaches the
    // nested closure the same way a real `check-project` pass already
    // does.
    current_function: Option<FunctionDecl>,
    current_is_static: bool,
    // A purely syntactic id source for `document_highlight`'s own same-
    // variable grouping - unrelated to `ProbeScope`/`Checker::
    // probe_var_provenance`'s real flow-sensitive scoping (hover/
    // definition never read `var_scope` at all). Starts at `0` so the
    // first real nested scope's id (`1`) never collides with
    // `base_scope_id`'s own default of `0`.
    next_scope_id: u32,
    // The scope id a variable resolves to once `scope_frames` is
    // exhausted (falls off the end) - `0` by default, meaning "top-level
    // script scope or the current named function/method's own top-level
    // body," reassigned per-function by `function()`'s own swap. This
    // default state (0, no frames) IS the top-level-script-scope case
    // with no special-casing needed: a closure/arrow-fn declared
    // directly in top-level code still pushes its own frame around
    // itself exactly as it would inside a named function.
    base_scope_id: u32,
    // Innermost-last stack of currently-open `Closure`/`ArrowFn` scopes -
    // see `resolve_var_scope` for how a name is resolved against it.
    scope_frames: Vec<ScopeFrame>,
}

impl<'a> Walker<'a> {
    fn next_id(&mut self) -> u32 {
        self.next_scope_id += 1;
        self.next_scope_id
    }

    /// Which scope `name` actually belongs to, walking `scope_frames`
    /// innermost-first: a `Closure` frame claims `name` unless it's one
    /// of its own explicit `use (...)` captures (in which case resolution
    /// keeps walking outward looking for where that capture really
    /// lives); an `ArrowFn` frame claims `name` only if it's one of the
    /// arrow function's own parameters (everything else is an implicit
    /// by-value capture of whatever the name already means outside it).
    /// Falling off the end of the stack resolves to `base_scope_id`.
    fn resolve_var_scope(&self, name: &Arc<str>) -> u32 {
        for frame in self.scope_frames.iter().rev() {
            match frame {
                ScopeFrame::Closure { id, transparent } => {
                    if !transparent.contains(name) {
                        return *id;
                    }
                }
                ScopeFrame::ArrowFn { id, owns } => {
                    if owns.contains(name) {
                        return *id;
                    }
                }
            }
        }
        self.base_scope_id
    }

    /// The `ProbeScope` for a probe request at the current position - the
    /// enclosing named function/method if there is one, else the whole
    /// file's own top-level statements (real, checkable PHP in its own
    /// right, just via `Checker::check_program` instead of `check_
    /// function` - see `ProbeScope`'s own doc comment).
    fn probe_scope(&self) -> ProbeScope {
        match &self.current_function {
            Some(f) => ProbeScope::Function(Box::new(f.clone())),
            None => ProbeScope::TopLevel(self.top_level_stmts.to_vec()),
        }
    }

    fn record(&self, out: &mut Vec<(Span, Resolved)>, span: Span, value: Resolved) {
        if self.anchor.matches(span) {
            out.push((span, value));
        }
    }

    /// Records `span` as a local variable's own declaration site, self-
    /// resolving (no probe needed - the click's own position already is
    /// the answer) for go-to-definition when the cursor lands directly on
    /// it - `scope`/`is_static`/`class` are only for hover's own later,
    /// separate probe (see `Resolved::VariableSelf`'s own doc comment).
    fn record_var_self(
        &self,
        out: &mut Vec<(Span, Resolved)>,
        span: Span,
        current_class: Option<&str>,
        name: &Arc<str>,
    ) {
        self.record(
            out,
            span,
            Resolved::VariableSelf {
                span,
                scope: self.probe_scope(),
                is_static: self.current_is_static,
                class: current_class.map(str::to_string),
                name: name.clone(),
                var_scope: self.resolve_var_scope(name),
            },
        );
    }

    /// A binding target - a `foreach` key/value - is a declaration site,
    /// not a read, including every leaf of a list-destructuring target
    /// (`foreach ($rows as [$a, $b])`). Anything else (rare: a property/
    /// index write target as a foreach binding isn't valid PHP at all)
    /// falls back to an ordinary read-walk.
    fn bind_var(
        &mut self,
        e: &Expr,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, Resolved)>,
    ) {
        match &e.kind {
            ExprKind::Variable(name) => self.record_var_self(out, e.span, current_class, name),
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    if let Some(k) = &item.key {
                        self.expr(k, ctx, current_class, out);
                    }
                    self.bind_var(&item.value, ctx, current_class, out);
                }
            }
            _ => self.expr(e, ctx, current_class, out),
        }
    }

    /// Resolves a compound type-hint (`?Foo`, `Foo|Bar`) to whichever
    /// single class name inside it the cursor lands on, if any. Under
    /// `Anchor::All`, every class name in the hint gets its own entry.
    fn record_type_hint(
        &self,
        out: &mut Vec<(Span, Resolved)>,
        hint: &TypeHint,
        current_class: Option<&str>,
        ctx: &NameContext,
    ) {
        if !self.anchor.matches(hint.span) {
            return;
        }
        for (name, name_span) in hint.class_names() {
            if self.anchor.matches(name_span)
                && let Some(fqn) = resolve_class_ref(name, current_class, ctx, self.registry)
            {
                out.push((name_span, Resolved::Class(fqn)));
            }
        }
    }

    fn stmts(
        &mut self,
        stmts: &[Stmt],
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, Resolved)>,
    ) {
        for stmt in stmts {
            self.stmt(stmt, ctx, current_class, out);
        }
    }

    fn stmt(
        &mut self,
        stmt: &Stmt,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, Resolved)>,
    ) {
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(imports),
            StmtKind::Expression(e) | StmtKind::Throw(e) => self.expr(e, ctx, current_class, out),
            StmtKind::Return(Some(e)) => self.expr(e, ctx, current_class, out),
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.expr(e, ctx, current_class, out);
                }
            }
            StmtKind::If { cond, .. }
            | StmtKind::While { cond, .. }
            | StmtKind::DoWhile { cond, .. } => {
                self.expr(cond, ctx, current_class, out);
            }
            StmtKind::For {
                init, cond, step, ..
            } => {
                for e in init.iter().chain(cond).chain(step) {
                    self.expr(e, ctx, current_class, out);
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                ..
            } => {
                self.expr(subject, ctx, current_class, out);
                if let Some(k) = key {
                    self.bind_var(k, ctx, current_class, out);
                }
                self.bind_var(value, ctx, current_class, out);
            }
            StmtKind::FunctionDecl(f) => {
                self.record(
                    out,
                    f.name_span,
                    Resolved::Function(ctx.declared_name(&f.name)),
                );
                self.function(f, &mut ctx.clone(), current_class, false, out);
            }
            StmtKind::ClassDecl(c) => self.class(c, ctx, out),
            StmtKind::EnumDecl(e) => self.enum_decl(e, ctx, out),
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.expr(&c.value, ctx, current_class, out);
                }
            }
            // `global $x;`/`static $x = ...;` have no per-name span of
            // their own (only the whole statement's) - the same
            // imprecision `Checker::declare`'s own instrumentation for
            // these two shapes accepts (`checker/stmt.rs`).
            StmtKind::Global(vars) => {
                for var in vars {
                    match var {
                        GlobalVar::Name(name) => {
                            self.record_var_self(out, stmt.span, current_class, name);
                        }
                        GlobalVar::Dynamic(e) => self.expr(e, ctx, current_class, out),
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, d) in vars {
                    if let Some(e) = d {
                        self.expr(e, ctx, current_class, out);
                    }
                    self.record_var_self(out, stmt.span, current_class, name);
                }
            }
            StmtKind::Switch { subject, cases } => {
                self.expr(subject, ctx, current_class, out);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.expr(t, ctx, current_class, out);
                    }
                }
            }
            StmtKind::Try { catches, .. } => {
                for c in catches {
                    for (name, span) in c.types.iter().zip(c.types_spans.iter()) {
                        if self.anchor.matches(*span)
                            && let Some(fqn) =
                                resolve_class_ref(name, current_class, ctx, self.registry)
                        {
                            out.push((*span, Resolved::Class(fqn)));
                        }
                    }
                    if let (Some(name), Some(span)) = (&c.var, c.var_span) {
                        self.record_var_self(out, span, current_class, name);
                    }
                }
            }
            _ => {}
        }
        for child in stmt_children(stmt) {
            self.stmt(child, ctx, current_class, out);
        }
    }

    fn expr(
        &mut self,
        expr: &Expr,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, Resolved)>,
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
                    self.record(out, callee.span, Resolved::Function(fqn));
                } else {
                    self.expr(callee, ctx, current_class, out);
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
                }
                return;
            }
            // `new Foo(...)`'s callee - always a class reference, the
            // mirror image of `Call` just above.
            ExprKind::New(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind
                    && let Some(fqn) = resolve_class_ref(n, current_class, ctx, self.registry)
                {
                    self.record(out, callee.span, Resolved::Class(fqn));
                } else {
                    self.expr(callee, ctx, current_class, out);
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
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
                    self.record(out, expr.span, resolved);
                }
            }
            ExprKind::ClassConstFetch(StaticTarget::Name(class, class_span), member) => {
                if self.anchor.matches(expr.span)
                    && let Some(fqn) = resolve_class_ref(class, current_class, ctx, self.registry)
                {
                    let spans = [*class_span, member.span];
                    // Two independent `if`s, not `if`/`else if`: under
                    // `Anchor::All` a single expression can contain both
                    // a class-name occurrence AND a member-name
                    // occurrence, and both need their own entry.
                    // `resolve_at`'s own behavior is unaffected - a
                    // single `Position` can still only ever match one of
                    // these two disjoint sub-spans.
                    if self.anchor.matches(spans[0]) {
                        out.push((spans[0], Resolved::Class(fqn.clone())));
                    }
                    if self.anchor.matches(spans[1]) {
                        out.push((spans[1], Resolved::Member(fqn, member.to_string())));
                    }
                }
            }
            ExprKind::ClassNameFetch(StaticTarget::Name(class, class_span)) => {
                if self.anchor.matches(*class_span)
                    && let Some(fqn) = resolve_class_ref(class, current_class, ctx, self.registry)
                {
                    out.push((*class_span, Resolved::Class(fqn)));
                }
            }
            // `Foo::method()` mirrors `ClassConstFetch`/`StaticPropertyFetch` above exactly (a
            // literal class name, resolved by plain name lookup). A
            // genuinely dynamic receiver (`$obj::method()`) has no
            // class-name text to resolve here at all - it needs the
            // exact same Stage B type-inference probe `MethodCall`'s own
            // "any other receiver" branch below already uses, `Resolved::
            // Instance` being agnostic to `->` vs `::` call syntax (a
            // real PHP static method is still just a method looked up
            // on the receiver's own resolved class either way).
            ExprKind::StaticCall(target, member, args) => {
                match target {
                    StaticTarget::Name(class, class_span) => {
                        if self.anchor.matches(expr.span)
                            && let Some(fqn) =
                                resolve_class_ref(class, current_class, ctx, self.registry)
                        {
                            let spans = [*class_span, member.span];
                            if self.anchor.matches(spans[0]) {
                                out.push((spans[0], Resolved::Class(fqn.clone())));
                            }
                            if self.anchor.matches(spans[1]) {
                                out.push((spans[1], Resolved::Member(fqn, member.to_string())));
                            }
                        }
                    }
                    StaticTarget::Expr(receiver) => {
                        self.expr(receiver, ctx, current_class, out);
                        if self.anchor.matches(member.span) {
                            out.push((
                                member.span,
                                Resolved::Instance {
                                    receiver_span: receiver.span,
                                    member: member.to_string(),
                                    scope: self.probe_scope(),
                                    is_static: self.current_is_static,
                                    class: current_class.map(str::to_string),
                                },
                            ));
                        }
                    }
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
                }
                return;
            }
            ExprKind::DynamicStaticCall(target, name, args) => {
                if let StaticTarget::Expr(receiver) = target {
                    self.expr(receiver, ctx, current_class, out);
                }
                self.expr(name, ctx, current_class, out);
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
                }
                return;
            }
            ExprKind::Instanceof(_, InstanceofTarget::Name(name, span)) => {
                if self.anchor.matches(*span)
                    && let Some(fqn) = resolve_class_ref(name, current_class, ctx, self.registry)
                {
                    out.push((*span, Resolved::Class(fqn)));
                }
            }
            // A plain variable read (`$this` excluded - it's never a
            // locally-declared variable, and is resolved separately via
            // `current_class` wherever it appears as a receiver). Needs
            // a real checker probe to resolve (see `Resolved::
            // VariableRead`'s own doc comment) - this only records enough
            // for `lsp.rs`'s own orchestration to run it, at either scope
            // `probe_scope` covers (a named function/method, or top-level
            // script scope).
            ExprKind::Variable(name) if **name != *"this" => {
                if self.anchor.matches(expr.span) {
                    out.push((
                        expr.span,
                        Resolved::VariableRead {
                            span: expr.span,
                            scope: self.probe_scope(),
                            is_static: self.current_is_static,
                            class: current_class.map(str::to_string),
                            name: name.clone(),
                            var_scope: self.resolve_var_scope(name),
                        },
                    ));
                }
            }
            // `$this->method()`/`$this->prop` resolve directly (only
            // `current_class` is needed, already in hand); any *other*
            // receiver needs Stage B's own type-inference probe - this
            // records enough (the receiver's own span, the member name,
            // and the enclosing scope to re-check) for `lsp.rs`'s own
            // orchestration to run that probe. The recorded span is the
            // member NAME's own sub-span (`spans[0]`), not `expr.span` -
            // the latter covers the whole `$x->method(...)` call
            // including its arguments, which would make every
            // `document_highlight` range for this arm too wide.
            ExprKind::MethodCall(recv, method, _, _)
            | ExprKind::PropertyAccess(recv, method, _) => {
                self.expr(recv, ctx, current_class, out);
                if self.anchor.matches(method.span) {
                    let spans = [method.span];
                    if matches!(&recv.kind, ExprKind::Variable(v) if **v == *"this")
                        && let Some(owner) = current_class
                    {
                        out.push((
                            spans[0],
                            Resolved::Member(owner.to_string(), method.to_string()),
                        ));
                    } else {
                        out.push((
                            spans[0],
                            Resolved::Instance {
                                receiver_span: recv.span,
                                member: method.to_string(),
                                scope: self.probe_scope(),
                                is_static: self.current_is_static,
                                class: current_class.map(str::to_string),
                            },
                        ));
                    }
                }
                for a in expr_children(expr) {
                    if !std::ptr::eq(a, recv.as_ref()) {
                        self.expr(a, ctx, current_class, out);
                    }
                }
                return;
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                self.class(decl, &mut ctx.clone(), out);
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
                }
                return;
            }
            ExprKind::Closure {
                params, uses, body, ..
            } => {
                for p in params {
                    self.param(p, ctx, current_class, out);
                }
                // A `use (&$x)`/`use ($x)` capture is itself a real,
                // navigable reference to the outer scope's `$x` -
                // resolved the same way an ordinary variable read is
                // (`probe_scope()` called *before* recursing into `body`,
                // since `current_function` is deliberately never updated
                // for a closure literal - see this file's own module doc -
                // so it still correctly points at the enclosing named
                // function/top-level scope, not the closure itself).
                // Each capture's own `var_scope` is resolved BEFORE this
                // closure's own frame is pushed below, since a `use`
                // clause names an OUTER variable, not one of the
                // closure's own locals.
                for u in uses {
                    let var_scope = self.resolve_var_scope(&u.name);
                    self.record(
                        out,
                        u.span,
                        Resolved::VariableRead {
                            span: u.span,
                            scope: self.probe_scope(),
                            is_static: self.current_is_static,
                            class: current_class.map(str::to_string),
                            name: u.name.clone(),
                            var_scope,
                        },
                    );
                }
                let id = self.next_id();
                self.scope_frames.push(ScopeFrame::Closure {
                    id,
                    transparent: uses.iter().map(|u| u.name.clone()).collect(),
                });
                self.stmts(body, &mut ctx.clone(), current_class, out);
                self.scope_frames.pop();
                return;
            }
            ExprKind::ArrowFn { params, body, .. } => {
                for p in params {
                    self.param(p, ctx, current_class, out);
                }
                let id = self.next_id();
                self.scope_frames.push(ScopeFrame::ArrowFn {
                    id,
                    owns: params.iter().map(|p| p.name.clone()).collect(),
                });
                self.expr(body, &mut ctx.clone(), current_class, out);
                self.scope_frames.pop();
                return;
            }
            _ => {}
        }
        for child in expr_children(expr) {
            self.expr(child, ctx, current_class, out);
        }
    }

    fn param(
        &mut self,
        p: &Param,
        ctx: &NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, Resolved)>,
    ) {
        if let Some(hint) = &p.type_hint {
            self.record_type_hint(out, hint, current_class, ctx);
        }
        self.record_var_self(out, p.name_span, current_class, &p.name);
    }

    fn function(
        &mut self,
        f: &FunctionDecl,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        is_static: bool,
        out: &mut Vec<(Span, Resolved)>,
    ) {
        // A named function/method (including one nested inside another
        // function's body - real, legal PHP) is its own fresh top-level
        // variable scope, independent of (and swapped separately from)
        // `current_function`/`current_is_static` below, which track the
        // real `Checker` probe scope, not this purely syntactic same-
        // variable grouping. Swapped BEFORE the params loop: a
        // parameter's own `var_scope` must resolve against THIS
        // function's fresh scope, never whatever enclosing `Closure`/
        // `ArrowFn` frame happened to be open at the declaration site.
        let new_base_scope_id = self.next_id();
        let previous_base_scope_id = std::mem::replace(&mut self.base_scope_id, new_base_scope_id);
        let previous_scope_frames = std::mem::take(&mut self.scope_frames);
        for p in &f.params {
            self.param(p, ctx, current_class, out);
        }
        if let Some(rt) = &f.return_type {
            self.record_type_hint(out, rt, current_class, ctx);
        }
        let previous_function = self.current_function.replace(f.clone());
        let previous_is_static = std::mem::replace(&mut self.current_is_static, is_static);
        self.stmts(&f.body, ctx, current_class, out);
        self.current_function = previous_function;
        self.current_is_static = previous_is_static;
        self.scope_frames = previous_scope_frames;
        self.base_scope_id = previous_base_scope_id;
    }

    fn class(&mut self, c: &ClassDecl, ctx: &mut NameContext, out: &mut Vec<(Span, Resolved)>) {
        let fqn = ctx.declared_name(&c.name);
        self.record(out, c.name_span, Resolved::Class(fqn.clone()));
        for (name, span) in c.extends.iter().zip(c.extends_spans.iter()) {
            if self.anchor.matches(*span)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                out.push((*span, Resolved::Class(target)));
            }
        }
        for (name, span) in c.implements.iter().zip(c.implements_spans.iter()) {
            if self.anchor.matches(*span)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                out.push((*span, Resolved::Class(target)));
            }
        }
        for (name, span) in c.uses.iter().zip(c.uses_spans.iter()) {
            if self.anchor.matches(*span)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                out.push((*span, Resolved::Class(target)));
            }
        }
        for p in &c.properties {
            self.record(
                out,
                p.name_span,
                Resolved::Member(fqn.clone(), p.name.to_string()),
            );
            if let Some(hint) = &p.type_hint {
                self.record_type_hint(out, hint, Some(&fqn), ctx);
            }
            if let Some(d) = &p.default {
                self.expr(d, &mut ctx.clone(), Some(&fqn), out);
            }
        }
        for m in &c.methods {
            self.record(
                out,
                m.function.name_span,
                Resolved::Member(fqn.clone(), m.function.name.to_string()),
            );
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, out);
        }
        for cst in &c.consts {
            self.record(
                out,
                cst.name_span,
                Resolved::Member(fqn.clone(), cst.name.to_string()),
            );
            if let Some(hint) = &cst.type_hint {
                self.record_type_hint(out, hint, Some(&fqn), ctx);
            }
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), out);
        }
    }

    fn enum_decl(&mut self, e: &EnumDecl, ctx: &mut NameContext, out: &mut Vec<(Span, Resolved)>) {
        let fqn = ctx.declared_name(&e.name);
        self.record(out, e.name_span, Resolved::Class(fqn.clone()));
        for (name, span) in e.implements.iter().zip(e.implements_spans.iter()) {
            if self.anchor.matches(*span)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                out.push((*span, Resolved::Class(target)));
            }
        }
        for (name, span) in e.uses.iter().zip(e.uses_spans.iter()) {
            if self.anchor.matches(*span)
                && let Some(target) = resolve_class_ref(name, Some(&fqn), ctx, self.registry)
            {
                out.push((*span, Resolved::Class(target)));
            }
        }
        for m in &e.methods {
            self.record(
                out,
                m.function.name_span,
                Resolved::Member(fqn.clone(), m.function.name.to_string()),
            );
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, out);
        }
        for cst in &e.consts {
            self.record(
                out,
                cst.name_span,
                Resolved::Member(fqn.clone(), cst.name.to_string()),
            );
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), out);
        }
        for case in &e.cases {
            self.record(
                out,
                case.name_span,
                Resolved::Member(fqn.clone(), case.name.to_string()),
            );
            if let Some(v) = &case.value {
                self.expr(v, &mut ctx.clone(), Some(&fqn), out);
            }
        }
    }
}
