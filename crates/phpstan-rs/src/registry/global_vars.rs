//! Corpus-wide `global $x;` variable type inference - see this project's
//! own plan doc for the full design. In short: real PHPStan can't do this
//! (its own `GlobalHandler` always resolves an unannotated `global $x;`
//! to `MixedType`, per-file, with no whole-program dataflow), but this
//! project already builds a whole-project `Registry` before checking any
//! file, so it can fold every real assignment site to a given global name
//! anywhere in the corpus into one inferred `Type`, replacing the
//! hardcoded `Mixed` `checker::stmt`'s own `StmtKind::Global` arm
//! otherwise falls back to.
//!
//! Two site categories fold into the same per-name accumulator: a file's
//! own top-level assignments (gated by the include-graph reachability
//! check below - only a file whose top level provably runs in real global
//! scope on some resolved path counts), and an assignment inside a
//! function/method/closure body *after* that body's own `global $x;`
//! introduces the name (never before, never in a sibling scope that
//! never declared it). Top-level sites can't be folded in immediately -
//! whether they count at all depends on the include graph, which itself
//! is only fully known after every file has been walked - so they're
//! staged, tagged by their own source file, and applied only once that
//! graph is resolved.

use std::path::Path;
use syntax::name::{NameMap, NameSet};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::registry::infer_const_type;
use crate::registry::summaries::ArgAt;
use syntax::ast::{
    BinOp, ClosurePayload, ExprId, ExprKind, ExprNode, GlobalVar, Literal, NodeArenas,
    QualifiedName, StaticTarget, StmtId, StmtKind, UnaryOp,
};
use syntax::visit::arena::{self, Visitor};

use types::{Name, Shape, Type};

use super::Registry;
use crate::file_id::FileId;

/// Mirrors `types::MAX_UNION_MEMBERS`'s own naming style -
/// a different, unrelated constant, not that one reused. Past this many
/// distinct keys, a global's accumulated shape degrades to a plain
/// `Array(Mixed)` rather than growing unboundedly.
const MAX_GLOBAL_SHAPE_FIELDS: usize = 64;

/// A single, self-contained per-name accumulator - either building up an
/// `ArrayShape` field-by-field, or folding a plain `Type` through
/// `Type::union` (the object/`Mixed` case), or both at once (a genuine
/// object/array conflict, resolved in `finish`).
#[derive(Default)]
struct GlobalAccumulator {
    shape_fields: Vec<(String, Type, bool)>,
    shape_overflowed: bool,
    other: Option<Type>,
}

impl GlobalAccumulator {
    // Declared types are joined while the class hierarchy is being built.
    #[allow(clippy::disallowed_methods)]
    fn add_shape_field(&mut self, key: String, ty: Type) {
        if let Some((_, existing, _)) = self.shape_fields.iter_mut().find(|(k, _, _)| *k == key) {
            *existing = Type::union(existing.clone(), ty);
            return;
        }
        if self.shape_fields.len() >= MAX_GLOBAL_SHAPE_FIELDS {
            self.shape_overflowed = true;
            return;
        }
        self.shape_fields.push((key, ty, true));
    }

    // Declared types are joined while the class hierarchy is being built.
    #[allow(clippy::disallowed_methods)]
    fn add_other(&mut self, ty: Type) {
        self.other = Some(match self.other.take() {
            Some(existing) => Type::union(existing, ty),
            None => ty,
        });
    }

    fn has_shape(&self) -> bool {
        !self.shape_fields.is_empty() || self.shape_overflowed
    }

    fn shape_type(&self) -> Type {
        if self.shape_overflowed {
            Type::array(Type::unknown_array_key(), Type::mixed())
        } else {
            Type::array_shape(Shape::from_named(self.shape_fields.clone()))
        }
    }

    // Declared types are joined while the class hierarchy is being built.
    #[allow(clippy::disallowed_methods)]
    fn finish(self) -> Type {
        let shape_type = self.has_shape().then(|| self.shape_type());
        match (shape_type, self.other) {
            (None, None) => Type::mixed(),
            (Some(shape), None) => shape,
            (None, Some(other)) => other,
            (Some(shape), Some(other)) => Type::union(shape, other),
        }
    }
}

/// One field-level contribution: `$name[<key>] = <value>`.
#[derive(Debug, Clone)]
struct FieldContribution {
    key: String,
    ty: Type,
}

/// What a single assignment site contributes - applied to a
/// `GlobalAccumulator` only once the include-graph gate (for a top-level
/// site) has been resolved.
#[derive(Debug, Clone)]
enum SiteContribution {
    WholeValue(Type),
    ShapeField(String, Type),
    ShapeFields(Vec<FieldContribution>),
}

impl SiteContribution {
    fn apply(self, acc: &mut GlobalAccumulator) {
        match self {
            SiteContribution::WholeValue(ty) => match ty.as_array_shape() {
                Some(shape) => {
                    for f in shape.fields() {
                        acc.add_shape_field(f.key.to_string(), f.ty.clone());
                    }
                }
                None => acc.add_other(ty),
            },
            SiteContribution::ShapeField(key, ty) => acc.add_shape_field(key, ty),
            SiteContribution::ShapeFields(fields) => {
                for f in fields {
                    acc.add_shape_field(f.key, f.ty);
                }
            }
        }
    }
}

