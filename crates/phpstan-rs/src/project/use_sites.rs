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
//! and merged into this same index by `Project::ensure_member_index_built`,
//! the analysis (`Project::keep_analysed_check`) and each recheck
//! (`UseSiteIndex::replace_members`).

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::{Visitor, walk_args, walk_catch_clause, walk_class_def, walk_expr, walk_stmts};

/// What a reference site points at - no local-variable variant (see
/// this item's own edge-case notes: local-variable rename/references is
/// explicitly out of v1 scope, always answered via a `None` short-circuit
/// before this index is ever consulted).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum RefTarget {
    Class(String),
    Function(String),
    /// An unqualified function name in a namespace, by its namespaced
    /// name: that function when one is declared as the call runs, else the
    /// global one of the same name.
    UnqualifiedFunction(String),
    /// `(declaring owner FQN, member name)` - always the true,
    /// ancestry-resolved declaring class, never the raw syntactic
    /// receiver/class name a `self::`/inherited-call site was written
    /// against. A method is named as it is declared.
    Member(String, String),
}

impl RefTarget {
    /// The target as the index keys it: class and function names, which
    /// PHP compares without case, in lowercase. A member's name keeps its
    /// case: a property's and a constant's are compared with it.
    fn folded(self) -> RefTarget {
        match self {
            RefTarget::Class(name) => RefTarget::Class(name.to_ascii_lowercase()),
            RefTarget::Function(name) => RefTarget::Function(name.to_ascii_lowercase()),
            RefTarget::UnqualifiedFunction(name) => {
                RefTarget::UnqualifiedFunction(name.to_ascii_lowercase())
            }
            RefTarget::Member(owner, member) => {
                RefTarget::Member(owner.to_ascii_lowercase(), member)
            }
        }
    }
}

/// A function's name after its namespace.
fn last_segment(name: &str) -> &str {
    name.rsplit_once('\\').map_or(name, |(_, last)| last)
}

/// A site that may reach a function, and the other function it reaches
/// instead when that one is declared: the global one for an unqualified
/// call naming a namespaced function, the namespaced one for an
/// unqualified call falling back to a global function.
pub(crate) struct FunctionSite<'a> {
    pub(crate) path: &'a Path,
    pub(crate) span: Span,
    pub(crate) other: Option<&'a str>,
}

/// Every reference site this project has ever indexed, in both
/// directions: `by_target` answers "where is X used" (`references`'s own
/// query shape); `by_file` answers "what did this file contribute"
/// (`remove_file`'s own undo shape).
#[derive(Default)]
pub(crate) struct UseSiteIndex {
    by_target: FxHashMap<RefTarget, Vec<(PathBuf, Span)>>,
    by_file: FxHashMap<PathBuf, Vec<(RefTarget, Span)>>,
    // For each global function name, the namespaced names unqualified
    // calls are recorded under that fall back to it.
    fallbacks: FxHashMap<String, rustc_hash::FxHashSet<String>>,
}

