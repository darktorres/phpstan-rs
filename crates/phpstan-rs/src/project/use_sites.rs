//! `textDocument/references`/`textDocument/rename`'s own whole-project
//! use-site index - two tiers, kept as two separate mechanisms because
//! they need fundamentally different machinery:
//!
//! Tier 1 (this module): class/function reference sites - a purely
//! syntactic, name-resolution-only walk (`ClassFnRefWalker`), structured
//! after (not sharing code with) `lsp::navigation::Walker` - the same
//! proven traversal shape, reused for "record every match", never "find
//! the one match at a cursor". It is a product of each file's parse
//! (`index_class_fn_refs`, from the file's text, with the span of each
//! identifier itself), kept with the file's other products, and run only
//! when something consumes it - the language server's references and
//! rename, or the dead code check (`Project::indexes_use_sites`): the walk
//! is too expensive to pay for every loaded file (including every
//! vendor/context one) otherwise. `index_class_fn_refs_without_source`
//! walks the tree of a file whose text is no longer at hand, for the
//! whole-program checks that read only whether a name is referenced.
//!
//! Tier 2 (member reference sites - method/property/class-constant
//! access) needs a real, checker-driven resolution of the receiver's
//! declaring owner - built separately, in `checker.rs`/`checker/expr.rs`,
//! and merged into this same index by `Project::ensure_member_index_built`,
//! the analysis (`Project::keep_analysed_check`) and each recheck
//! (`UseSiteIndex::replace_members`).

use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::arena::{self, Visitor};

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
pub(crate) struct UseSiteIndex {
    by_target: Vec<FxHashMap<RefTarget, Vec<(PathBuf, Span)>>>,
    by_file: Vec<FxHashMap<PathBuf, Vec<(RefTarget, Span)>>>,
    // For each global function name, the namespaced names unqualified
    // calls are recorded under that fall back to it.
    fallbacks: Vec<FxHashMap<String, rustc_hash::FxHashSet<String>>>,
}

const USE_SITE_SHARDS: usize = 32;

fn site_shard<K: Hash + ?Sized>(key: &K) -> usize {
    let mut hasher = rustc_hash::FxHasher::default();
    key.hash(&mut hasher);
    (hasher.finish() >> (u64::BITS - 5)) as usize
}

impl Default for UseSiteIndex {
    fn default() -> Self {
        Self {
            by_target: (0..USE_SITE_SHARDS).map(|_| FxHashMap::default()).collect(),
            by_file: (0..USE_SITE_SHARDS).map(|_| FxHashMap::default()).collect(),
            fallbacks: (0..USE_SITE_SHARDS).map(|_| FxHashMap::default()).collect(),
        }
    }
}