/// A staged site: `gate` is `Some(file)` for a top-level site (only
/// applied once `file` is confirmed to be a genuine global-scope
/// candidate), `None` for a function-body site already gated by its own
/// enclosing `global $x;` at collection time (always applied).
#[derive(Debug, Clone)]
pub(in crate::registry) struct StagedSite {
    gate: Option<FileId>,
    name: Name,
    contribution: SiteContribution,
}

/// One raw `include`-shaped call site found anywhere in the corpus, with
/// the literal path suffix its target names, if it names one.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IncludeSite {
    top_level: bool,
    suffix: Option<String>,
}

/// What one file's walk found: its sites, and its include sites.
#[derive(Debug, Clone, Default)]
pub(in crate::registry) struct FileSites {
    sites: Vec<StagedSite>,
    include_sites: Vec<IncludeSite>,
}

/// An assigned value, as far as its type can be said before the registry
/// is looked at: what `resolve_value` turns into a `Type`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum StaticValue {
    /// A type that needs no declaration to tell.
    Known(Type),
    /// An array literal's literal-keyed entries.
    Array(Vec<(String, StaticValue)>),
    /// The return type of a function called by name.
    Call(QualifiedName),
    /// The return type of a method called on a named class.
    StaticCall(Option<Name>, Name),
    /// The type of a class constant.
    ClassConst(Option<Name>, Name),
    /// The return type of a method called on the value.
    MethodCall(Box<StaticValue>, Name),
}

/// What a site contributes, before the registry is looked at.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum ContributionFact {
    /// `mixed`, whole.
    Mixed,
    Whole(StaticValue),
    /// `$name = array_merge($name, <second>);`, which folds the keys of
    /// `second` in when it is an array shape, and is `whole` otherwise.
    ArrayMerge {
        callee: QualifiedName,
        second: StaticValue,
        whole: StaticValue,
    },
    Field(String, StaticValue),
    /// A field of unknown value.
    FieldMixed(String),
    /// The variable, or the field `key` of it, passed as an argument that
    /// the callee may take by reference.
    ByRefArgument {
        callee: ByRefCallee,
        at: ArgAt,
        key: Option<String>,
    },
}

/// What a call passing a variable by reference calls, as written.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum ByRefCallee {
    Function(QualifiedName),
    /// A method, a static method or a constructor, by its name.
    Method(Name),
    /// A callee that is computed.
    Unknown,
}

/// The names of the methods any loaded class declares with a parameter
/// taken by reference, found on first use.
struct ByRefMethods<'r> {
    registry: &'r Registry,
    names: std::sync::OnceLock<FxHashSet<String>>,
}

impl ByRefMethods<'_> {
    fn declared(&self, method: &str) -> bool {
        let names = self.names.get_or_init(|| {
            let mut names = FxHashSet::default();
            for (class, _) in self.registry.class_ids.iter() {
                for name in self.registry.method_names(class) {
                    if self
                        .registry
                        .lookup_own_method(class, name)
                        .is_some_and(|sig| sig.params.iter().any(|p| p.by_ref))
                    {
                        names.insert(name.to_ascii_lowercase());
                    }
                }
            }
            names
        });
        names.contains(&method.to_ascii_lowercase())
    }
}

