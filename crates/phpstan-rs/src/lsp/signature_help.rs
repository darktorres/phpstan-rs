//! `textDocument/signatureHelp` - an active-parameter tooltip while
//! typing inside a call's parens. Structured after (not sharing code
//! with) `navigation::Walker`: the same exhaustive per-`StmtKind` field
//! extraction is unavoidable here too (a call can appear inside any
//! statement's own condition/subject/return-value/... - `stmt_children`
//! alone only reaches nested *statements*, never a statement's own
//! `Expr` fields - see `ast_walk`'s own doc comment), but the payload at
//! each call-shaped `Expr` is different: not "what does this resolve
//! to", but "is the cursor inside this call's own argument list, and if
//! so, which argument".
//!
//! **Claim-and-clear, not overwrite-only-on-match.** Every call-shaped
//! node (`Call`, `New`, `MethodCall`, `DynamicMethodCall`, `StaticCall`,
//! `NewAnonymousClass`) runs a three-step protocol every time it's
//! visited: (1) if the cursor is anywhere inside this node's own overall
//! span, unconditionally clear `best` to `None` first; (2) only if the
//! cursor is also past this call's own args-region boundary (see below)
//! AND the callee/member actually resolves, set `best` to `Some`; (3)
//! recurse into children, so a still-more-specific nested match
//! overrides step 1's clear. Skipping step 1 would leak an enclosing
//! resolvable call's signature into an unresolvable or out-of-scope
//! nested construct sitting inside it (a dynamically-called callee, or
//! an anonymous class's own constructor arg gap) - the cursor is
//! genuinely inside *that* inner, non-matching construct, not the outer
//! call, so the right answer is `None`, not a stale outer match.
//!
//! **The args-region gate.** A call's own overall span starts at the
//! callee's first token, so `contains(call.span, pos)` alone is true
//! anywhere from `fo|o(...)` onward, including while the cursor still
//! sits on the function/method/class name itself, before the opening
//! paren - too early to offer a *parameter* tooltip for. Step 2 above
//! additionally requires `pos` to be at or past an args-region boundary,
//! computed per kind from a span the AST gives: `Call`/`New` ->
//! `callee.span.end`; `MethodCall` -> the end of the method name's
//! span; `DynamicMethodCall` ->
//! `name_expr.span.end` (a few characters before the real `(` for the
//! `{$expr}` brace form - narrow, disclosed slack, never a wrong-call
//! leak); `StaticCall` -> the end of the method name's span.

use std::path::Path;

use crate::ast::{
    Arg, ClassDecl, EnumDecl, Expr, ExprKind, FunctionDecl, Param, StaticTarget, Stmt, StmtKind,
};
use crate::ast_walk::{expr_children, stmt_children};
use crate::project::Project;
use crate::registry::Registry;
use crate::token::{Position, Span};

use super::navigation::{ProbeScope, contains, resolve_class_ref};
use crate::name_resolution::NameContext;

/// What a resolved signature-help target needs to look its own
/// `FunctionSig` up (or, for `Instance`, to run the same checker probe
/// `navigation::Resolved::Instance` already uses for hover).
pub(crate) enum Target {
    Function(String),
    /// `new Foo(...)` - resolves via `registry.lookup_method_with_owner
    /// (class, "__construct")`, not a plain function lookup.
    Constructor(String),
    /// `$this->method()`/`self::method()`/`Foo::method()` (a literal,
    /// statically-known owner) - resolves directly, no probe needed.
    Member(String, String),
    /// Any other `->`/`::` receiver - needs the same checker probe
    /// `navigation::Resolved::Instance` already runs for hover; may
    /// resolve to more than one real owner for a `Union`/`Intersection`
    /// receiver, each offered as its own `SignatureInformation`.
    Instance {
        receiver_span: Span,
        member: String,
        scope: ProbeScope,
        is_static: bool,
        class: Option<String>,
    },
}

pub(crate) struct SigHelp {
    pub(crate) target: Target,
    pub(crate) active_parameter: usize,
}

pub(crate) fn resolve_at(project: &Project, path: &Path, pos: Position) -> Option<SigHelp> {
    let stmts = project.ast(path)?;
    let registry = project.registry();
    let mut ctx = NameContext::default();
    let mut best = None;
    let mut w = Walker {
        registry,
        pos,
        top_level_stmts: stmts,
        current_function: None,
        current_is_static: false,
    };
    w.stmts(stmts, &mut ctx, None, &mut best);
    best
}

