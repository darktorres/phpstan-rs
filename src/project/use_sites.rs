//! `textDocument/references`/`textDocument/rename`'s own whole-project
//! use-site index - two tiers, kept as two separate mechanisms because
//! they need fundamentally different machinery:
//!
//! Tier 1 (this module): class/function reference sites - a purely
//! syntactic, name-resolution-only walk (`ClassFnRefWalker`), structured
//! after (not sharing code with) `lsp::navigation::Walker` - the same
//! proven traversal shape, reused for "record every match", never "find
//! the one match at a cursor". Cheap enough to run eagerly on every
//! `insert_parsed_file`/`update_file`, gated only by `retain_source`
//! (the same gate `DeclLocations` already uses, for the identical
//! "only the LSP needs this" reasoning).
//!
//! Tier 2 (member reference sites - method/property/class-constant
//! access) needs a real, checker-driven resolution of the receiver's
//! declaring owner - built separately, in `checker.rs`/`checker/expr.rs`,
//! and merged into this same index by `Project::ensure_member_index_built`/
//! `Project::reindex_tier2_for_file`.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::registry::NameContext;
use crate::text_split::{split_identifier_spans, type_hint_identifiers};
use crate::token::Span;

/// What a reference site points at - no local-variable variant (see
/// this item's own edge-case notes: local-variable rename/references is
/// explicitly out of v1 scope, always answered via a `None` short-circuit
/// before this index is ever consulted).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RefTarget {
    Class(String),
    Function(String),
    /// `(declaring owner FQN, member name)` - always the true,
    /// ancestry-resolved declaring class, never the raw syntactic
    /// receiver/class name a `self::`/inherited-call site was written
    /// against.
    Member(String, String),
}

/// Every reference site this project has ever indexed, in both
/// directions: `by_target` answers "where is X used" (`references`'s own
/// query shape); `by_file` answers "what did this file contribute"
/// (`remove_file`'s own undo shape, mirroring `DeclLocations`'s identical
/// two-directions-for-two-operations split).
#[derive(Default)]
pub(crate) struct UseSiteIndex {
    by_target: FxHashMap<RefTarget, Vec<(PathBuf, Span)>>,
    by_file: FxHashMap<PathBuf, Vec<(RefTarget, Span)>>,
}

impl UseSiteIndex {
    pub(crate) fn insert(&mut self, path: &Path, target: RefTarget, span: Span) {
        self.by_target
            .entry(target.clone())
            .or_default()
            .push((path.to_path_buf(), span));
        self.by_file
            .entry(path.to_path_buf())
            .or_default()
            .push((target, span));
    }

    /// Drops every site `path` previously contributed, from both
    /// directions - called unconditionally on every re-index/removal of
    /// `path` (a no-op before anything was ever inserted for it, in
    /// particular before Tier 2's own index is first built at all).
    pub(crate) fn remove_file(&mut self, path: &Path) {
        let Some(entries) = self.by_file.remove(path) else {
            return;
        };
        for (target, span) in entries {
            if let Some(sites) = self.by_target.get_mut(&target) {
                sites.retain(|(p, s)| !(p == path && *s == span));
                if sites.is_empty() {
                    self.by_target.remove(&target);
                }
            }
        }
    }