/// Whether the function `sig` takes the argument passed `at` by reference.
fn takes_by_ref(sig: &crate::registry::FunctionReflection, at: &ArgAt) -> bool {
    match at {
        ArgAt::Position(at) => sig
            .params
            .get(*at as usize)
            .or_else(|| sig.params.last().filter(|p| p.variadic))
            .is_some_and(|p| p.by_ref),
        ArgAt::Named(name) => sig
            .params
            .iter()
            .find(|p| p.name.as_ref() == name.as_str())
            .or_else(|| sig.params.last().filter(|p| p.variadic))
            .is_some_and(|p| p.by_ref),
        ArgAt::Spread => sig.params.iter().any(|p| p.by_ref),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SiteFact {
    /// A file's own top level, whose include-graph gate is the file's.
    top_level: bool,
    name: Name,
    contribution: ContributionFact,
}

/// What a file assigns to the variables that are globals where they are
/// assigned, and the includes it makes, in the form the file wrote them:
/// made by one walk of the file, and resolved against the registry by
/// `infer_global_variables_of`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GlobalFacts {
    sites: Vec<SiteFact>,
    include_sites: Vec<IncludeSite>,
}

impl GlobalFacts {
    /// The facts of the statements of one file.
    pub fn of(arenas: &NodeArenas, stmts: &[StmtId]) -> GlobalFacts {
        let mut facts = GlobalFacts::default();
        walk_top_level(arenas, stmts, &mut facts);
        facts
    }

    /// The sites with what they assign, as `registry` says it is.
    fn resolve(&self, id: FileId, registry: &Registry, methods: &ByRefMethods) -> FileSites {
        FileSites {
            sites: self
                .sites
                .iter()
                .filter_map(|site| {
                    Some(StagedSite {
                        gate: site.top_level.then_some(id),
                        name: site.name.clone(),
                        contribution: site.contribution.resolve(registry, methods)?,
                    })
                })
                .collect(),
            include_sites: self.include_sites.clone(),
        }
    }
}

impl ContributionFact {
    fn resolve(&self, registry: &Registry, methods: &ByRefMethods) -> Option<SiteContribution> {
        Some(match self {
            ContributionFact::Mixed => SiteContribution::WholeValue(Type::mixed()),
            ContributionFact::Whole(value) => {
                SiteContribution::WholeValue(resolve_value(value, registry))
            }
            ContributionFact::ArrayMerge {
                callee,
                second,
                whole,
            } => {
                let shape = registry
                    .called_function_name(callee)
                    .eq_ignore_ascii_case("array_merge")
                    .then(|| resolve_value(second, registry))
                    .and_then(|ty| ty.as_array_shape().cloned());
                match shape {
                    Some(shape) => SiteContribution::ShapeFields(
                        shape
                            .into_fields()
                            .into_iter()
                            .map(|f| FieldContribution {
                                key: f.key.to_string(),
                                ty: f.ty,
                            })
                            .collect(),
                    ),
                    None => SiteContribution::WholeValue(resolve_value(whole, registry)),
                }
            }
            ContributionFact::Field(key, value) => {
                SiteContribution::ShapeField(key.clone(), resolve_value(value, registry))
            }
            ContributionFact::FieldMixed(key) => {
                SiteContribution::ShapeField(key.clone(), Type::mixed())
            }
            ContributionFact::ByRefArgument { callee, at, key } => {
                let by_ref = match callee {
                    ByRefCallee::Function(name) => registry
                        .resolve_function(name)
                        .is_some_and(|(_, sig)| takes_by_ref(sig, at)),
                    ByRefCallee::Method(name) => methods.declared(name),
                    ByRefCallee::Unknown => true,
                };
                if !by_ref {
                    return None;
                }
                match key {
                    Some(key) => SiteContribution::ShapeField(key.clone(), Type::mixed()),
                    None => SiteContribution::WholeValue(Type::mixed()),
                }
            }
        })
    }
}

impl Registry {
    /// Builds `self.global_variables` from scratch over `files` - see the
    /// module doc comment. Called at the end of `Registry::index_and_flatten`,
    /// `Project::finish_loading`, and the equivalent point in
    /// `Project::update_file`/`update_file_cancellable` - the same settle
    /// points `precompute_member_owners`/`precompute_ancestor_names`
    /// already run at in all three places, since every function/class this
    /// pass's own resolver depends on must already be indexed.
    pub(crate) fn infer_global_variables(&mut self, files: &[(FileId, &Path, &GlobalFacts)]) {
        self.global_sites.clear();
        self.infer_global_variables_of(files, &FxHashSet::default());
    }

    /// `infer_global_variables`, walking only the files in `stale` and
    /// those it hasn't walked before; every other file's sites are those
    /// of its last walk.
    pub(crate) fn infer_global_variables_of(
        &mut self,
        files: &[(FileId, &Path, &GlobalFacts)],
        stale: &FxHashSet<FileId>,
    ) {
        let loaded: FxHashSet<FileId> = files.iter().map(|(id, ..)| *id).collect();
        self.global_sites
            .retain(|id, _| loaded.contains(id) && !stale.contains(id));
        // Every read below goes through this reborrow, mirroring
        // `precompute_member_owners`'s own established shape
        // (`hierarchy.rs`): build the whole result into locals first,
        // assign into `self` only once the borrow is done.
        let registry: &Registry = self;
        let methods = ByRefMethods {
            registry,
            names: std::sync::OnceLock::new(),
        };
        // Each file's walk reads only the registry, so the files are
        // walked side by side; the sites come back in the files' order.
        let walked: Vec<(FileId, FileSites)> = files
            .par_iter()
            .filter(|(id, ..)| !registry.global_sites.contains_key(id))
            .map(|(id, _, facts)| (*id, facts.resolve(*id, registry, &methods)))
            .collect();
        self.global_sites.extend(walked);

        let include_sites: Vec<(FileId, &IncludeSite)> = files
            .iter()
            .flat_map(|(id, ..)| {
                self.global_sites[id]
                    .include_sites
                    .iter()
                    .map(move |site| (*id, site))
            })
            .collect();
        let global_scope_files = resolve_global_scope_files(files, &include_sites);

        let mut accumulators: NameMap<GlobalAccumulator> = NameMap::default();
        for (id, ..) in files {
            for site in &self.global_sites[id].sites {
                if let Some(gate) = &site.gate
                    && !global_scope_files.contains(gate)
                {
                    continue;
                }
                site.contribution
                    .clone()
                    .apply(accumulators.entry(site.name.clone()).or_default());
            }
        }

        // A superglobal is what PHP fills it with; the code that writes a
        // key of it does not make it narrower.
        let global_variables = accumulators
            .into_iter()
            .map(|(name, acc)| {
                let ty = if syntax::ast::is_superglobal(&name) {
                    crate::checker::unassigned_variable_type(&name)
                } else {
                    acc.finish()
                };
                (name, ty)
            })
            .collect();
        self.global_variables = global_variables;
    }
}

/// Resolves the include graph and returns the set of file paths whose own
/// top level genuinely runs in real global scope on at least one resolved
/// path - see the module doc comment. A file with no resolved incoming
/// edges at all (including every file this corpus's own dynamic includes
/// only ever reach) defaults to included, matching today's behavior; a
/// file whose only resolved incoming edges are all from inside a function
/// body is excluded.
fn resolve_global_scope_files(
    files: &[(FileId, &Path, &GlobalFacts)],
    include_sites: &[(FileId, &IncludeSite)],
) -> FxHashSet<FileId> {
    // Each file's path with `/` separators, grouped by file name: an
    // include's target can only be a file of its last segment's name.
    // Normalize paths independently, then index borrowed names and paths.
    // Keeping the basename as a slice avoids the second path-sized String
    // allocation the old index made for every file.
    let file_paths: Vec<(FileId, String)> = files
        .par_iter()
        .map(|(id, path, _)| (*id, path.to_string_lossy().replace('\\', "/")))
        .collect();
    let mut by_file_name: FxHashMap<&str, Vec<(FileId, &str)>> = FxHashMap::default();
    for (id, text) in &file_paths {
        let name = text.rsplit('/').next().unwrap_or_default();
        by_file_name.entry(name).or_default().push((*id, text));
    }

    let mut top_level_edges: FxHashMap<FileId, Vec<FileId>> = FxHashMap::default();
    // Every resolved incoming edge, top-level or not - a file targeted
    // *only* by function-scoped includes still has a real incoming edge
    // here, and must NOT default to "root" just because none of its
    // edges happen to be top-level (that was a real bug: it made a
    // file's own top level count as included whenever its only resolved
    // includers were all inside a function, exactly backwards from the
    // "excluded" outcome that shape is supposed to get).
    let mut has_any_incoming: FxHashSet<FileId> = FxHashSet::default();

    // Each include site is resolved independently against the filename
    // index. The resulting relation is ID-addressed before the serial graph
    // bookkeeping and reachability walk.
    let resolved_edges: Vec<(FileId, FileId, bool)> = include_sites
        .par_iter()
        .filter_map(|(source, site)| {
            let suffix = site.suffix.as_deref()?;
            if suffix.is_empty() {
                return None;
            }
            let suffix = suffix.trim_start_matches('/');
            let name = suffix.rsplit('/').next().unwrap_or_default();
            let mut matches = by_file_name
                .get(name)?
                .iter()
                .filter(|(_, text)| text_ends_with_suffix(text, suffix));
            let (target, _) = *matches.next()?;
            // An ambiguous suffix is not a resolved edge, matching the
            // previous exact-one-match rule without allocating a Vec.
            matches
                .next()
                .is_none()
                .then_some((*source, target, site.top_level))
        })
        .collect();

    for (source, target, top_level) in resolved_edges {
        has_any_incoming.insert(target);
        if top_level {
            top_level_edges.entry(source).or_default().push(target);
        }
    }

    // Roots: never a resolved include target anywhere at all (a real
    // entry point) - reached only through a function-scoped include is
    // excluded, not defaulted to included; reached only through an
    // unresolved/dynamic include has no edge recorded at all, so it still
    // defaults to included, same as today.
    let mut result: FxHashSet<FileId> = FxHashSet::default();
    let mut queue: Vec<FileId> = Vec::new();
    for (id, ..) in files {
        if !has_any_incoming.contains(id) && result.insert(*id) {
            queue.push(*id);
        }
    }
    while let Some(current) = queue.pop() {
        if let Some(targets) = top_level_edges.get(&current) {
            for target in targets {
                if result.insert(*target) {
                    queue.push(*target);
                }
            }
        }
    }
    result
}

fn text_ends_with_suffix(path: &str, suffix: &str) -> bool {
    path == suffix
        || path
            .strip_suffix(suffix)
            .is_some_and(|rest| rest.ends_with('/'))
}

/// Resolves an `include`/`require`-style target expression to the literal
/// path suffix it names, or `None` if it depends on anything not
/// statically known (a variable, an index, a call, ...). A bare `Name`
/// (a constant reference such as `PHPWG_ROOT_PATH`) contributes an empty
/// string and doesn't invalidate resolution - only the trailing literal
/// suffix needs to match a real file's path, never the constant's actual
/// value.
fn resolve_include_target_suffix(arenas: &NodeArenas, id: ExprId) -> Option<String> {
    let expr = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    syntax::stack::guard(|| match &expr.kind {
        ExprKind::Literal(Literal::String(s)) => {
            Some(String::from_utf8_lossy(arenas.string_bytes(s)?).into_owned())
        }
        ExprKind::Name(_) => Some(String::new()),
        ExprKind::Binary(BinOp::Concat, l, r) => {
            let l = resolve_include_target_suffix(arenas, *l)?;
            let r = resolve_include_target_suffix(arenas, *r)?;
            Some(l + &r)
        }
        _ => None,
    })
}

/// Walks a file's own top-level statements - every assignment found here
/// is staged as a top-level site (gated by the include-graph result
/// applied later), regardless of any `global $x;` (file top level is
/// already, potentially, the real global scope).
fn walk_top_level(arenas: &NodeArenas, stmts: &[StmtId], out: &mut GlobalFacts) {
    let empty = NameSet::default();
    walk_scope(arenas, stmts, true, &empty, out);
}

/// Walks one function/method/closure body - `global_names` is the
/// complete set of names any `StmtKind::Global` anywhere in this body
/// introduces (computed once, upfront, since real PHP's `global $x;`
/// binds `$x` for the rest of the whole body regardless of which branch
/// it textually appears in - no need to track this incrementally through
/// the walk with per-branch cloning). Every site found here is
/// unconditional - a function body's own scope isn't subject to the
/// include-graph check the way file top level is.
fn walk_function_body(arenas: &NodeArenas, stmts: &[StmtId], out: &mut GlobalFacts) {
    let global_names = collect_global_names(arenas, stmts);
    walk_scope(arenas, stmts, false, &global_names, out);
}

/// Every name a `global` statement in this body imports. A nested
/// function, class or closure needs its own `global`, so none is entered.
fn collect_global_names(arenas: &NodeArenas, stmts: &[StmtId]) -> NameSet {
    struct Globals(NameSet);
    impl Visitor for Globals {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            let Some(stmt) = arenas.stmt(id) else { return };
            if let StmtKind::Global(names) = &stmt.kind {
                for name in arenas
                    .global_vars(names.clone())
                    .iter()
                    .filter_map(GlobalVar::name)
                {
                    self.0.insert(name.clone());
                }
            }
            arena::walk_stmt(self, arenas, id);
        }

        fn visit_function_decl(&mut self, _: &NodeArenas, _: &syntax::ast::FunctionDecl) {}
        fn visit_class_def(&mut self, _: &NodeArenas, _: &syntax::ast::ClassDef) {}
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if arenas.expr(id).is_some_and(|expr| {
                matches!(
                    &expr.kind,
                    ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::NewAnonymousClass(_)
                )
            }) {
                return;
            }
            arena::walk_expr(self, arenas, id);
        }
    }
    let mut out = NameSet::default();
    let mut globals = Globals(std::mem::take(&mut out));
    arena::walk_stmts(&mut globals, arenas, stmts);
    std::mem::take(&mut globals.0)
}

