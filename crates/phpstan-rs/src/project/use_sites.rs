//! `textDocument/references`/`textDocument/rename`'s own whole-project
//! use-site index - two tiers, kept as two separate mechanisms because
//! they need fundamentally different machinery:
//!
//! Tier 1 (this module): class/function reference sites - a purely
//! syntactic, name-resolution-only walk (`ClassFnRefWalker`), structured
//! after (not sharing code with) `lsp::navigation::Walker` - the same
//! proven traversal shape, reused for "record every match", never "find
//! the one match at a cursor". Real caret-precision spans need the
//! file's retained source text (`index_class_fn_refs`, LSP-only, gated
//! behind `retain_source` exactly like `DeclLocations` used to be); a
//! batch `check-project` run has no retained source but, when `project::
//! whole_program::dead_code` is enabled, still needs to know *whether* a
//! class/function is referenced at all -
//! `index_class_fn_refs_without_source` runs instead there, trading
//! precise sub-spans (needs real source text to carve out of a combined
//! multi-identifier span) for real, already-on-hand AST-node spans that
//! are merely less narrowly scoped - see that function's own doc
//! comment. NOT run unconditionally the way `DeclLocations` now is,
//! though: measured on a real corpus, this walk is too expensive to pay
//! for every loaded file (including every vendor/context one) when
//! nothing consumes it - `Project::insert_parsed_file` gates it on
//! `check_dead_code` specifically, its only batch-mode consumer.
//!
//! Tier 2 (member reference sites - method/property/class-constant
//! access) needs a real, checker-driven resolution of the receiver's
//! declaring owner - built separately, in `checker.rs`/`checker/expr.rs`,
//! and merged into this same index by `Project::ensure_member_index_built`/
//! `Project::reindex_tier2_for_file`.

use crate::registry::class_decl_fqn;
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{any_child_expr, any_child_stmt};

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
fn resolve_class_ref(name: &QualifiedName) -> Option<String> {
    name.resolved_class().map(str::to_string)
}

/// Every `(Span, RefTarget::Class|Function)` pair in `stmts` - Tier 1's
/// own entry point. `parent`/member halves of a `ClassConstFetch`/`StaticPropertyFetch`/
/// `StaticCall` are deliberately NOT resolved here (parent needs the
/// registry's own ancestor lookup only `resolve_class_ref` in
/// `navigation.rs` does via `registry.classes`, harmless to add later but
/// unneeded for Tier 1's own class/function scope; the member half is
/// Tier 2's job entirely, via a real Checker-resolved declaring owner).
pub(crate) fn index_class_fn_refs(stmts: &[Stmt], source: &str) -> Vec<(Span, RefTarget)> {
    let mut out = Vec::new();
    let w = ClassFnRefWalker {
        source: Some(source),
    };
    w.stmts(stmts, None, &mut out);
    out
}

/// Same walk, for a caller with no retained source text at all (a batch
/// `check-project` run - `FileState.source` is `None` there, see
/// `Project::retain_source`'s own doc comment). Each of the 3 spots below
/// that would otherwise need `split_identifier_spans` to carve a precise
/// sub-span out of a combined multi-identifier text span instead records
/// the whole already-real AST span it has on hand - still a correct
/// reference *site* (this project's own convention for "the identifier's
/// own span, when its owning node doesn't carve out a narrower one"), just
/// not narrowed to the exact identifier's own characters. Existence-only
/// callers (a whole-program "was this ever referenced" check, e.g.
/// `project::whole_program::dead_code`) never read a use-site's span at
/// all, so the imprecision is invisible to them; a real caret-precision
/// consumer (`textDocument/references`/`rename`) only ever runs with
/// `retain_source` on, so it always gets `index_class_fn_refs`'s own exact
/// version above, never this one.
pub(crate) fn index_class_fn_refs_without_source(stmts: &[Stmt]) -> Vec<(Span, RefTarget)> {
    let mut out = Vec::new();
    let w = ClassFnRefWalker { source: None };
    w.stmts(stmts, None, &mut out);
    out
}

struct ClassFnRefWalker<'a> {
    source: Option<&'a str>,
}