impl UseSiteIndex {
    pub(crate) fn extend_files(&mut self, files: Vec<(PathBuf, Vec<(Span, RefTarget)>)>) {
        use rayon::prelude::*;

        struct Buckets {
            targets: [Vec<(RefTarget, (PathBuf, Span))>; USE_SITE_SHARDS],
            files: [Vec<(PathBuf, (RefTarget, Span))>; USE_SITE_SHARDS],
            fallbacks: [Vec<(String, String)>; USE_SITE_SHARDS],
        }

        let per_file: Vec<Buckets> = files
            .into_par_iter()
            .map(|(path, sites)| {
                let mut buckets = Buckets {
                    targets: std::array::from_fn(|_| Vec::new()),
                    files: std::array::from_fn(|_| Vec::new()),
                    fallbacks: std::array::from_fn(|_| Vec::new()),
                };
                for (span, target) in sites {
                    let target = target.folded();
                    buckets.targets[site_shard(&target)]
                        .push((target.clone(), (path.clone(), span)));
                    buckets.files[site_shard(&path)].push((path.clone(), (target.clone(), span)));
                    if let RefTarget::UnqualifiedFunction(namespaced) = target {
                        let global = last_segment(&namespaced).to_owned();
                        buckets.fallbacks[site_shard(&global)].push((global, namespaced));
                    }
                }
                buckets
            })
            .collect();

        let mut targets: [Vec<(RefTarget, (PathBuf, Span))>; USE_SITE_SHARDS] =
            std::array::from_fn(|_| Vec::new());
        let mut by_file: [Vec<(PathBuf, (RefTarget, Span))>; USE_SITE_SHARDS] =
            std::array::from_fn(|_| Vec::new());
        let mut fallbacks: [Vec<(String, String)>; USE_SITE_SHARDS] =
            std::array::from_fn(|_| Vec::new());
        for mut file_buckets in per_file {
            for (target, source) in targets.iter_mut().zip(&mut file_buckets.targets) {
                target.append(source);
            }
            for (target, source) in by_file.iter_mut().zip(&mut file_buckets.files) {
                target.append(source);
            }
            for (target, source) in fallbacks.iter_mut().zip(&mut file_buckets.fallbacks) {
                target.append(source);
            }
        }

        // The three tables partition by different keys, so assemble each
        // independently after the per-file parallel bucketing above.
        self.by_target
            .par_iter_mut()
            .zip(targets.into_par_iter())
            .for_each(|(shard, entries)| {
                for (target, site) in entries {
                    shard.entry(target).or_default().push(site);
                }
            });
        self.by_file
            .par_iter_mut()
            .zip(by_file.into_par_iter())
            .for_each(|(shard, entries)| {
                for (path, site) in entries {
                    shard.entry(path).or_default().push(site);
                }
            });
        self.fallbacks
            .par_iter_mut()
            .zip(fallbacks.into_par_iter())
            .for_each(|(shard, entries)| {
                for (global, namespaced) in entries {
                    shard.entry(global).or_default().insert(namespaced);
                }
            });
    }

    pub(crate) fn insert(&mut self, path: &Path, target: RefTarget, span: Span) {
        let target = target.folded();
        if let RefTarget::UnqualifiedFunction(namespaced) = &target {
            self.fallbacks[site_shard(last_segment(namespaced))]
                .entry(last_segment(namespaced).to_string())
                .or_default()
                .insert(namespaced.clone());
        }
        self.by_target[site_shard(&target)]
            .entry(target.clone())
            .or_default()
            .push((path.to_path_buf(), span));
        self.by_file[site_shard(path)]
            .entry(path.to_path_buf())
            .or_default()
            .push((target, span));
    }

    /// Drops every site `path` previously contributed, from both
    /// directions - called unconditionally on every re-index/removal of
    /// `path` (a no-op before anything was ever inserted for it, in
    /// particular before Tier 2's own index is first built at all).
    pub(crate) fn remove_file(&mut self, path: &Path) {
        let Some(entries) = self.by_file[site_shard(path)].remove(path) else {
            return;
        };
        for (target, span) in entries {
            self.unlink(path, &target, span);
        }
    }