fn walk_scope(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    top_level: bool,
    global_names: &NameSet,
    out: &mut GlobalFacts,
) {
    let mut scope = Scope {
        top_level,
        global_names,
        out,
    };
    arena::walk_stmts(&mut scope, arenas, stmts);
}

/// One scope's walk: a nested function, method or closure body is walked
/// as its own scope, and the include sites are recorded on the way.
struct Scope<'r> {
    top_level: bool,
    global_names: &'r NameSet,
    out: &'r mut GlobalFacts,
}

impl Visitor for Scope<'_> {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::FunctionDecl(id) => {
                if let Some(function) = arenas.function(*id) {
                    walk_function_body(arenas, function.body_stmts(arenas), self.out);
                }
                return;
            }
            StmtKind::ClassDecl(c) => {
                let c = arenas.class_decl(c.clone());
                if let Some(def) = arenas.class(c.def) {
                    for method in arenas.methods(def.methods.clone()) {
                        walk_function_body(arenas, method.function.body_stmts(arenas), self.out);
                    }
                }
                return;
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                by_ref,
                ..
            } => {
                for target in key.iter().chain(Some(value)) {
                    self.written(arenas, *target);
                }
                if *by_ref {
                    self.written(arenas, *subject);
                }
            }
            StmtKind::Try { catches, .. } => {
                for clause in arenas.catch_clauses(catches.clone()) {
                    if let Some(var) = &clause.var
                        && self.is_site(&var.name)
                    {
                        self.out.sites.push(SiteFact {
                            top_level: self.top_level,
                            name: var.name.clone(),
                            contribution: ContributionFact::Mixed,
                        });
                    }
                }
            }
            _ => {}
        }
        arena::walk_stmt(self, arenas, id);
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else { return };
        match &expr.kind {
            ExprKind::Closure(id) => {
                if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*id) {
                    walk_function_body(arenas, arenas.stmt_ids(&closure.body), self.out);
                }
                return;
            }
            ExprKind::NewAnonymousClass(id) => {
                let Some(class) = arenas.anonymous_class(*id) else {
                    return;
                };
                if let Some(def) = arenas.class(class.def) {
                    for method in arenas.methods(def.methods.clone()) {
                        walk_function_body(arenas, method.function.body_stmts(arenas), self.out);
                    }
                }
                return;
            }
            // An arrow function's body is a scope of its own, which takes
            // the variables around it by value.
            ExprKind::ArrowFn(_) => {
                let names = NameSet::default();
                let mut inner = Scope {
                    top_level: false,
                    global_names: &names,
                    out: &mut *self.out,
                };
                arena::walk_expr(&mut inner, arenas, id);
                return;
            }
            ExprKind::Include(_, target) => {
                self.out.include_sites.push(IncludeSite {
                    top_level: self.top_level,
                    suffix: resolve_include_target_suffix(arenas, *target),
                });
            }
            ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _) => {
                handle_write_expr(arenas, id, self.top_level, self.global_names, self.out);
                self.written_pattern(arenas, *lhs);
            }
            ExprKind::CompoundAssign(..) => {
                handle_write_expr(arenas, id, self.top_level, self.global_names, self.out);
            }
            ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                operand,
            ) => self.written(arenas, *operand),
            ExprKind::Call(callee, args) => {
                let callee = match arenas
                    .peeled(*callee)
                    .and_then(|id| arenas.expr(id))
                    .map(|callee| &callee.kind)
                {
                    Some(ExprKind::Name(name)) => arenas
                        .qualified_name(name)
                        .map_or(ByRefCallee::Unknown, |name| {
                            ByRefCallee::Function(name.clone())
                        }),
                    _ => ByRefCallee::Unknown,
                };
                self.passed(arenas, callee, args.clone());
            }
            ExprKind::MethodCall(_, method, args, _) | ExprKind::StaticCall(_, method, args) => {
                self.passed(
                    arenas,
                    ByRefCallee::Method(method.name.clone()),
                    args.clone(),
                );
            }
            ExprKind::New(_, args) => {
                self.passed(
                    arenas,
                    ByRefCallee::Method(Name::from("__construct")),
                    args.clone(),
                );
            }
            ExprKind::DynamicMethodCall(_, _, args, _)
            | ExprKind::DynamicStaticCall(_, _, args) => {
                self.passed(arenas, ByRefCallee::Unknown, args.clone());
            }
            _ => {}
        }
        arena::walk_expr(self, arenas, id);
    }
}