impl UseSiteIndex {
    pub(crate) fn insert(&mut self, path: &Path, target: RefTarget, span: Span) {
        let target = target.folded();
        if let RefTarget::UnqualifiedFunction(namespaced) = &target {
            self.fallbacks
                .entry(last_segment(namespaced).to_string())
                .or_default()
                .insert(namespaced.clone());
        }
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
            self.unlink(path, &target, span);
        }
    }

    /// Replaces the member sites `path` contributed with `members`, those
    /// its last check found, keeping its class and function sites.
    pub(crate) fn replace_members(&mut self, path: &Path, members: Vec<(Span, String, String)>) {
        if let Some(entries) = self.by_file.get_mut(path) {
            let dropped: Vec<(RefTarget, Span)> = entries
                .extract_if(.., |(target, _)| matches!(target, RefTarget::Member(..)))
                .collect();
            if entries.is_empty() {
                self.by_file.remove(path);
            }
            for (target, span) in dropped {
                self.unlink(path, &target, span);
            }
        }
        for (span, owner, member) in members {
            self.insert(path, RefTarget::Member(owner, member), span);
        }
    }

    /// Drops `path`'s site `span` from `target`'s.
    fn unlink(&mut self, path: &Path, target: &RefTarget, span: Span) {
        let Some(sites) = self.by_target.get_mut(target) else {
            return;
        };
        sites.retain(|(p, s)| !(p == path && *s == span));
        if sites.is_empty() {
            self.by_target.remove(target);
            if let RefTarget::UnqualifiedFunction(namespaced) = target {
                let global = last_segment(namespaced);
                if let Some(names) = self.fallbacks.get_mut(global) {
                    names.remove(namespaced);
                    if names.is_empty() {
                        self.fallbacks.remove(global);
                    }
                }
            }
        }
    }

    /// The sites recorded for `target` itself, its names compared as PHP
    /// compares them. A function's are `function_references`.
    pub(crate) fn references(&self, target: &RefTarget) -> &[(PathBuf, Span)] {
        self.by_target
            .get(&target.clone().folded())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Every site that may reach function `name`: the calls naming it,
    /// and the unqualified calls in a namespace that reach it or the
    /// other function of their two.
    pub(crate) fn function_references(&self, name: &str) -> Vec<FunctionSite<'_>> {
        let key = name.to_ascii_lowercase();
        let sites = |target: &RefTarget| self.by_target.get(target).into_iter().flatten();
        let mut out: Vec<FunctionSite<'_>> = sites(&RefTarget::Function(key.clone()))
            .map(|(path, span)| FunctionSite {
                path,
                span: *span,
                other: None,
            })
            .collect();
        if let Some((RefTarget::UnqualifiedFunction(namespaced), found)) = self
            .by_target
            .get_key_value(&RefTarget::UnqualifiedFunction(key.clone()))
        {
            let global = last_segment(namespaced);
            out.extend(found.iter().map(|(path, span)| FunctionSite {
                path,
                span: *span,
                other: Some(global),
            }));
        }
        for namespaced in self.fallbacks.get(&key).into_iter().flatten() {
            out.extend(
                sites(&RefTarget::UnqualifiedFunction(namespaced.clone())).map(|(path, span)| {
                    FunctionSite {
                        path,
                        span: *span,
                        other: Some(namespaced.as_str()),
                    }
                }),
            );
        }
        out
    }

    /// Every site of `target`: a function's as `function_references`
    /// gives them, anything else's as `references`.
    pub(crate) fn sites(&self, target: &RefTarget) -> Vec<(&Path, Span)> {
        match target {
            RefTarget::Function(name) | RefTarget::UnqualifiedFunction(name) => self
                .function_references(name)
                .into_iter()
                .map(|site| (site.path, site.span))
                .collect(),
            _ => self
                .references(target)
                .iter()
                .map(|(path, span)| (path.as_path(), *span))
                .collect(),
        }
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
        let target = match name.candidates() {
            (namespaced, Some(global)) if !namespaced.eq_ignore_ascii_case(global) => {
                RefTarget::UnqualifiedFunction(namespaced.to_string())
            }
            (fqn, _) => RefTarget::Function(fqn.to_string()),
        };
        self.out.push((span, target));
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

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.class_at(&attribute.name, attribute.name.span);
        walk_args(self, &attribute.args);
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
    UnqualifiedFunction(name),
    Member(class, member),
});

/// Each target's sites and each file's as sets: they are in the order the
/// files were indexed in.
impl syntax::canonical::Canonical for UseSiteIndex {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let UseSiteIndex {
            by_target,
            by_file,
            fallbacks,
        } = self;
        p.node(
            "UseSiteIndex",
            &[
                ("by_target", &syntax::canonical::MapOfSets(by_target)),
                ("by_file", &syntax::canonical::MapOfSets(by_file)),
                ("fallbacks", fallbacks),
            ],
        );
    }
}