impl<'a> ClassFnRefWalker<'a> {
    fn record_type_hint(&self, out: &mut Vec<(Span, RefTarget)>, hint: &TypeHint) {
        for (name, name_span) in hint.class_names() {
            if let Some(fqn) = resolve_class_ref(name) {
                // Without the source, a site is the whole declaration.
                let span = if self.source.is_some() {
                    name_span
                } else {
                    hint.span
                };
                out.push((span, RefTarget::Class(fqn)));
            }
        }
    }

    /// A parameter's own default-value expression (including a promoted
    /// constructor parameter's own default) - not visited by
    /// `lsp::navigation::Walker::param` at all (that walker only ever
    /// needs a parameter's type-hint/name-declaration site for
    /// cursor-driven hover/definition), but a legitimate home for a bare
    /// `Name`/`ClassNameFetch` reference this item's own index
    /// must not silently miss.
    fn param(&self, p: &Param, current_class: Option<&str>, out: &mut Vec<(Span, RefTarget)>) {
        if let Some(hint) = &p.type_hint {
            self.record_type_hint(out, hint);
        }
        if let Some(d) = &p.default {
            self.expr(d, current_class, out);
        }
    }

    fn stmts(&self, stmts: &[Stmt], current_class: Option<&str>, out: &mut Vec<(Span, RefTarget)>) {
        for stmt in stmts {
            self.stmt(stmt, current_class, out);
        }
    }

    fn stmt(&self, stmt: &Stmt, current_class: Option<&str>, out: &mut Vec<(Span, RefTarget)>) {
        syntax::stack::guard(|| {
            match &stmt.kind {
                StmtKind::Expression(e) | StmtKind::Throw(e) => self.expr(e, current_class, out),
                StmtKind::Return(Some(e)) => self.expr(e, current_class, out),
                StmtKind::Echo(exprs) | StmtKind::Unset(exprs) => {
                    for e in exprs {
                        self.expr(e, current_class, out);
                    }
                }
                StmtKind::If(if_stmt) => {
                    for cond in if_stmt.conditions() {
                        self.expr(cond, current_class, out);
                    }
                }
                StmtKind::While { cond, .. } | StmtKind::DoWhile { cond, .. } => {
                    self.expr(cond, current_class, out);
                }
                StmtKind::For {
                    init, cond, step, ..
                } => {
                    for e in init.iter().chain(cond).chain(step) {
                        self.expr(e, current_class, out);
                    }
                }
                StmtKind::Foreach { subject, .. } => {
                    self.expr(subject, current_class, out);
                }
                StmtKind::FunctionDecl(f) => {
                    self.function(f, current_class, out);
                }
                StmtKind::ClassDecl(c) => self.class(c, out),
                StmtKind::GlobalConst(consts) => {
                    for c in consts {
                        self.expr(&c.value, current_class, out);
                    }
                }
                StmtKind::StaticVar(vars) => {
                    for (_, d) in vars {
                        if let Some(e) = d {
                            self.expr(e, current_class, out);
                        }
                    }
                }
                StmtKind::Switch { subject, cases } => {
                    self.expr(subject, current_class, out);
                    for case in cases {
                        if let Some(t) = &case.test {
                            self.expr(t, current_class, out);
                        }
                    }
                }
                StmtKind::Try { catches, .. } => {
                    for c in catches {
                        for (name, span) in c.types.iter().zip(c.types_spans.iter()) {
                            if let Some(fqn) = resolve_class_ref(name) {
                                out.push((*span, RefTarget::Class(fqn)));
                            }
                        }
                    }
                }
                _ => {}
            }
            any_child_stmt(stmt, |child| {
                self.stmt(child, current_class, out);
                false
            });
        })
    }