/// The number of arguments whose own span ends strictly before `pos`,
/// clamped to the last real parameter index for a non-variadic
/// signature (typing past the last declared parameter still highlights
/// it, matching every other LSP server's convention, rather than
/// reporting an out-of-range index) - `lsp.rs`'s own response-building
/// step is what actually knows `param_count` and does the clamping
/// (this function only counts).
fn active_parameter(args: &[Arg], pos: Position) -> usize {
    args.iter()
        .filter(|a| {
            let end = a.value.span.end;
            (end.line, end.col) < (pos.line, pos.col)
        })
        .count()
}

struct Walker<'a> {
    registry: &'a Registry,
    pos: Position,
    top_level_stmts: &'a [Stmt],
    current_function: Option<FunctionDecl>,
    current_is_static: bool,
}

impl<'a> Walker<'a> {
    fn probe_scope(&self) -> ProbeScope {
        match &self.current_function {
            Some(f) => ProbeScope::Function(Box::new(f.clone())),
            None => ProbeScope::TopLevel(self.top_level_stmts.to_vec()),
        }
    }

    fn stmts(
        &mut self,
        stmts: &[Stmt],
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<SigHelp>,
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
        best: &mut Option<SigHelp>,
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
                    self.expr(k, ctx, current_class, best);
                }
                self.expr(value, ctx, current_class, best);
            }
            StmtKind::FunctionDecl(f) => {
                self.function(f, &mut ctx.clone(), current_class, false, best);
            }
            StmtKind::ClassDecl(c) => self.class(c, &mut ctx.clone(), best),
            StmtKind::EnumDecl(e) => self.enum_decl(e, &mut ctx.clone(), best),
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.expr(&c.value, ctx, current_class, best);
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
            StmtKind::StaticVar(vars) => {
                for (_, d) in vars {
                    if let Some(e) = d {
                        self.expr(e, ctx, current_class, best);
                    }
                }
            }
            _ => {}
        }
        for child in stmt_children(stmt) {
            self.stmt(child, ctx, current_class, best);
        }
    }

    fn param(
        &mut self,
        p: &Param,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<SigHelp>,
    ) {
        if let Some(d) = &p.default {
            self.expr(d, ctx, current_class, best);
        }
    }

    fn function(
        &mut self,
        f: &FunctionDecl,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        is_static: bool,
        best: &mut Option<SigHelp>,
    ) {
        for p in &f.params {
            self.param(p, ctx, current_class, best);
        }
        let previous_function = self.current_function.replace(f.clone());
        let previous_is_static = std::mem::replace(&mut self.current_is_static, is_static);
        self.stmts(&f.body, ctx, current_class, best);
        self.current_function = previous_function;
        self.current_is_static = previous_is_static;
    }

    /// Mirrors `navigation::Walker::class`'s own precedent exactly:
    /// takes no `current_class` of its own (everything inside always
    /// resolves against this class's own freshly-computed `fqn`) -
    /// `NewAnonymousClass`'s own expr-arm is what decides which
    /// `current_class` its *constructor arguments* (as opposed to this
    /// class's own body) see, and it deliberately does NOT pass one in
    /// here.
    fn class(&mut self, c: &ClassDecl, ctx: &mut NameContext, best: &mut Option<SigHelp>) {
        let fqn = ctx.declared_name(&c.name);
        for p in &c.properties {
            if let Some(d) = &p.default {
                self.expr(d, &mut ctx.clone(), Some(&fqn), best);
            }
        }
        for m in &c.methods {
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, best);
        }
        for cst in &c.consts {
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), best);
        }
    }

    fn enum_decl(&mut self, e: &EnumDecl, ctx: &mut NameContext, best: &mut Option<SigHelp>) {
        let fqn = ctx.declared_name(&e.name);
        for m in &e.methods {
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), m.is_static, best);
        }
        for cst in &e.consts {
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), best);
        }
        for case in &e.cases {
            if let Some(v) = &case.value {
                self.expr(v, &mut ctx.clone(), Some(&fqn), best);
            }
        }
    }

    fn expr(
        &mut self,
        expr: &Expr,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        best: &mut Option<SigHelp>,
    ) {
        match &expr.kind {
            ExprKind::Call(callee, args) => {
                if contains(expr.span, self.pos) {
                    *best = None;
                    if self.pos_past(callee.span.end)
                        && let ExprKind::Name(n) = &callee.kind
                        && let Some(fqn) = resolve_class_ref(n, current_class, ctx, self.registry)
                        && self.registry.lookup_function(&fqn).is_some()
                    {
                        *best = Some(SigHelp {
                            target: Target::Function(fqn),
                            active_parameter: active_parameter(args, self.pos),
                        });
                    }
                }
                self.expr(callee, ctx, current_class, best);
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::New(callee, args) => {
                if contains(expr.span, self.pos) {
                    *best = None;
                    if self.pos_past(callee.span.end)
                        && let ExprKind::Name(n) = &callee.kind
                        && let Some(fqn) = resolve_class_ref(n, current_class, ctx, self.registry)
                        && self
                            .registry
                            .lookup_method_with_owner(&fqn, "__construct")
                            .is_some()
                    {
                        *best = Some(SigHelp {
                            target: Target::Constructor(fqn),
                            active_parameter: active_parameter(args, self.pos),
                        });
                    }
                }
                self.expr(callee, ctx, current_class, best);
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::MethodCall(recv, method, args, _) => {
                if contains(expr.span, self.pos) {
                    *best = None;
                    if self.pos_past(method.span.end) {
                        if matches!(&recv.kind, ExprKind::Variable(v) if **v == *"this")
                            && let Some(owner) = current_class
                        {
                            *best = Some(SigHelp {
                                target: Target::Member(owner.to_string(), method.to_string()),
                                active_parameter: active_parameter(args, self.pos),
                            });
                        } else {
                            *best = Some(SigHelp {
                                target: Target::Instance {
                                    receiver_span: recv.span,
                                    member: method.to_string(),
                                    scope: self.probe_scope(),
                                    is_static: self.current_is_static,
                                    class: current_class.map(str::to_string),
                                },
                                active_parameter: active_parameter(args, self.pos),
                            });
                        }
                    }
                }
                self.expr(recv, ctx, current_class, best);
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::DynamicStaticCall(target, name_expr, args) => {
                // Unresolvable statically - clear only, never set.
                if contains(expr.span, self.pos) {
                    *best = None;
                }
                if let StaticTarget::Expr(receiver) = target {
                    self.expr(receiver, ctx, current_class, best);
                }
                self.expr(name_expr, ctx, current_class, best);
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::DynamicMethodCall(recv, name_expr, args) => {
                // Unresolvable statically - clear only (Fix 1), never
                // set. `name_expr.span.end` isn't consulted for a gate
                // since there's nothing to gate toward setting.
                if contains(expr.span, self.pos) {
                    *best = None;
                }
                self.expr(recv, ctx, current_class, best);
                self.expr(name_expr, ctx, current_class, best);
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::StaticCall(target, member, args) => {
                if contains(expr.span, self.pos) {
                    *best = None;
                    // A dynamic receiver has no class-name text to resolve
                    // here (this pass doesn't run a type probe the way
                    // `navigation.rs`'s own equivalent does).
                    if let Some(class) = target.as_name()
                        && let Some(fqn) =
                            resolve_class_ref(class, current_class, ctx, self.registry)
                        && self.pos_past(member.span.end)
                    {
                        *best = Some(SigHelp {
                            target: Target::Member(fqn, member.to_string()),
                            active_parameter: active_parameter(args, self.pos),
                        });
                    }
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                // Out of scope for v1 (no signature to offer for the
                // anonymous class's own constructor) - clear only, so a
                // cursor in its arg-gap/trailing-comma region doesn't
                // leak an enclosing resolvable call's signature.
                if contains(expr.span, self.pos) {
                    *best = None;
                }
                // The class's own body (methods/consts/property
                // defaults) resolves everything against its own fresh
                // synthetic FQN - `class` takes no `current_class` of
                // its own, matching `navigation::Walker::class`'s
                // identical precedent.
                self.class(decl, &mut ctx.clone(), best);
                // The constructor ARGUMENTS execute in the *enclosing*
                // scope, not the anonymous class's own - passed through
                // unmodified, the mirror image of the line above.
                for a in args {
                    self.expr(&a.value, ctx, current_class, best);
                }
                return;
            }
            ExprKind::FirstClassCallable(_) => return,
            ExprKind::Closure {
                params,
                uses: _,
                body,
                ..
            } => {
                for p in params {
                    self.param(p, ctx, current_class, best);
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

    /// The args-region gate (see this module's own doc comment) -
    /// `boundary` is a callee/member sub-span's own end position; the
    /// cursor must be at or past it, not merely somewhere inside the
    /// call's whole span, before a signature is actually offered.
    fn pos_past(&self, boundary: Position) -> bool {
        (boundary.line, boundary.col) <= (self.pos.line, self.pos.col)
    }
}

#[cfg(test)]
mod tests;