impl Scope<'_> {
    fn is_site(&self, name: &str) -> bool {
        self.top_level || self.global_names.contains(name)
    }

    /// Records that `target` is written a value not known: the variable
    /// whole, or the field of it a literal key names, or each target of a
    /// pattern.
    fn written(&mut self, arenas: &NodeArenas, target: ExprId) {
        let Some(expr) = arenas.peeled(target).and_then(|id| arenas.expr(id)) else {
            return;
        };
        match &expr.kind {
            ExprKind::Variable(name) if self.is_site(name) => self.out.sites.push(SiteFact {
                top_level: self.top_level,
                name: name.clone(),
                contribution: ContributionFact::Mixed,
            }),
            ExprKind::Index(..) => {
                if let Some((name, key)) = root_variable_and_first_key(arenas, expr.id)
                    && self.is_site(name)
                {
                    self.out.sites.push(SiteFact {
                        top_level: self.top_level,
                        name: name.clone(),
                        contribution: ContributionFact::FieldMixed(key),
                    });
                }
            }
            ExprKind::Destructure(items, _) => {
                for item in arenas.destructure_items(items) {
                    if let Some(target) = item.target {
                        self.written(arenas, target);
                    }
                }
            }
            _ => {}
        }
    }

    /// Records the targets of `lhs` when it is a pattern.
    fn written_pattern(&mut self, arenas: &NodeArenas, lhs: ExprId) {
        if arenas
            .peeled(lhs)
            .and_then(|id| arenas.expr(id))
            .is_some_and(|expr| matches!(expr.kind, ExprKind::Destructure(..)))
        {
            self.written(arenas, lhs);
        }
    }

    /// Records the variables a call passes as arguments the callee may
    /// take by reference.
    fn passed(&mut self, arenas: &NodeArenas, callee: ByRefCallee, args: syntax::ast::ArgRange) {
        let mut spread = false;
        for (at, arg) in arenas.args(args).iter().enumerate() {
            spread |= arg.spread;
            let at = match &arg.name {
                _ if spread => ArgAt::Spread,
                Some(name) => ArgAt::Named(name.name.clone()),
                None => ArgAt::Position(at as u32),
            };
            let Some(expr) = arenas.peeled(arg.value).and_then(|id| arenas.expr(id)) else {
                continue;
            };
            let (name, key) = match &expr.kind {
                ExprKind::Variable(name) => (name, None),
                ExprKind::Index(..) => match root_variable_and_first_key(arenas, expr.id) {
                    Some((name, key)) => (name, Some(key)),
                    None => continue,
                },
                _ => continue,
            };
            if self.is_site(name) {
                self.out.sites.push(SiteFact {
                    top_level: self.top_level,
                    name: name.clone(),
                    contribution: ContributionFact::ByRefArgument {
                        callee: callee.clone(),
                        at,
                        key,
                    },
                });
            }
        }
    }
}

