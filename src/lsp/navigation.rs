//! "What symbol is at this (file, position)" - the shared first step
//! both hover and go-to-definition need. Walks a
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
//! (`$x->method()`, `$x->prop`) or a local variable *read*'s own
//! declaration site - both need real type-inference/dataflow machinery
//! this module has none of. `lsp.rs`'s own orchestration runs a real,
//! throwaway `Checker` probe for both (`Resolved::Instance`/`Resolved::
//! VariableRead`'s own doc comments), reusing the checker's already-
//! correct flow-sensitive analysis (branch merging, loop fixed-point
//! convergence, termination-pruning) rather than a second, hand-rolled
//! approximation of it - see `Checker::probe_var_provenance`'s own doc
//! comment in `checker.rs` for the full design.

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
    /// fields below.
    VariableSelf {
        span: Span,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
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

/// The entry point: what's declared/referenced at `pos` in the file
/// loaded under `path`, or `None` for nothing resolvable there.
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
        top_level_stmts: stmts,
        current_function: None,
        current_is_static: false,
    };
    w.stmts(stmts, &mut ctx, None, &mut best);
    best
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
        "?" | "{dynamic}" => None,
        "self" | "static" => current_class.map(str::to_string),
        "parent" => registry
            .classes
            .get(current_class?)
            .and_then(|info| info.extends.first())
            .map(|s| s.to_string()),
        _ => Some(ctx.resolve(name)),
    }
}

struct Walker<'a> {
    registry: &'a Registry,
    source: &'a str,
    pos: Position,
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
}

impl<'a> Walker<'a> {
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

    fn record(&self, best: &mut Option<Resolved>, span: Span, value: Resolved) {
        if contains(span, self.pos) {
            *best = Some(value);
        }
    }