    fn expr(&self, expr: &Expr, current_class: Option<&str>, out: &mut Vec<(Span, RefTarget)>) {
        syntax::stack::guard(|| {
            match &expr.kind {
                ExprKind::Call(callee, args) => {
                    if let ExprKind::Name(n) = &callee.kind {
                        let fqn = n.candidates().0.to_string();
                        out.push((callee.span, RefTarget::Function(fqn)));
                    } else {
                        self.expr(callee, current_class, out);
                    }
                    for a in args {
                        self.expr(&a.value, current_class, out);
                    }
                    return;
                }
                ExprKind::New(callee, args) => {
                    if let ExprKind::Name(n) = &callee.kind {
                        if let Some(fqn) = resolve_class_ref(n) {
                            out.push((callee.span, RefTarget::Class(fqn)));
                        }
                    } else {
                        self.expr(callee, current_class, out);
                    }
                    for a in args {
                        self.expr(&a.value, current_class, out);
                    }
                    return;
                }
                ExprKind::Name(n) => {
                    // A bare `Name` node reached any other way than a
                    // `Call`/`New` callee (e.g. a first-class-callable
                    // target `foo(...)`) is taken as a function.
                    let fqn = n.candidates().0.to_string();
                    out.push((expr.span, RefTarget::Function(fqn)));
                }
                ExprKind::ClassConstFetch(StaticTarget::Name(class, class_span), _)
                | ExprKind::ClassNameFetch(StaticTarget::Name(class, class_span))
                | ExprKind::StaticPropertyFetch(StaticTarget::Name(class, class_span), _)
                | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Name(class, class_span), _) => {
                    if let Some(fqn) = resolve_class_ref(class) {
                        let span = if self.source.is_some() {
                            *class_span
                        } else {
                            expr.span
                        };
                        out.push((span, RefTarget::Class(fqn)));
                    }
                }
                // A genuinely dynamic receiver has no class-name text to
                // record a reference for here at all - any name reference
                // buried inside it is still picked up by the generic
                // `any_child_expr` walk below (it's a real child of this
                // node now, not a discarded `"?"` sentinel).
                ExprKind::StaticCall(StaticTarget::Name(class, class_span), _, _) => {
                    if let Some(fqn) = resolve_class_ref(class) {
                        let span = if self.source.is_some() {
                            *class_span
                        } else {
                            expr.span
                        };
                        out.push((span, RefTarget::Class(fqn)));
                    }
                }
                ExprKind::Instanceof(_, InstanceofTarget::Name(name, span)) => {
                    if let Some(fqn) = resolve_class_ref(name) {
                        out.push((*span, RefTarget::Class(fqn)));
                    }
                }
                ExprKind::NewAnonymousClass(decl, args) => {
                    self.class(decl, out);
                    for a in args {
                        self.expr(&a.value, current_class, out);
                    }
                    return;
                }
                ExprKind::Closure { params, body, .. } => {
                    for p in params {
                        self.param(p, current_class, out);
                    }
                    self.stmts(body, current_class, out);
                    return;
                }
                ExprKind::ArrowFn { params, body, .. } => {
                    for p in params {
                        self.param(p, current_class, out);
                    }
                    self.expr(body, current_class, out);
                    return;
                }
                _ => {}
            }
            any_child_expr(expr, |child| {
                self.expr(child, current_class, out);
                false
            });
        })
    }

    fn function(
        &self,
        f: &FunctionDecl,
        current_class: Option<&str>,
        out: &mut Vec<(Span, RefTarget)>,
    ) {
        for p in &f.params {
            self.param(p, current_class, out);
        }
        if let Some(rt) = &f.return_type {
            self.record_type_hint(out, rt);
        }
        self.stmts(&f.body, current_class, out);
    }

    fn class(&self, c: &ClassDecl, out: &mut Vec<(Span, RefTarget)>) {
        let fqn = class_decl_fqn(c).to_string();
        for (name, span) in c.extends.iter().zip(c.extends_spans.iter()) {
            if let Some(target) = resolve_class_ref(name) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for (name, span) in c.implements.iter().zip(c.implements_spans.iter()) {
            if let Some(target) = resolve_class_ref(name) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for (name, span) in c.uses.iter().zip(c.uses_spans.iter()) {
            if let Some(target) = resolve_class_ref(name) {
                out.push((*span, RefTarget::Class(target)));
            }
        }
        for p in &c.properties {
            if let Some(hint) = &p.type_hint {
                self.record_type_hint(out, hint);
            }
            if let Some(d) = &p.default {
                self.expr(d, Some(&fqn), out);
            }
        }
        for m in &c.methods {
            self.function(&m.function, Some(&fqn), out);
        }
        for cst in &c.consts {
            if let Some(hint) = &cst.type_hint {
                self.record_type_hint(out, hint);
            }
            self.expr(&cst.value, Some(&fqn), out);
        }
        for v in c.enum_cases.iter().filter_map(|case| case.value.as_ref()) {
            self.expr(v, Some(&fqn), out);
        }
    }
}
