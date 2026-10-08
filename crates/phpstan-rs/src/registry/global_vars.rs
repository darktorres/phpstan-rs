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
use crate::registry::summaries::{ArgAt, writes_computed_variables};
use syntax::ast::{
    BinOp, ClosurePayload, ExprId, ExprKind, GlobalVar, Literal, NodeArenas, QualifiedName,
    StaticTarget, StmtId, StmtKind, UnaryOp,
};
use syntax::visit::arena::{self, Visitor};

use types::{Name, Shape, Type};

use std::sync::Arc;

use super::{Registry, ResolvedSnapshot};
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
#[derive(Debug, Default, Clone, PartialEq)]
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

    /// What both accumulated.
    fn merged(&self, other: &GlobalAccumulator) -> GlobalAccumulator {
        let mut both = GlobalAccumulator {
            shape_fields: self.shape_fields.clone(),
            shape_overflowed: self.shape_overflowed,
            other: self.other.clone(),
        };
        for (key, ty, _) in &other.shape_fields {
            both.add_shape_field(key.clone(), ty.clone());
        }
        both.shape_overflowed |= other.shape_overflowed;
        if let Some(ty) = &other.other {
            both.add_other(ty.clone());
        }
        both
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

/// What the global variables may hold where each file's top level runs: the
/// writes code that runs from anywhere may make, and, per global scope, those
/// of the top levels of the files that share it.
#[derive(Debug, Clone, Default)]
pub(in crate::registry) struct GlobalScopes {
    /// What the function bodies write after their own `global`.
    function_facts: NameMap<GlobalAccumulator>,
    /// Some function body writes globals whose names it computes.
    function_dynamic: bool,
    /// The scope each file's top level runs in, when it runs in a global one.
    scope_of: FxHashMap<FileId, u32>,
    scopes: Vec<GlobalScope>,
    /// What each include may write to the variables of the scope it is in,
    /// by the file it is in and its place there.
    include_writes: FxHashMap<(FileId, u32), IncludeWrites>,
}

/// The variables of the including scope an `include` may write: the ones
/// the included files' top levels write or bind, or any when the file is not
/// known or runs code that names variables by computing them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IncludeWrites {
    Any,
    Names(Vec<Name>),
}