    /// Records `span` as a local variable's own declaration site, self-
    /// resolving (no probe needed - the click's own position already is
    /// the answer) for go-to-definition when the cursor lands directly on
    /// it - `scope`/`is_static`/`class` are only for hover's own later,
    /// separate probe (see `Resolved::VariableSelf`'s own doc comment).
    fn record_var_self(
        &self,
        best: &mut Option<Resolved>,
        span: Span,
        current_class: Option<&str>,
    ) {
        self.record(
            best,
            span,
            Resolved::VariableSelf {
                span,
                scope: self.probe_scope(),
                is_static: self.current_is_static,
                class: current_class.map(str::to_string),
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
        best: &mut Option<Resolved>,
    ) {
        match &e.kind {
            ExprKind::Variable(_) => self.record_var_self(best, e.span, current_class),
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
            StmtKind::If { cond, .. }
            | StmtKind::While { cond, .. }
            | StmtKind::DoWhile { cond, .. } => {
                self.expr(cond, ctx, current_class, best);
            }
            StmtKind::For {
                init, cond, step, ..
            } => {
                for e in init.iter().chain(cond).chain(step) {
                    self.expr(e, ctx, current_class, best);
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                ..
            } => {
                self.expr(subject, ctx, current_class, best);
                if let Some(k) = key {
                    self.bind_var(k, ctx, current_class, best);
                }
                self.bind_var(value, ctx, current_class, best);
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
            // `global $x;`/`static $x = ...;` have no per-name span of
            // their own (only the whole statement's) - the same
            // imprecision `Checker::declare`'s own instrumentation for
            // these two shapes accepts (`checker/stmt.rs`).
            StmtKind::Global(names) => {
                for _ in names {
                    self.record_var_self(best, stmt.span, current_class);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (_, d) in vars {
                    if let Some(e) = d {
                        self.expr(e, ctx, current_class, best);
                    }
                    self.record_var_self(best, stmt.span, current_class);
                }
            }
            StmtKind::Switch { subject, cases } => {
                self.expr(subject, ctx, current_class, best);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.expr(t, ctx, current_class, best);
                    }
                }
            }
            StmtKind::Try { catches, .. } => {
                for c in catches {
                    for (name, span) in c.types.iter().zip(c.types_spans.iter()) {
                        if contains(*span, self.pos)
                            && let Some(fqn) =
                                resolve_class_ref(name, current_class, ctx, self.registry)
                        {
                            *best = Some(Resolved::Class(fqn));
                        }
                    }
                    if let (Some(_), Some(span)) = (&c.var, c.var_span) {
                        self.record_var_self(best, span, current_class);
                    }
                }
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
            // A plain variable read (`$this` excluded - it's never a
            // locally-declared variable, and is resolved separately via
            // `current_class` wherever it appears as a receiver). Needs
            // a real checker probe to resolve (see `Resolved::
            // VariableRead`'s own doc comment) - this only records enough
            // for `lsp.rs`'s own orchestration to run it, at either scope
            // `probe_scope` covers (a named function/method, or top-level
            // script scope).
            ExprKind::Variable(name) if **name != *"this" => {
                if contains(expr.span, self.pos) {
                    *best = Some(Resolved::VariableRead {
                        span: expr.span,
                        scope: self.probe_scope(),
                        is_static: self.current_is_static,
                        class: current_class.map(str::to_string),
                    });
                }
            }
            // `$this->method()`/`$this->prop` resolve directly (only
            // `current_class` is needed, already in hand); any *other*
            // receiver needs Stage B's own type-inference probe - this
            // records enough (the receiver's own span, the member name,
            // and the enclosing scope to re-check) for `lsp.rs`'s own
            // orchestration to run that probe.
            ExprKind::MethodCall(recv, method, _, _)
            | ExprKind::PropertyAccess(recv, method, _) => {
                self.expr(recv, ctx, current_class, best);
                if contains(expr.span, self.pos)
                    && let Some(spans) =
                        split_identifier_spans(self.source, expr.span, &[method.as_ref()])
                    && contains(spans[0], self.pos)
                {
                    if matches!(&recv.kind, ExprKind::Variable(v) if **v == *"this")
                        && let Some(owner) = current_class
                    {
                        *best = Some(Resolved::Member(owner.to_string(), method.to_string()));
                    } else {
                        *best = Some(Resolved::Instance {
                            receiver_span: recv.span,
                            member: method.to_string(),
                            scope: self.probe_scope(),
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
                for p in params {
                    self.param(p, ctx, current_class, best);
                }
                // A `use (&$x)`/`use ($x)` capture is itself a real,
                // navigable reference to the outer scope's `$x` -
                // resolved the same way an ordinary variable read is
                // (`probe_scope()` called *before* recursing into `body`,
                // since `current_function` is deliberately never updated
                // for a closure literal - see this file's own module doc -
                // so it still correctly points at the enclosing named
                // function/top-level scope, not the closure itself).
                for u in uses {
                    self.record(
                        best,
                        u.span,
                        Resolved::VariableRead {
                            span: u.span,
                            scope: self.probe_scope(),
                            is_static: self.current_is_static,
                            class: current_class.map(str::to_string),
                        },
                    );
                }
                self.stmts(body, &mut ctx.clone(), current_class, best);
                return;
            }
            ExprKind::ArrowFn { params, body, .. } => {
                for p in params {
                    self.param(p, ctx, current_class, best);
                }
                self.expr(body, &mut ctx.clone(), current_class, best);
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
        self.record_var_self(best, p.name_span, current_class);
    }

    fn function(
        &mut self,
        f: &FunctionDecl,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        is_static: bool,
        best: &mut Option<Resolved>,
    ) {
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
                Resolved::Member(fqn.clone(), p.name.to_string()),
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
                Resolved::Member(fqn.clone(), m.function.name.to_string()),
            );
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, best);
        }
        for cst in &c.consts {
            self.record(
                best,
                cst.name_span,
                Resolved::Member(fqn.clone(), cst.name.to_string()),
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
                Resolved::Member(fqn.clone(), m.function.name.to_string()),
            );
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, best);
        }
        for cst in &e.consts {
            self.record(
                best,
                cst.name_span,
                Resolved::Member(fqn.clone(), cst.name.to_string()),
            );
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), best);
        }
        for case in &e.cases {
            self.record(
                best,
                case.name_span,
                Resolved::Member(fqn.clone(), case.name.to_string()),
            );
            if let Some(v) = &case.value {
                self.expr(v, &mut ctx.clone(), Some(&fqn), best);
            }
        }
    }
}