    pub(crate) fn references(&self, target: &RefTarget) -> &[(PathBuf, Span)] {
        self.by_target.get(target).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Resolves a bare class-name-shaped reference - structured after (not
/// sharing code with) `lsp::navigation::resolve_class_ref`, for the same
/// "independent reimplementation" reason `navigation.rs`'s own module
/// doc comment gives for its 4 siblings. Deliberately DIVERGES from it on
/// one point: `self`/`static`/`parent` resolve to `None` here, never a
/// real FQN. `navigation.rs`'s own version resolves them (correct for
/// read-only hover/go-to-definition, where jumping from a `self::`
/// reference to the current class's own declaration is exactly right) -
/// but this index feeds BOTH `references` and `rename`, and `rename`
/// literally rewrites a site's own source span to the new name. A
/// `self::CONST` site's span covers the literal text "self", not the
/// class's own name - renaming `Foo` to `Bar` must never turn `self::` in
/// `Foo`'s own body into `Bar::`, since `self` already means "whatever
/// this class is called," automatically, forever. Recording it as a real
/// `Foo` reference site would corrupt exactly that rename.
fn resolve_class_ref(
    name: &str,
    _current_class: Option<&str>,
    ctx: &NameContext,
) -> Option<String> {
    match name {
        "?" | "{dynamic}" | "self" | "static" | "parent" => None,
        _ => Some(ctx.resolve(name)),
    }
}

/// Every `(Span, RefTarget::Class|Function)` pair in `stmts` - Tier 1's
/// own entry point. `parent`/member halves of a `StaticAccess`/
/// `StaticCall` are deliberately NOT resolved here (parent needs the
/// registry's own ancestor lookup only `resolve_class_ref` in
/// `navigation.rs` does via `registry.classes`, harmless to add later but
/// unneeded for Tier 1's own class/function scope; the member half is
/// Tier 2's job entirely, via a real Checker-resolved declaring owner).
pub(crate) fn index_class_fn_refs(stmts: &[Stmt], source: &str) -> Vec<(Span, RefTarget)> {
    let mut ctx = NameContext::default();
    let mut out = Vec::new();
    let w = ClassFnRefWalker { source };
    w.stmts(stmts, &mut ctx, None, &mut out);
    out
}

struct ClassFnRefWalker<'a> {
    source: &'a str,
}

impl<'a> ClassFnRefWalker<'a> {
    fn record_type_hint(
        &self,
        out: &mut Vec<(Span, RefTarget)>,
        hint: &str,
        span: Span,
        current_class: Option<&str>,
        ctx: &NameContext,
    ) {
        let names = type_hint_identifiers(hint);
        let expected: Vec<&str> = names.iter().map(String::as_str).collect();
        let Some(spans) = split_identifier_spans(self.source, span, &expected) else {
            return;
        };
        for (name, name_span) in names.iter().zip(spans.iter()) {
            if let Some(fqn) = resolve_class_ref(name, current_class, ctx) {
                out.push((*name_span, RefTarget::Class(fqn)));
            }
        }
    }

    /// A parameter's own default-value expression (including a promoted
    /// constructor parameter's own default) - not visited by
    /// `lsp::navigation::Walker::param` at all (that walker only ever
    /// needs a parameter's type-hint/name-declaration site for
    /// cursor-driven hover/definition), but a legitimate home for a bare
    /// `Name`/`StaticAccess(::class)` reference this item's own index
    /// must not silently miss.
    fn param(
        &self,
        p: &Param,
        ctx: &NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, RefTarget)>,
    ) {
        if let (Some(hint), Some(span)) = (&p.type_hint, p.type_hint_span) {
            self.record_type_hint(out, hint, span, current_class, ctx);
        }
        if let Some(d) = &p.default {
            self.expr(d, &mut ctx.clone(), current_class, out);
        }
    }

    fn stmts(
        &self,
        stmts: &[Stmt],
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, RefTarget)>,
    ) {
        for stmt in stmts {
            self.stmt(stmt, ctx, current_class, out);
        }
    }