    /// Replaces the member sites `path` contributed with `members`, those
    /// its last check found, keeping its class and function sites.
    pub(crate) fn replace_members(&mut self, path: &Path, members: Vec<(Span, String, String)>) {
        let file_shard = site_shard(path);
        if let Some(entries) = self.by_file[file_shard].get_mut(path) {
            let dropped: Vec<(RefTarget, Span)> = entries
                .extract_if(.., |(target, _)| matches!(target, RefTarget::Member(..)))
                .collect();
            if entries.is_empty() {
                self.by_file[file_shard].remove(path);
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
        let target_shard = site_shard(target);
        let Some(sites) = self.by_target[target_shard].get_mut(target) else {
            return;
        };
        sites.retain(|(p, s)| !(p == path && *s == span));
        if sites.is_empty() {
            self.by_target[target_shard].remove(target);
            if let RefTarget::UnqualifiedFunction(namespaced) = target {
                let global = last_segment(namespaced);
                let fallback_shard = site_shard(global);
                if let Some(names) = self.fallbacks[fallback_shard].get_mut(global) {
                    names.remove(namespaced);
                    if names.is_empty() {
                        self.fallbacks[fallback_shard].remove(global);
                    }
                }
            }
        }
    }

    /// The sites recorded for `target` itself, its names compared as PHP
    /// compares them. A function's are `function_references`.
    pub(crate) fn references(&self, target: &RefTarget) -> &[(PathBuf, Span)] {
        let target = target.clone().folded();
        self.by_target[site_shard(&target)]
            .get(&target)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Every site that may reach function `name`: the calls naming it,
    /// and the unqualified calls in a namespace that reach it or the
    /// other function of their two.
    pub(crate) fn function_references(&self, name: &str) -> Vec<FunctionSite<'_>> {
        let key = name.to_ascii_lowercase();
        let sites = |target: &RefTarget| {
            self.by_target[site_shard(target)]
                .get(target)
                .into_iter()
                .flatten()
        };
        let mut out: Vec<FunctionSite<'_>> = sites(&RefTarget::Function(key.clone()))
            .map(|(path, span)| FunctionSite {
                path,
                span: *span,
                other: None,
            })
            .collect();
        if let Some((RefTarget::UnqualifiedFunction(namespaced), found)) = self.by_target
            [site_shard(&RefTarget::UnqualifiedFunction(key.clone()))]
        .get_key_value(&RefTarget::UnqualifiedFunction(key.clone()))
        {
            let global = last_segment(namespaced);
            out.extend(found.iter().map(|(path, span)| FunctionSite {
                path,
                span: *span,
                other: Some(global),
            }));
        }
        for namespaced in self.fallbacks[site_shard(&key)]
            .get(&key)
            .into_iter()
            .flatten()
        {
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
/// `navigation.rs` does via `registry.classes()`, harmless to add later but
/// unneeded for Tier 1's own class/function scope; the member half is
/// Tier 2's job entirely, via a real Checker-resolved declaring owner).
pub(crate) fn index_class_fn_refs(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    source: &str,
) -> Vec<(Span, RefTarget)> {
    let mut w = ClassFnRefWalker {
        source: Some(source),
        out: Vec::new(),
    };
    arena::walk_stmts(&mut w, arenas, stmts);
    w.out
}

/// Same walk, for a caller with no source text at hand: the whole-program
/// checks, which walk the tree of an analysed file after it is loaded. Each
/// of the 3 spots below that would otherwise need `split_identifier_spans`
/// to carve a precise sub-span out of a combined multi-identifier text span
/// instead records the whole already-real AST span it has on hand - still a
/// correct reference *site*, just not narrowed to the exact identifier's own
/// characters. A caller that reads only whether a name is referenced never
/// reads a span; the index `textDocument/references`/`rename` read is built
/// with `index_class_fn_refs`.
pub(crate) fn index_class_fn_refs_without_source(
    arenas: &NodeArenas,
    stmts: &[StmtId],
) -> Vec<(Span, RefTarget)> {
    let mut w = ClassFnRefWalker {
        source: None,
        out: Vec::new(),
    };
    arena::walk_stmts(&mut w, arenas, stmts);
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

impl Visitor for ClassFnRefWalker<'_> {
    fn visit_type_hint(&mut self, arenas: &NodeArenas, hint: &TypeHint) {
        for name in hint.class_names(arenas) {
            self.class_at(name, hint.span);
        }
    }

    fn visit_class_def(&mut self, arenas: &NodeArenas, class: &ClassDef) {
        for name in class
            .extends(arenas)
            .iter()
            .chain(class.implements(arenas))
            .chain(class.used_traits(arenas))
        {
            self.class_at(name, name.span);
        }
        arena::walk_class_def(self, arenas, class);
    }

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        for name in arenas.qualified_names(&clause.types) {
            self.class_at(name, name.span);
        }
        arena::walk_catch_clause(self, arenas, clause);
    }

    fn visit_attribute(&mut self, arenas: &NodeArenas, attribute: &Attribute) {
        self.class_at(&attribute.name, attribute.name.span);
        for arg in arenas.args(attribute.args.clone()) {
            self.visit_arg(arenas, arg);
        }
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else { return };
        syntax::stack::guard(|| {
            match &expr.kind {
                // The callee's name is recorded here, as a function or a
                // class; only the arguments are walked.
                ExprKind::Call(callee, args) | ExprKind::New(callee, args)
                    if matches!(
                        arenas.expr(callee.clone()).map(|node| &node.kind),
                        Some(ExprKind::Name(_))
                    ) =>
                {
                    let Some(callee_node) = arenas.expr(callee.clone()) else {
                        return;
                    };
                    let ExprKind::Name(n) = &callee_node.kind else {
                        unreachable!()
                    };
                    let Some(n) = arenas.qualified_name(n) else {
                        return;
                    };
                    if matches!(expr.kind, ExprKind::Call(..)) {
                        self.function_at(
                            n,
                            arenas
                                .expr_span(callee_node.id)
                                .expect("callee expression has a parallel span"),
                        );
                    } else {
                        self.class_at(
                            n,
                            arenas
                                .expr_span(callee_node.id)
                                .expect("callee expression has a parallel span"),
                        );
                    }
                    for arg in arenas.args(args.clone()) {
                        self.visit_arg(arenas, arg);
                    }
                    return;
                }
                ExprKind::ClassConstFetch(StaticTarget::Name(class), _)
                | ExprKind::ClassNameFetch(StaticTarget::Name(class))
                | ExprKind::StaticPropertyFetch(StaticTarget::Name(class), _)
                | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Name(class), _)
                | ExprKind::StaticCall(StaticTarget::Name(class), _, _) => {
                    self.class_at(
                        class,
                        arenas
                            .expr_span(expr.id)
                            .expect("expression has a parallel span"),
                    );
                }
                ExprKind::Instanceof(_, InstanceofTarget::Name(name)) => {
                    if let Some(name) = arenas.qualified_name(name) {
                        self.class_at(name, name.span);
                    }
                }
                _ => {}
            }
            arena::walk_expr(self, arenas, id);
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
        let by_target: FxHashMap<_, _> = self
            .by_target
            .iter()
            .flat_map(|shard| shard.iter())
            .map(|(target, sites)| (target, syntax::canonical::AsSet(sites.as_slice())))
            .collect();
        let by_file: FxHashMap<_, _> = self
            .by_file
            .iter()
            .flat_map(|shard| shard.iter())
            .map(|(path, sites)| (path, syntax::canonical::AsSet(sites.as_slice())))
            .collect();
        let fallbacks: FxHashMap<_, _> = self
            .fallbacks
            .iter()
            .flat_map(|shard| shard.iter())
            .collect();
        p.node(
            "UseSiteIndex",
            &[
                ("by_target", &by_target),
                ("by_file", &by_file),
                ("fallbacks", &fallbacks),
            ],
        );
    }
}

#[cfg(test)]
mod sharded_index_tests {
    use super::*;

    #[test]
    fn parallel_file_merge_and_removal_keep_both_index_directions() {
        let first = PathBuf::from("first.php");
        let second = PathBuf::from("second.php");
        let target = RefTarget::Function("greet".to_owned());
        let span = Span::new(4, 9);
        let mut index = UseSiteIndex::default();
        index.extend_files(vec![
            (first.clone(), vec![(span, target.clone())]),
            (second.clone(), vec![(span, target.clone())]),
        ]);

        let sites = index.references(&target);
        assert_eq!(sites.len(), 2);
        assert!(
            sites
                .iter()
                .any(|(path, found)| path == &first && *found == span)
        );
        assert!(
            sites
                .iter()
                .any(|(path, found)| path == &second && *found == span)
        );

        index.remove_file(&first);
        assert_eq!(index.sites(&target), vec![(second.as_path(), span)]);
    }
}
