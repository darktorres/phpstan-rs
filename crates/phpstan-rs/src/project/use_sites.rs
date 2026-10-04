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
//! behind `retain_source`); a
//! batch `check-project` run has no retained source but, when `project::
//! whole_program::dead_code` is enabled, still needs to know *whether* a
//! class/function is referenced at all -
//! `index_class_fn_refs_without_source` runs instead there, trading
//! precise sub-spans (needs real source text to carve out of a combined
//! multi-identifier span) for real, already-on-hand AST-node spans that
//! are merely less narrowly scoped - see that function's own doc
//! comment. Not run unconditionally, though: this walk is too
//! expensive to pay
//! for every loaded file (including every vendor/context one) when
//! nothing consumes it - `Project::insert_parsed_file` gates it on
//! `check_dead_code` specifically, its only batch-mode consumer.
//!
//! Tier 2 (member reference sites - method/property/class-constant
//! access) needs a real, checker-driven resolution of the receiver's
//! declaring owner - built separately, in `checker.rs`/`checker/expr.rs`,
//! and merged into this same index by `Project::ensure_member_index_built`/
//! `Project::reindex_tier2_for_file`.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::{Visitor, walk_catch_clause, walk_class_def, walk_expr, walk_stmts};

/// What a reference site points at - no local-variable variant (see
/// this item's own edge-case notes: local-variable rename/references is
/// explicitly out of v1 scope, always answered via a `None` short-circuit
/// before this index is ever consulted).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
/// (`remove_file`'s own undo shape).
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
    let mut w = ClassFnRefWalker {
        source: Some(source),
        out: Vec::new(),
    };
    walk_stmts(&mut w, stmts);
    w.out
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
    let mut w = ClassFnRefWalker {
        source: None,
        out: Vec::new(),
    };
    walk_stmts(&mut w, stmts);
    w.out
}

struct ClassFnRefWalker<'a> {
    source: Option<&'a str>,
    out: Vec<(Span, RefTarget)>,
}

impl ClassFnRefWalker<'_> {
    /// Records `name`, a class name, at its span, or at `whole` without
    /// the source: then a site is the span its node already has.
    fn class_at(&mut self, name: &QualifiedName, whole: Span) {
        if let Some(fqn) = resolve_class_ref(name) {
            let span = if self.source.is_some() {
                name.span
            } else {
                whole
            };
            self.out.push((span, RefTarget::Class(fqn)));
        }
    }

    /// Records `name`, a function name as a call or callable names it.
    fn function_at(&mut self, name: &QualifiedName, span: Span) {
        let fqn = name.candidates().0.to_string();
        self.out.push((span, RefTarget::Function(fqn)));
    }
}

impl<'ast> Visitor<'ast> for ClassFnRefWalker<'_> {
    fn visit_type_hint(&mut self, hint: &'ast TypeHint) {
        for name in hint.class_names() {
            self.class_at(name, hint.span);
        }
    }

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        for name in class
            .extends
            .iter()
            .chain(&class.implements)
            .chain(class.used_traits())
        {
            self.class_at(name, name.span);
        }
        walk_class_def(self, class);
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        for name in &clause.types {
            self.class_at(name, name.span);
        }
        walk_catch_clause(self, clause);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        syntax::stack::guard(|| {
            match &expr.kind {
                // The callee's name is recorded here, as a function or a
                // class; only the arguments are walked.
                ExprKind::Call(callee, args) | ExprKind::New(callee, args)
                    if matches!(callee.kind, ExprKind::Name(_)) =>
                {
                    let ExprKind::Name(n) = &callee.kind else {
                        unreachable!()
                    };
                    if matches!(expr.kind, ExprKind::Call(..)) {
                        self.function_at(n, callee.span);
                    } else {
                        self.class_at(n, callee.span);
                    }
                    for arg in args {
                        self.visit_arg(arg);
                    }
                    return;
                }
                // A bare `Name` reached any other way than a callee (e.g. a
                // first-class-callable target `foo(...)`) is taken as a
                // function.
                ExprKind::Name(n) => self.function_at(n, expr.span),
                ExprKind::ClassConstFetch(StaticTarget::Name(class), _)
                | ExprKind::ClassNameFetch(StaticTarget::Name(class))
                | ExprKind::StaticPropertyFetch(StaticTarget::Name(class), _)
                | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Name(class), _)
                | ExprKind::StaticCall(StaticTarget::Name(class), _, _) => {
                    self.class_at(class, expr.span);
                }
                ExprKind::Instanceof(_, InstanceofTarget::Name(name)) => {
                    self.class_at(name, name.span);
                }
                _ => {}
            }
            walk_expr(self, expr);
        })
    }
}

syntax::canonical_enum!(RefTarget {
    Class(name),
    Function(name),
    Member(class, member),
});

syntax::canonical_struct!(UseSiteIndex { by_target, by_file });