    fn stmt(
        &self,
        stmt: &Stmt,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, RefTarget)>,
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
            StmtKind::Foreach { subject, .. } => {
                self.expr(subject, ctx, current_class, out);
            }
            StmtKind::FunctionDecl(f) => {
                self.function(f, &mut ctx.clone(), current_class, out);
            }
            StmtKind::ClassDecl(c) => self.class(c, ctx, out),
            StmtKind::EnumDecl(e) => self.enum_decl(e, ctx, out),
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.expr(&c.value, ctx, current_class, out);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (_, d) in vars {
                    if let Some(e) = d {
                        self.expr(e, ctx, current_class, out);
                    }
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
                        if let Some(fqn) = resolve_class_ref(name, current_class, ctx) {
                            out.push((*span, RefTarget::Class(fqn)));
                        }
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
        &self,
        expr: &Expr,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, RefTarget)>,
    ) {
        match &expr.kind {
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind {
                    if let Some(fqn) = resolve_class_ref(n, current_class, ctx) {
                        out.push((callee.span, RefTarget::Function(fqn)));
                    }
                } else {
                    self.expr(callee, ctx, current_class, out);
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
                }
                return;
            }
            ExprKind::New(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind {
                    if let Some(fqn) = resolve_class_ref(n, current_class, ctx) {
                        out.push((callee.span, RefTarget::Class(fqn)));
                    }
                } else {
                    self.expr(callee, ctx, current_class, out);
                }
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
                }
                return;
            }
            ExprKind::Name(n) => {
                if let Some(fqn) = resolve_class_ref(n, current_class, ctx) {
                    // Same function-preferred disambiguation as
                    // `navigation.rs`'s identical arm - a bare `Name` node
                    // reached any other way than a `Call`/`New` callee
                    // (e.g. a first-class-callable target `foo(...)`).
                    out.push((expr.span, RefTarget::Function(fqn)));
                }
            }
            ExprKind::StaticAccess(class, member) => {
                if let Some(fqn) = resolve_class_ref(class, current_class, ctx)
                    && let Some(spans) = split_identifier_spans(
                        self.source,
                        expr.span,
                        &[class.as_str(), member.as_str()],
                    )
                {
                    out.push((spans[0], RefTarget::Class(fqn)));
                }
            }
            // A genuinely dynamic receiver has no class-name text to
            // record a reference for here at all - any name reference
            // buried inside it is still picked up by the generic
            // `expr_children` walk below (it's a real child of this
            // node now, not a discarded `"?"` sentinel).
            ExprKind::StaticCall(StaticTarget::Name(class, _), member, _) => {
                if let Some(fqn) = resolve_class_ref(class, current_class, ctx)
                    && let Some(spans) = split_identifier_spans(
                        self.source,
                        expr.span,
                        &[class.as_str(), member.as_str()],
                    )
                {
                    out.push((spans[0], RefTarget::Class(fqn)));
                }
            }
            ExprKind::Instanceof(_, InstanceofTarget::Name(name, span)) => {
                if let Some(fqn) = resolve_class_ref(name, current_class, ctx) {
                    out.push((*span, RefTarget::Class(fqn)));
                }
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                self.class(decl, &mut ctx.clone(), out);
                for a in args {
                    self.expr(&a.value, ctx, current_class, out);
                }
                return;
            }
            ExprKind::Closure { params, body, .. } => {
                for p in params {
                    self.param(p, ctx, current_class, out);
                }
                self.stmts(body, &mut ctx.clone(), current_class, out);
                return;
            }
            ExprKind::ArrowFn { params, body, .. } => {
                for p in params {
                    self.param(p, ctx, current_class, out);
                }
                self.expr(body, &mut ctx.clone(), current_class, out);
                return;
            }
            _ => {}
        }
        for child in expr_children(expr) {
            self.expr(child, ctx, current_class, out);
        }
    }

    fn function(
        &self,
        f: &FunctionDecl,
        ctx: &mut NameContext,
        current_class: Option<&str>,
        out: &mut Vec<(Span, RefTarget)>,
    ) {
        for p in &f.params {
            self.param(p, ctx, current_class, out);
        }
        if let (Some(rt), Some(span)) = (&f.return_type, f.return_type_span) {
            self.record_type_hint(out, rt, span, current_class, ctx);
        }
        self.stmts(&f.body, ctx, current_class, out);
    }

    fn class(&self, c: &ClassDecl, ctx: &mut NameContext, out: &mut Vec<(Span, RefTarget)>) {
        let fqn = ctx.declared_name(&c.name);
        for (name, span) in c.extends.iter().zip(c.extends_spans.iter()) {
            if let Some(target) = resolve_class_ref(name, Some(&fqn), ctx) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for (name, span) in c.implements.iter().zip(c.implements_spans.iter()) {
            if let Some(target) = resolve_class_ref(name, Some(&fqn), ctx) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for (name, span) in c.uses.iter().zip(c.uses_spans.iter()) {
            if let Some(target) = resolve_class_ref(name, Some(&fqn), ctx) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for p in &c.properties {
            if let (Some(hint), Some(span)) = (&p.type_hint, p.type_hint_span) {
                self.record_type_hint(out, hint, span, Some(&fqn), ctx);
            }
            if let Some(d) = &p.default {
                self.expr(d, &mut ctx.clone(), Some(&fqn), out);
            }
        }
        for m in &c.methods {
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), out);
        }
        for cst in &c.consts {
            if let (Some(hint), Some(span)) = (&cst.type_hint, cst.type_hint_span) {
                self.record_type_hint(out, hint, span, Some(&fqn), ctx);
            }
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), out);
        }
    }

    fn enum_decl(&self, e: &EnumDecl, ctx: &mut NameContext, out: &mut Vec<(Span, RefTarget)>) {
        let fqn = ctx.declared_name(&e.name);
        for (name, span) in e.implements.iter().zip(e.implements_spans.iter()) {
            if let Some(target) = resolve_class_ref(name, Some(&fqn), ctx) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for (name, span) in e.uses.iter().zip(e.uses_spans.iter()) {
            if let Some(target) = resolve_class_ref(name, Some(&fqn), ctx) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for m in &e.methods {
            self.function(&m.function, &mut ctx.clone(), Some(&fqn), out);
        }
        for cst in &e.consts {
            self.expr(&cst.value, &mut ctx.clone(), Some(&fqn), out);
        }
        for case in &e.cases {
            if let Some(v) = &case.value {
                self.expr(v, &mut ctx.clone(), Some(&fqn), out);
            }
        }
    }
}