/// An expression statement's assignment to a variable that is a global
/// here (at file top level, or imported with `global`): records it as a
/// site of that global, gated on the file being included when at top
/// level. A compound assignment only widens the variable.
fn handle_write_expr(
    arenas: &NodeArenas,
    id: ExprId,
    top_level: bool,
    global_names: &NameSet,
    out: &mut GlobalFacts,
) {
    let is_site = |name: &str| top_level || global_names.contains(name);
    let Some(e) = arenas.expr(id) else { return };

    match &e.kind {
        ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
            handle_assignment_target(arenas, *lhs, *rhs, &is_site, top_level, out, false);
        }
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, rhs) => {
            handle_assignment_target(arenas, *lhs, *rhs, &is_site, top_level, out, false);
        }
        ExprKind::CompoundAssign(_, lhs, rhs) => {
            handle_assignment_target(arenas, *lhs, *rhs, &is_site, top_level, out, true);
        }
        _ => {}
    }
}

fn handle_assignment_target(
    arenas: &NodeArenas,
    lhs: ExprId,
    rhs: ExprId,
    is_site: &dyn Fn(&str) -> bool,
    top_level: bool,
    out: &mut GlobalFacts,
    force_mixed: bool,
) {
    let Some(lhs) = arenas.peeled(lhs).and_then(|id| arenas.expr(id)) else {
        return;
    };
    let Some(rhs) = arenas.expr(rhs) else { return };
    match &lhs.kind {
        ExprKind::Variable(name) if is_site(name) => {
            let contribution = if force_mixed {
                ContributionFact::Mixed
            } else if let Some(merge) = array_merge_self(arenas, name, rhs.id) {
                merge
            } else {
                ContributionFact::Whole(static_value(arenas, rhs.id))
            };
            out.sites.push(SiteFact {
                top_level,
                name: name.clone(),
                contribution,
            });
        }
        ExprKind::Index(base, key_opt) => {
            if let Some(key) = key_opt
                && let Some(base) = arenas.peeled(*base).and_then(|id| arenas.expr(id))
                && let ExprKind::Variable(name) = &base.kind
                && is_site(name)
                && let Some(key_str) = literal_string_key(arenas, *key)
            {
                // `$name[<key>] = <rhs>;` - a direct, single-level write:
                // the field's own value is `<rhs>`'s real resolved type.
                let contribution = if force_mixed {
                    ContributionFact::FieldMixed(key_str)
                } else {
                    ContributionFact::Field(key_str, static_value(arenas, rhs.id))
                };
                out.sites.push(SiteFact {
                    top_level,
                    name: name.clone(),
                    contribution,
                });
            } else if let Some((name, key_str)) = root_variable_and_first_key(arenas, lhs.id)
                && is_site(name)
            {
                // `$name[<key>][...]...= <rhs>;` - a deeper, multi-level
                // write (real Piwigo shape:
                // `$switch_lang['language'][$user['language']]['lang_info']
                // = $lang_info;`). The precise nested value type isn't
                // tracked, but the outermost key's own *existence* is
                // real and must be recorded - missing it here is exactly
                // what produced a real false `offsetAccess.notFound`/
                // `isset.offset` on `$switch_lang['language']` reads
                // elsewhere before this arm existed (confirmed via a
                // piwigo16 corpus diff, not hypothesized).
                out.sites.push(SiteFact {
                    top_level,
                    name: name.clone(),
                    contribution: ContributionFact::FieldMixed(key_str),
                });
            }
        }
        _ => {}
    }
}

