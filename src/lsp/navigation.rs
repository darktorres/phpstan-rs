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
    /// A local variable read, resolved to its own declaration site (a
    /// parameter, an assignment's left-hand side, a `foreach` key/value
    /// binding, or a `catch (...) $e` binding) - always same-file, so no
    /// registry lookup is needed, unlike every other variant here. Found
    /// via a plain textual-order heuristic (the nearest preceding
    /// declaration to the same name in the walk, see `Walker::var_decls`'s
    /// own doc comment), not real control-flow analysis - deliberately
    /// disclosed as an approximation, never a wrong-file guess.
    Variable(Span),
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
    };
    w.stmts(stmts, &mut ctx, None, &mut best);
    best
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
    // `(name, span)` binding seen so far in the *current* function/
    // closure/top-level scope, in document order - a plain, deliberately
    // approximate "nearest preceding declaration wins" heuristic (see
    // `Resolved::Variable`'s own doc comment), not real control-flow
    // analysis. Reset to a fresh, empty `Vec` on entry to a named
    // function/method or a `Closure` (PHP's own two real variable-scope
    // boundaries - a `Closure` only sees its own params plus whatever its
    // `use (...)` clause explicitly names, copied in from the *outer*
    // scope's own `var_decls` at that point). An `ArrowFn` does *not* get
    // a fresh scope (it implicitly captures the whole enclosing scope by
    // value, the real PHP semantics `checker/expr.rs`'s own `ArrowFn` arm
    // already relies on) - its own params are pushed and popped around
    // just its one body expression instead.
    var_decls: Vec<(String, Span)>,
}

impl<'a> Walker<'a> {
    /// Records `span` as `name`'s own declaration site, and - if the
    /// cursor lands directly on the declaration itself (clicking an
    /// assignment's own left-hand side, a parameter, ...) - resolves it
    /// to itself right here, same as every other declaration-site `record`
    /// call in this module does.
    fn declare_var(&mut self, name: &str, span: Span, best: &mut Option<Resolved>) {
        if contains(span, self.pos) {
            *best = Some(Resolved::Variable(span));
        }
        self.var_decls.push((name.to_string(), span));
    }

    /// The nearest-preceding declaration for `name` in the current scope,
    /// by walking `var_decls` in reverse - the last (innermost/most
    /// recent) entry wins, matching plain sequential document order.
    fn lookup_var(&self, name: &str) -> Option<Span> {
        self.var_decls
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, s)| *s)
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
                // `foreach`'s own key/value are *bindings*, not reads -
                // recorded as declaration sites (`declare_var`), not
                // resolved as if they were references to some earlier
                // declaration. A non-`Variable` shape (list destructuring,
                // `foreach ($rows as [$a, $b])`) falls back to an ordinary
                // read-walk instead - its own inner names aren't tracked
                // as declarations, a disclosed, narrower residual gap.
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
                    // The catch's own bound variable (`catch (Foo $e)`) is
                    // a declaration site too - recorded here, before
                    // `stmt_children`'s own generic recursion walks the
                    // catch body below, so a read inside that body already
                    // sees it.
                    if let (Some(var), Some(span)) = (&c.var, c.var_span) {
                        self.declare_var(var, span, best);
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
                if contains(expr.span, self.pos)
                    && let Some(decl_span) = self.lookup_var(name)
                {
                    *best = Some(Resolved::Variable(decl_span));
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
                for u in uses {
                    if let Some(span) = outer_var_decls
                        .iter()
                        .rev()
                        .find(|(n, _)| n == &u.name)
                        .map(|(_, s)| *s)
                    {
                        self.var_decls.push((u.name.clone(), span));
                    }
                }
                for p in params {
                    self.param(p, ctx, current_class, best);
                }
                self.stmts(body, &mut ctx.clone(), current_class, best);
                self.var_decls = outer_var_decls;
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

    /// A `foreach` key/value's own binding - a declaration site, not a
    /// read. Falls back to an ordinary read-walk for anything that isn't a
    /// plain `$var` (list-destructuring, `foreach ($rows as [$a, $b])`),
    /// whose own inner names then aren't tracked as declarations - a
    /// disclosed, narrower residual gap than "not supported at all".
    fn bind_var(
        &mut self,
        e: &Expr,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<Resolved>,
    ) {
        if let ExprKind::Variable(name) = &e.kind {
            self.declare_var(name, e.span, best);
        } else {
            self.expr(e, ctx, current_class, best);
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
        // own doc comment) - fresh scope, seeded below by its own params.
        let previous_var_decls = std::mem::take(&mut self.var_decls);
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