impl IncludeWrites {
    /// Whether the variable `name` may be written.
    pub(crate) fn may_write(&self, name: &str) -> bool {
        match self {
            IncludeWrites::Any => true,
            IncludeWrites::Names(names) => names.iter().any(|written| **written == *name),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct GlobalScope {
    /// Some top-level include names no single file: the scope may be
    /// written by any file's top level.
    open: bool,
    /// A top level in it writes variables whose names it computes.
    dynamic: bool,
    /// What the top levels of the files that share it write.
    facts: NameMap<GlobalAccumulator>,
}

impl GlobalScopes {
    /// The global scopes by their lowest file, with the files in each.
    fn by_first_file(&self) -> FxHashMap<FileId, (Vec<FileId>, &GlobalScope)> {
        let mut members: FxHashMap<u32, Vec<FileId>> = FxHashMap::default();
        for (file, scope) in &self.scope_of {
            members.entry(*scope).or_default().push(*file);
        }
        members
            .into_iter()
            .map(|(scope, mut files)| {
                files.sort_unstable();
                (files[0], (files, &self.scopes[scope as usize]))
            })
            .collect()
    }

    /// The names whose fact in some scope differs from `before`'s:
    /// `everywhere` are those the whole program has one for, in either.
    pub(in crate::registry) fn changed_names(
        &self,
        before: &GlobalScopes,
        everywhere: &[&NameMap<Type>],
    ) -> Vec<Name> {
        let mut out: FxHashSet<Name> = FxHashSet::default();
        let all = |out: &mut FxHashSet<Name>| {
            for names in everywhere {
                out.extend(names.keys().cloned());
            }
        };
        for name in self
            .function_facts
            .keys()
            .chain(before.function_facts.keys())
        {
            if self.function_facts.get(name) != before.function_facts.get(name) {
                out.insert(name.clone());
            }
        }
        if self.function_dynamic != before.function_dynamic {
            all(&mut out);
        }
        let (now, was) = (self.by_first_file(), before.by_first_file());
        for first in now.keys().chain(was.keys()) {
            match (was.get(first), now.get(first)) {
                (Some((a_files, a)), Some((b_files, b)))
                    if a_files == b_files && a.open == b.open && a.dynamic == b.dynamic =>
                {
                    for name in a.facts.keys().chain(b.facts.keys()) {
                        if a.facts.get(name) != b.facts.get(name) {
                            out.insert(name.clone());
                        }
                    }
                }
                (a, b) => {
                    for scope in [a, b].into_iter().flatten().map(|(_, scope)| scope) {
                        if scope.open || scope.dynamic {
                            all(&mut out);
                        }
                        out.extend(scope.facts.keys().cloned());
                    }
                    if let (Some((_, a)), Some((_, b))) = (a, b)
                        && (a.open != b.open || a.dynamic != b.dynamic)
                    {
                        all(&mut out);
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    /// What the global variable `name` may hold in the scope the top level
    /// of `file` runs in, after code that is not known ran: the type of
    /// everything that may write it there. `None` when `file` is not known
    /// to run in a global scope of its own or `name` is written nowhere.
    fn fact(&self, file: FileId, name: &str, everywhere: &NameMap<Type>) -> Option<Type> {
        let scope = &self.scopes[*self.scope_of.get(&file)? as usize];
        if scope.open || scope.dynamic || syntax::ast::is_superglobal(name) {
            return everywhere.get(name).cloned();
        }
        let (own, anywhere) = (scope.facts.get(name), self.function_facts.get(name));
        Some(match (own, anywhere) {
            (None, None) => return None,
            (Some(only), None) | (None, Some(only)) => only.clone().finish(),
            (Some(own), Some(anywhere)) => own.merged(anywhere).finish(),
        })
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
    /// The include expression, by its id in the file.
    at: u32,
}

/// What one file's walk found: its sites, and its include sites.
#[derive(Debug, Clone, Default)]
pub(in crate::registry) struct FileSites {
    sites: Vec<StagedSite>,
    include_sites: Vec<IncludeSite>,
    dynamic_top_level: bool,
    dynamic_function: bool,
    /// The variables the file's top level writes or binds by name, sorted.
    top_level_names: Vec<Name>,
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
    /// The file's top level writes variables whose names it computes
    /// (`extract`, `$$name`).
    #[serde(default)]
    dynamic_top_level: bool,
    /// A function body writes the global `$GLOBALS` names with a computed
    /// key, or imports one with `global $$n`: any global may be written
    /// from a function.
    #[serde(default)]
    dynamic_function: bool,
    /// The variables the file's top level binds with `global` or `static`.
    #[serde(default)]
    bound_top_level: Vec<Name>,
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
            dynamic_top_level: self.dynamic_top_level,
            dynamic_function: self.dynamic_function,
            top_level_names: {
                let mut names: Vec<Name> = self
                    .sites
                    .iter()
                    .filter(|site| site.top_level)
                    .map(|site| site.name.clone())
                    .chain(self.bound_top_level.iter().cloned())
                    .collect();
                names.sort_unstable();
                names.dedup();
                names
            },
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
        let graph = resolve_global_scope_files(files, &include_sites);

        let mut accumulators: NameMap<GlobalAccumulator> = NameMap::default();
        let mut scopes = GlobalScopes {
            scope_of: graph.component.clone(),
            scopes: (0..graph.component.values().map(|n| n + 1).max().unwrap_or(0))
                .map(|n| GlobalScope {
                    open: graph.open.contains(&n),
                    dynamic: false,
                    facts: NameMap::default(),
                })
                .collect(),
            ..GlobalScopes::default()
        };
        scopes.include_writes = self.include_writes_of(&graph);
        for (id, ..) in files {
            scopes.function_dynamic |= self.global_sites[id].dynamic_function;
            if self.global_sites[id].dynamic_top_level
                && graph.global_scope.contains(id)
                && let Some(scope) = graph.component.get(id)
            {
                scopes.scopes[*scope as usize].dynamic = true;
            }
            for site in &self.global_sites[id].sites {
                let own = match &site.gate {
                    Some(gate) if !graph.global_scope.contains(gate) => continue,
                    Some(gate) => Some(&mut scopes.scopes[graph.component[gate] as usize].facts),
                    None => None,
                };
                let scoped = match own {
                    Some(facts) => facts,
                    None => &mut scopes.function_facts,
                };
                site.contribution
                    .clone()
                    .apply(scoped.entry(site.name.clone()).or_default());
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
        self.global_scopes = Arc::new(scopes);
    }

    /// What each include of the loaded files may write to the variables of
    /// the scope it is in: what the top levels of the files it reaches write,
    /// through the includes those make at their top levels.
    fn include_writes_of(&self, graph: &IncludeGraph) -> FxHashMap<(FileId, u32), IncludeWrites> {
        let mut reached: FxHashMap<FileId, IncludeWrites> = FxHashMap::default();
        let mut out: FxHashMap<(FileId, u32), IncludeWrites> = FxHashMap::default();
        for (&(source, at), target) in &graph.targets {
            let writes = match target {
                None => IncludeWrites::Any,
                Some(target) => reached
                    .entry(*target)
                    .or_insert_with(|| self.top_level_writes_from(*target, graph))
                    .clone(),
            };
            out.insert((source, at), writes);
        }
        out
    }

    /// What the top level of `start` writes, and of every file it includes
    /// at its top level, and so on.
    fn top_level_writes_from(&self, start: FileId, graph: &IncludeGraph) -> IncludeWrites {
        let mut names: Vec<Name> = Vec::new();
        let mut seen: FxHashSet<FileId> = FxHashSet::default();
        let mut queue = vec![start];
        while let Some(file) = queue.pop() {
            if !seen.insert(file) {
                continue;
            }
            let Some(sites) = self.global_sites.get(&file) else {
                return IncludeWrites::Any;
            };
            if sites.dynamic_top_level {
                return IncludeWrites::Any;
            }
            names.extend(sites.top_level_names.iter().cloned());
            for include in sites
                .include_sites
                .iter()
                .filter(|include| include.top_level)
            {
                match graph.targets.get(&(file, include.at)) {
                    Some(Some(target)) => queue.push(*target),
                    _ => return IncludeWrites::Any,
                }
            }
        }
        names.sort_unstable();
        names.dedup();
        IncludeWrites::Names(names)
    }

    /// What the include at `at` in `file` may write to the variables of the
    /// scope it is in.
    pub(crate) fn include_writes(&self, file: Option<FileId>, at: u32) -> &IncludeWrites {
        static ANY: IncludeWrites = IncludeWrites::Any;
        file.and_then(|file| self.global_scopes.include_writes.get(&(file, at)))
            .unwrap_or(&ANY)
    }

    /// The files with an include that may write other variables than it did
    /// in `before`.
    pub(crate) fn files_with_changed_include_writes(
        &self,
        before: &ResolvedSnapshot,
    ) -> Vec<FileId> {
        let (now, was) = (
            &self.global_scopes.include_writes,
            &before.global_scopes.include_writes,
        );
        let mut files: Vec<FileId> = now
            .iter()
            .filter(|(key, writes)| was.get(key) != Some(writes))
            .map(|((file, _), _)| *file)
            .chain(
                was.keys()
                    .filter(|key| !now.contains_key(key))
                    .map(|(file, _)| *file),
            )
            .collect();
        files.sort_unstable();
        files.dedup();
        files
    }

    /// The names whose fact in some global scope differs from what `before`
    /// had.
    pub(crate) fn changed_global_scope_names(&self, before: &ResolvedSnapshot) -> Vec<Name> {
        self.global_scopes.changed_names(
            &before.global_scopes,
            &[&before.global_variables, &self.global_variables],
        )
    }

    /// What the functions that bind the global `name` may write to it, when
    /// any does.
    pub(crate) fn global_written_by_functions(&self, name: &str) -> Option<Type> {
        self.global_scopes
            .function_facts
            .get(name)
            .map(|written| written.clone().finish())
    }

    /// Whether code of the loaded program that is not known may write the
    /// global `name`: a function body binds it or writes it by name, or
    /// writes a global it computes the name of.
    pub(crate) fn code_may_write_global(&self, name: &str) -> bool {
        syntax::ast::is_superglobal(name)
            || self.global_scopes.function_dynamic
            || self.global_scopes.function_facts.contains_key(name)
    }

    /// What the global variable `name` may hold where the top level of
    /// `file` runs, after code that is not known ran: what every site that
    /// may write it there says, or the whole program's when `file`'s global
    /// scope is not known.
    pub(crate) fn global_in_scope_of(&self, file: Option<FileId>, name: &str) -> Option<Type> {
        match file {
            Some(file) if self.global_scopes.scope_of.contains_key(&file) => {
                self.global_scopes.fact(file, name, &self.global_variables)
            }
            _ => self.global_variables.get(name).cloned(),
        }
    }
}

/// What the include graph says of the files' top levels.
struct IncludeGraph {
    /// The files whose own top level runs in the real global scope.
    global_scope: FxHashSet<FileId>,
    /// The global scope each of those shares, numbered: files joined by a
    /// top-level include run in one.
    component: FxHashMap<FileId, u32>,
    /// The scopes some top-level include of which names no single file, so
    /// any file's top level may run in them.
    open: FxHashSet<u32>,
    /// The file each include names, by the file it is in and its place
    /// there; `None` when it names no single loaded file.
    targets: FxHashMap<(FileId, u32), Option<FileId>>,
}

/// Resolves the include graph: the files whose own
/// top level genuinely runs in real global scope on at least one resolved
/// path, and which of them share a scope - see the module doc comment. A file with no resolved incoming
/// edges at all (including every file this corpus's own dynamic includes
/// only ever reach) defaults to included, matching today's behavior; a
/// file whose only resolved incoming edges are all from inside a function
/// body is excluded.
fn resolve_global_scope_files(
    files: &[(FileId, &Path, &GlobalFacts)],
    include_sites: &[(FileId, &IncludeSite)],
) -> IncludeGraph {
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
    // The files with a top-level include that names no single file.
    let mut unresolved: FxHashSet<FileId> = FxHashSet::default();

    // Each include site is resolved independently against the filename
    // index. The resulting relation is ID-addressed before the serial graph
    // bookkeeping and reachability walk.
    let resolved_edges: Vec<(FileId, u32, Option<FileId>, bool)> = include_sites
        .par_iter()
        .map(|(source, site)| {
            let target = site.suffix.as_deref().and_then(|suffix| {
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
                matches.next().is_none().then_some(target)
            });
            (*source, site.at, target, site.top_level)
        })
        .collect();

    let mut targets: FxHashMap<(FileId, u32), Option<FileId>> = FxHashMap::default();
    for (source, at, target, top_level) in resolved_edges {
        targets.insert((source, at), target);
        let Some(target) = target else {
            if top_level {
                unresolved.insert(source);
            }
            continue;
        };
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
    let mut global_scope: FxHashSet<FileId> = FxHashSet::default();
    let mut queue: Vec<FileId> = Vec::new();
    for (id, ..) in files {
        if !has_any_incoming.contains(id) && global_scope.insert(*id) {
            queue.push(*id);
        }
    }
    while let Some(current) = queue.pop() {
        if let Some(targets) = top_level_edges.get(&current) {
            for target in targets {
                if global_scope.insert(*target) {
                    queue.push(*target);
                }
            }
        }
    }

    // The files that share one global scope: those joined by a top-level
    // include, whichever file includes which.
    let mut parents: FxHashMap<FileId, FileId> = global_scope.iter().map(|&id| (id, id)).collect();
    fn root(parents: &mut FxHashMap<FileId, FileId>, id: FileId) -> FileId {
        let mut at = id;
        while parents[&at] != at {
            let up = parents[&parents[&at]];
            parents.insert(at, up);
            at = up;
        }
        at
    }
    let mut edges: Vec<(&FileId, &FileId)> = top_level_edges
        .iter()
        .flat_map(|(source, targets)| targets.iter().map(move |target| (source, target)))
        .filter(|(source, target)| global_scope.contains(*source) && global_scope.contains(*target))
        .collect();
    edges.sort_unstable();
    for (source, target) in edges {
        let (a, b) = (root(&mut parents, *source), root(&mut parents, *target));
        if a != b {
            parents.insert(a.max(b), a.min(b));
        }
    }
    let mut ids: FxHashMap<FileId, u32> = FxHashMap::default();
    let mut component: FxHashMap<FileId, u32> = FxHashMap::default();
    let mut ordered: Vec<FileId> = global_scope.iter().copied().collect();
    ordered.sort_unstable();
    for id in ordered {
        let top = root(&mut parents, id);
        let next = ids.len() as u32;
        component.insert(id, *ids.entry(top).or_insert(next));
    }
    let open = unresolved
        .iter()
        .filter_map(|id| component.get(id).copied())
        .collect();
    IncludeGraph {
        global_scope,
        component,
        open,
        targets,
    }
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
            StmtKind::Unset(targets) => {
                for target in arenas.expr_ids(targets) {
                    self.unset(arenas, *target);
                }
            }
            StmtKind::StaticVar(vars) if self.top_level => {
                for var in arenas.static_vars(vars.clone()) {
                    self.out.bound_top_level.push(var.name.name.clone());
                }
            }
            StmtKind::Global(vars) => {
                if self.top_level {
                    for name in arenas
                        .global_vars(vars.clone())
                        .iter()
                        .filter_map(GlobalVar::name)
                    {
                        self.out.bound_top_level.push(name.clone());
                    }
                }
                if !self.top_level
                    && arenas
                        .global_vars(vars.clone())
                        .iter()
                        .any(|var| matches!(var, GlobalVar::Dynamic(_)))
                {
                    self.out.dynamic_function = true;
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
                    at: u32::from(id.into_raw()),
                });
            }
            ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
                handle_write_expr(arenas, id, self.top_level, self.global_names, self.out);
                self.written_pattern(arenas, *lhs);
                self.globals_written(
                    arenas,
                    *lhs,
                    Some(ContributionFact::Whole(static_value(arenas, *rhs))),
                );
                // A reference to a global writes it wherever it is used.
                if matches!(&expr.kind, ExprKind::AssignRef(..)) {
                    self.written(arenas, *rhs);
                }
            }
            ExprKind::CompoundAssign(_, lhs, _) => {
                handle_write_expr(arenas, id, self.top_level, self.global_names, self.out);
                self.written_pattern(arenas, *lhs);
                self.globals_written(arenas, *lhs, None);
            }
            ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                operand,
            ) => self.written(arenas, *operand),
            ExprKind::Call(callee, args) => {
                if writes_computed_variables(arenas, id) {
                    self.computed_write();
                }
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
        if self.globals_written(arenas, target, None) {
            return;
        }
        self.written_pattern(arenas, target);
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

    /// Records the targets of `lhs` when it is a pattern, and the write
    /// when its variable is named by a computed expression.
    fn written_pattern(&mut self, arenas: &NodeArenas, lhs: ExprId) {
        let Some(expr) = arenas.peeled(lhs).and_then(|id| arenas.expr(id)) else {
            return;
        };
        if matches!(expr.kind, ExprKind::Destructure(..)) {
            for item in match &expr.kind {
                ExprKind::Destructure(items, _) => arenas.destructure_items(items),
                _ => &[],
            } {
                if let Some(target) = item.target {
                    self.written(arenas, target);
                }
            }
        } else if names_computed_variable(arenas, expr.id) {
            self.computed_write();
        } else if names_computed_global(arenas, expr.id) {
            self.computed_global_write();
        }
    }

    /// Records that code in this scope writes a global whose name it
    /// computes: any global of the scope at a file's top level, any global
    /// from a body.
    fn computed_global_write(&mut self) {
        if self.top_level {
            self.out.dynamic_top_level = true;
        } else {
            self.out.dynamic_function = true;
        }
    }

    /// Records that code in this scope writes variables whose names it
    /// computes: any variable of the scope at a file's top level, the ones
    /// it imports with `global` in a body.
    fn computed_write(&mut self) {
        if self.top_level {
            self.out.dynamic_top_level = true;
            return;
        }
        for name in self.global_names {
            self.out.sites.push(SiteFact {
                top_level: false,
                name: name.clone(),
                contribution: ContributionFact::Mixed,
            });
        }
    }

    /// Records a write to an element of `$GLOBALS` named by a literal:
    /// `value` when it is the global itself, `mixed` otherwise. Whether
    /// `target` is such an element.
    fn globals_written(
        &mut self,
        arenas: &NodeArenas,
        target: ExprId,
        value: Option<ContributionFact>,
    ) -> bool {
        let Some((name, under)) = globals_element(arenas, target) else {
            return false;
        };
        let contribution = match under {
            Under::Whole => value.unwrap_or(ContributionFact::Mixed),
            Under::Key(key) => ContributionFact::FieldMixed(key),
            Under::Other => return true,
        };
        self.out.sites.push(SiteFact {
            top_level: self.top_level,
            name,
            contribution,
        });
        true
    }

    /// Records what `unset($target)` does to a global: the variable is
    /// `null` after it at a file's top level, an element is gone from it
    /// anywhere. Unsetting the name a function imported only unbinds it
    /// there.
    fn unset(&mut self, arenas: &NodeArenas, target: ExprId) {
        let null = ContributionFact::Whole(StaticValue::Known(Type::null()));
        if self.globals_written(arenas, target, Some(null.clone())) {
            return;
        }
        let Some(expr) = arenas.peeled(target).and_then(|id| arenas.expr(id)) else {
            return;
        };
        match &expr.kind {
            ExprKind::Variable(name) if self.top_level => self.out.sites.push(SiteFact {
                top_level: true,
                name: name.clone(),
                contribution: null,
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
                } else if self.top_level && names_computed_variable(arenas, expr.id) {
                    self.computed_write();
                } else if names_computed_global(arenas, expr.id) {
                    self.computed_global_write();
                }
            }
            ExprKind::DynamicVariable(_) if self.top_level => self.computed_write(),
            _ => {}
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
            if names_computed_variable(arenas, expr.id) {
                self.computed_write();
                continue;
            }
            if names_computed_global(arenas, expr.id) {
                self.computed_global_write();
                continue;
            }
            if self.globals_written(arenas, expr.id, None) {
                continue;
            }
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

/// What of a global a write through `$GLOBALS['name']` reaches.
enum Under {
    /// The global itself.
    Whole,
    /// The element of it a literal key names.
    Key(String),
    /// Some other element.
    Other,
}

/// The global `id` is an element of, when it is `$GLOBALS['name']` or
/// something inside it, `name` a literal.
fn globals_element(arenas: &NodeArenas, id: ExprId) -> Option<(Name, Under)> {
    let expr = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    syntax::stack::guard(|| {
        let ExprKind::Index(base, key) = &expr.kind else {
            return None;
        };
        let base = arenas.peeled(*base).and_then(|id| arenas.expr(id))?;
        match &base.kind {
            ExprKind::Variable(name) if name.as_str() == "GLOBALS" => {
                let key = key.and_then(|id| literal_string_key(arenas, id))?;
                Some((Name::from(key), Under::Whole))
            }
            ExprKind::Index(..) => {
                let (name, under) = globals_element(arenas, base.id)?;
                let under = match under {
                    Under::Whole => key
                        .and_then(|id| literal_string_key(arenas, id))
                        .map_or(Under::Other, Under::Key),
                    deeper => deeper,
                };
                Some((name, under))
            }
            _ => None,
        }
    })
}

/// Whether `id` is an element of `$GLOBALS` named by a computed key, or
/// something inside one.
fn names_computed_global(arenas: &NodeArenas, id: ExprId) -> bool {
    let mut current = id;
    loop {
        let Some(expr) = arenas.peeled(current).and_then(|id| arenas.expr(id)) else {
            return false;
        };
        let ExprKind::Index(base, key) = &expr.kind else {
            return false;
        };
        let is_globals = arenas
            .peeled(*base)
            .and_then(|id| arenas.expr(id))
            .is_some_and(
                |base| matches!(&base.kind, ExprKind::Variable(name) if name.as_str() == "GLOBALS"),
            );
        if is_globals {
            return key.is_none_or(|key| literal_string_key(arenas, key).is_none());
        }
        current = *base;
    }
}

/// Whether `id` is a variable named by a computed expression, or an element
/// of one.
fn names_computed_variable(arenas: &NodeArenas, id: ExprId) -> bool {
    let mut current = id;
    loop {
        let Some(expr) = arenas.peeled(current).and_then(|id| arenas.expr(id)) else {
            return false;
        };
        match &expr.kind {
            ExprKind::DynamicVariable(_) => return true,
            ExprKind::Index(base, _) => current = *base,
            _ => return false,
        }
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
    dynamic_top_level,
    dynamic_function,
    bound_top_level,
});

syntax::canonical_struct!(SiteFact {
    top_level,
    name,
    contribution,
});

syntax::canonical_struct!(IncludeSite {
    top_level,
    suffix,
    at
});

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
    dynamic_top_level,
    dynamic_function,
    top_level_names,
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

syntax::canonical_struct!(GlobalAccumulator {
    shape_fields,
    shape_overflowed,
    other,
});

syntax::canonical_struct!(GlobalScope {
    open,
    dynamic,
    facts
});

syntax::canonical_struct!(GlobalScopes {
    function_facts,
    function_dynamic,
    scope_of,
    scopes,
    include_writes,
});
syntax::canonical_enum!(IncludeWrites {
    Any,
    Names(names),
});