/// Walks a chain of `Index` wrappers down to the `Variable` it's rooted
/// at, returning that variable's name together with the *first* (i.e.
/// innermost, closest-to-the-variable) literal-string key in the chain -
/// `$name['language'][$dynamic]['lang_info']` resolves to `(name,
/// "language")`, not `"lang_info"`. The outermost level's own key can be
/// `None` (a real Piwigo shape: `$switch_lang['stack'][] = ...;`, an
/// append onto a nested key, `Index(Index(Variable, Some("stack")),
/// None)`) - only the *innermost* key, the one paired directly with the
/// bottomed-out `Variable`, has to actually be a literal. `None` if the
/// chain doesn't bottom out at a bare variable, or if that innermost key
/// isn't a literal (including a bare append directly on the variable
/// itself, `$name[] = ...` - nothing to report there, not a special
/// case, `key.as_ref()` on `None` already falls through to `None`).
fn root_variable_and_first_key(arenas: &NodeArenas, id: ExprId) -> Option<(&Name, String)> {
    let expr = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    syntax::stack::guard(|| {
        let ExprKind::Index(base, key) = &expr.kind else {
            return None;
        };
        let base = arenas.peeled(*base).and_then(|id| arenas.expr(id))?;
        match &base.kind {
            ExprKind::Variable(name) => key
                .map(|id| literal_string_key(arenas, id))
                .flatten()
                .map(|k| (name, k)),
            ExprKind::Index(..) => root_variable_and_first_key(arenas, base.id),
            _ => None,
        }
    })
}

/// `$name = array_merge($name, <rhs>);` - real PHP folds `<rhs>`'s own
/// keys into `$name`, so if `<rhs>` resolves to an array-shaped type,
/// merge its fields in rather than discarding them. `None` for anything
/// else (not this exact self-referencing shape) - the caller falls back to
/// its own ordinary whole-value handling in that case.
fn array_merge_self(arenas: &NodeArenas, name: &Name, rhs: ExprId) -> Option<ContributionFact> {
    let rhs = arenas.peeled(rhs).and_then(|id| arenas.expr(id))?;
    let ExprKind::Call(callee, args) = &rhs.kind else {
        return None;
    };
    let callee = arenas.peeled(*callee).and_then(|id| arenas.expr(id))?;
    let ExprKind::Name(callee_name) = &callee.kind else {
        return None;
    };
    let callee_name = arenas.qualified_name(callee_name)?;
    let args = arenas.args(args.clone());
    if args.len() != 2 {
        return None;
    }
    let first = arenas
        .peeled(args[0].value)
        .and_then(|id| arenas.expr(id))?;
    let ExprKind::Variable(first_name) = &first.kind else {
        return None;
    };
    if first_name != name {
        return None;
    }
    Some(ContributionFact::ArrayMerge {
        callee: callee_name.clone(),
        second: static_value(arenas, args[1].value),
        whole: static_value(arenas, rhs.id),
    })
}

fn literal_string_key(arenas: &NodeArenas, id: ExprId) -> Option<String> {
    let expr = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    match &expr.kind {
        ExprKind::Literal(Literal::String(s)) => {
            Some(String::from_utf8_lossy(arenas.string_bytes(s)?).into_owned())
        }
        ExprKind::Literal(Literal::Int(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// An assigned expression as far as its type can be said without the
/// registry: the types of calls, class constants and methods are looked up
/// when the facts are resolved.
fn static_value(arenas: &NodeArenas, id: ExprId) -> StaticValue {
    let Some(expr) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return StaticValue::Known(Type::mixed());
    };
    syntax::stack::guard(|| match &expr.kind {
        ExprKind::New(callee, _args) => StaticValue::Known(
            match arenas
                .peeled(*callee)
                .and_then(|id| arenas.expr(id))
                .map(|callee| &callee.kind)
            {
                Some(ExprKind::Name(name)) => arenas
                    .qualified_name(name)
                    .and_then(QualifiedName::resolved_class)
                    .map_or(Type::mixed(), |class| Type::object(class.into(), vec![])),
                _ => Type::mixed(),
            },
        ),
        ExprKind::ArrayLiteral(items) => {
            let mut fields = Vec::new();
            for item in arenas.array_items(items.clone()) {
                let Some(key_expr) = item.key else {
                    continue;
                };
                let Some(key) = literal_string_key(arenas, key_expr) else {
                    continue;
                };
                fields.push((key, static_value(arenas, item.value)));
            }
            StaticValue::Array(fields)
        }
        ExprKind::Call(callee, _args) => match arenas
            .peeled(*callee)
            .and_then(|id| arenas.expr(id))
            .map(|callee| &callee.kind)
        {
            Some(ExprKind::Name(name)) => arenas
                .qualified_name(name)
                .cloned()
                .map(StaticValue::Call)
                .unwrap_or_else(|| StaticValue::Known(Type::mixed())),
            _ => StaticValue::Known(Type::mixed()),
        },
        ExprKind::StaticCall(target, method, _args) => match target.as_name() {
            Some(name) => {
                StaticValue::StaticCall(name.resolved_class().map(Name::from), method.name.clone())
            }
            None => StaticValue::Known(Type::mixed()),
        },
        ExprKind::ClassConstFetch(StaticTarget::Name(class), name) => {
            StaticValue::ClassConst(class.resolved_class().map(Name::from), name.name.clone())
        }
        ExprKind::MethodCall(recv, method, _args, _) => {
            StaticValue::MethodCall(Box::new(static_value(arenas, *recv)), method.name.clone())
        }
        _ => StaticValue::Known(infer_const_type(arenas, expr.id)),
    })
}

/// The type of an assigned value, using only registry lookups - no
/// `Checker`/`Locals`, neither exists yet at this point in the pipeline.
fn resolve_value(value: &StaticValue, registry: &Registry) -> Type {
    match value {
        StaticValue::Known(ty) => ty.clone(),
        StaticValue::Array(items) => {
            let fields = items
                .iter()
                .map(|(key, value)| (key.clone(), resolve_value(value, registry), false))
                .collect::<Vec<_>>();
            Type::array_shape(Shape::from_named(fields))
        }
        StaticValue::Call(name) => registry
            .resolve_function(name)
            .map(|(_, sig)| sig.return_type.clone())
            .unwrap_or(Type::mixed()),
        StaticValue::StaticCall(class, method) => class
            .as_ref()
            .and_then(|class| registry.lookup_method(class, method))
            .map(|sig| sig.return_type.clone())
            .unwrap_or(Type::mixed()),
        StaticValue::ClassConst(class, name) => class
            .as_ref()
            .and_then(|class| registry.lookup_const_type(class, name))
            .cloned()
            .unwrap_or(Type::mixed()),
        StaticValue::MethodCall(recv, method) => resolve_value(recv, registry)
            .as_object()
            .and_then(|(class, _)| registry.lookup_method(class, method))
            .map(|sig| sig.return_type.clone())
            .unwrap_or(Type::mixed()),
    }
}

syntax::canonical_struct!(GlobalFacts {
    sites,
    include_sites,
});

syntax::canonical_struct!(SiteFact {
    top_level,
    name,
    contribution,
});

syntax::canonical_struct!(IncludeSite { top_level, suffix });

syntax::canonical_enum!(ContributionFact {
    Mixed,
    Whole(value),
    ArrayMerge {
        callee,
        second,
        whole,
    },
    Field(key, value),
    FieldMixed(key),
    ByRefArgument { callee, at, key },
});

syntax::canonical_enum!(ByRefCallee {
    Function(name),
    Method(name),
    Unknown,
});

syntax::canonical_enum!(StaticValue {
    Known(ty),
    Array(entries),
    Call(callee),
    StaticCall(class, method),
    ClassConst(class, name),
    MethodCall(base, method),
});

syntax::canonical_struct!(FileSites {
    sites,
    include_sites,
});

syntax::canonical_struct!(StagedSite {
    gate,
    name,
    contribution,
});

syntax::canonical_enum!(SiteContribution {
    WholeValue(ty),
    ShapeField(key, ty),
    ShapeFields(fields),
});

syntax::canonical_struct!(FieldContribution { key, ty });
