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
use syntax::ast::{
    BinOp, Expr, ExprKind, GlobalVar, Literal, QualifiedName, StaticTarget, Stmt, StmtKind,
};
use syntax::visit::{Visitor, any_stmt_in_body, walk_expr_in_scope, walk_stmt};

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
#[derive(Debug, Clone)]
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

/// Per-file, per-scope walk state that doesn't change as the walk
/// descends into nested scopes.
struct FileCtx<'a> {
    registry: &'a Registry,
    id: FileId,
}

/// Accumulates every site/include-site found across the whole walk,
/// threaded through by `&mut` rather than returned, since both
/// collections grow independently at arbitrary points in the recursion.
#[derive(Default)]
struct WalkOutput {
    sites: Vec<StagedSite>,
    include_sites: Vec<IncludeSite>,
}

impl Registry {
    /// Builds `self.global_variables` from scratch over `files` - see the
    /// module doc comment. Called at the end of `Registry::index_and_flatten`,
    /// `Project::finish_loading`, and the equivalent point in
    /// `Project::update_file`/`update_file_cancellable` - the same settle
    /// points `precompute_member_owners`/`precompute_ancestor_names`
    /// already run at in all three places, since every function/class this
    /// pass's own resolver depends on must already be indexed.
    pub(crate) fn infer_global_variables(&mut self, files: &[(FileId, &Path, &[Stmt])]) {
        self.global_sites.clear();
        self.infer_global_variables_of(files, &FxHashSet::default());
    }

    /// `infer_global_variables`, walking only the files in `stale` and
    /// those it hasn't walked before; every other file's sites are those
    /// of its last walk.
    pub(crate) fn infer_global_variables_of(
        &mut self,
        files: &[(FileId, &Path, &[Stmt])],
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
        // Each file's walk reads only the registry, so the files are
        // walked side by side; the sites come back in the files' order.
        let walked: Vec<(FileId, FileSites)> = files
            .par_iter()
            .filter(|(id, ..)| !registry.global_sites.contains_key(id))
            .map(|(id, _, stmts)| {
                let file_ctx = FileCtx { registry, id: *id };
                let mut out = WalkOutput::default();
                walk_top_level(stmts, &file_ctx, &mut out);
                (
                    *id,
                    FileSites {
                        sites: out.sites,
                        include_sites: out.include_sites,
                    },
                )
            })
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

        let global_variables = accumulators
            .into_iter()
            .map(|(name, acc)| (name, acc.finish()))
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
    files: &[(FileId, &Path, &[Stmt])],
    include_sites: &[(FileId, &IncludeSite)],
) -> FxHashSet<FileId> {
    // Each file's path with `/` separators, grouped by file name: an
    // include's target can only be a file of its last segment's name.
    let mut by_file_name: FxHashMap<String, Vec<(FileId, String)>> = FxHashMap::default();
    for (id, path, _) in files {
        let text = path.to_string_lossy().replace('\\', "/");
        let name = text.rsplit('/').next().unwrap_or_default().to_string();
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

    for (source, site) in include_sites {
        let Some(suffix) = &site.suffix else {
            continue;
        };
        if suffix.is_empty() {
            continue;
        }
        let suffix = suffix.trim_start_matches('/');
        let name = suffix.rsplit('/').next().unwrap_or_default();
        let matches: Vec<FileId> = by_file_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|(_, text)| text_ends_with_suffix(text, suffix))
            .map(|(id, _)| *id)
            .collect();
        let [target] = matches[..] else {
            continue;
        };
        has_any_incoming.insert(target);
        if site.top_level {
            top_level_edges.entry(*source).or_default().push(target);
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
fn resolve_include_target_suffix(expr: &Expr) -> Option<String> {
    syntax::stack::guard(|| match &expr.peeled().kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        ExprKind::Name(_) => Some(String::new()),
        ExprKind::Binary(BinOp::Concat, l, r) => {
            let l = resolve_include_target_suffix(l)?;
            let r = resolve_include_target_suffix(r)?;
            Some(l + &r)
        }
        _ => None,
    })
}

/// Walks a file's own top-level statements - every assignment found here
/// is staged as a top-level site (gated by the include-graph result
/// applied later), regardless of any `global $x;` (file top level is
/// already, potentially, the real global scope).
fn walk_top_level<'a>(stmts: &'a [Stmt], file_ctx: &FileCtx<'a>, out: &mut WalkOutput) {
    let empty = NameSet::default();
    walk_scope(stmts, file_ctx, true, &empty, out);
}

/// Walks one function/method/closure body - `global_names` is the
/// complete set of names any `StmtKind::Global` anywhere in this body
/// introduces (computed once, upfront, since real PHP's `global $x;`
/// binds `$x` for the rest of the whole body regardless of which branch
/// it textually appears in - no need to track this incrementally through
/// the walk with per-branch cloning). Every site found here is
/// unconditional (`gate: None`) - a function body's own scope isn't
/// subject to the include-graph check the way file top level is.
fn walk_function_body<'a>(stmts: &'a [Stmt], file_ctx: &FileCtx<'a>, out: &mut WalkOutput) {
    let global_names = collect_global_names(stmts);
    walk_scope(stmts, file_ctx, false, &global_names, out);
}

/// Every name a `global` statement in this body imports. A nested
/// function, class or closure needs its own `global`, so none is entered.
fn collect_global_names(stmts: &[Stmt]) -> NameSet {
    let mut out = NameSet::default();
    for stmt in stmts {
        any_stmt_in_body(stmt, |s| {
            if let StmtKind::Global(names) = &s.kind {
                for name in names.iter().filter_map(GlobalVar::name) {
                    out.insert(name.clone());
                }
            }
            false
        });
    }
    out
}

fn walk_scope<'a>(
    stmts: &'a [Stmt],
    file_ctx: &FileCtx<'a>,
    top_level: bool,
    global_names: &NameSet,
    out: &mut WalkOutput,
) {
    let mut scope = Scope {
        file_ctx,
        top_level,
        global_names,
        out,
    };
    for stmt in stmts {
        scope.visit_stmt(stmt);
    }
}

/// One scope's walk: a nested function, method or closure body is walked
/// as its own scope, and the include sites are recorded on the way.
struct Scope<'r, 'a> {
    file_ctx: &'r FileCtx<'a>,
    top_level: bool,
    global_names: &'r NameSet,
    out: &'r mut WalkOutput,
}

impl<'a> Visitor<'a> for Scope<'_, 'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                walk_function_body(f.body_stmts(), self.file_ctx, self.out);
                return;
            }
            StmtKind::ClassDecl(c) => {
                for method in &c.def.methods {
                    walk_function_body(method.function.body_stmts(), self.file_ctx, self.out);
                }
                return;
            }
            StmtKind::Expression(e) => {
                handle_top_expr_statement(
                    e,
                    self.file_ctx,
                    self.top_level,
                    self.global_names,
                    self.out,
                );
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match &expr.kind {
            ExprKind::Closure(closure) => {
                let syntax::ast::ClosureExpr { body, .. } = &**closure;
                walk_function_body(body, self.file_ctx, self.out);
                return;
            }
            ExprKind::NewAnonymousClass(class) => {
                for method in &class.def.methods {
                    walk_function_body(method.function.body_stmts(), self.file_ctx, self.out);
                }
                return;
            }
            ExprKind::Include(_, target) => {
                self.out.include_sites.push(IncludeSite {
                    top_level: self.top_level,
                    suffix: resolve_include_target_suffix(target),
                });
            }
            _ => {}
        }
        walk_expr_in_scope(self, expr);
    }
}

/// An expression statement's assignment to a variable that is a global
/// here (at file top level, or imported with `global`): records it as a
/// site of that global, gated on the file being included when at top
/// level. A compound assignment only widens the variable.
fn handle_top_expr_statement<'a>(
    e: &'a Expr,
    file_ctx: &FileCtx<'a>,
    top_level: bool,
    global_names: &NameSet,
    out: &mut WalkOutput,
) {
    let is_site = |name: &str| top_level || global_names.contains(name);
    let gate = || top_level.then_some(file_ctx.id);

    match &e.kind {
        ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
            handle_assignment_target(lhs, rhs, file_ctx, &is_site, &gate, out, false);
        }
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, rhs) => {
            handle_assignment_target(lhs, rhs, file_ctx, &is_site, &gate, out, false);
        }
        ExprKind::CompoundAssign(_, lhs, rhs) => {
            handle_assignment_target(lhs, rhs, file_ctx, &is_site, &gate, out, true);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_assignment_target<'a>(
    lhs: &Expr,
    rhs: &Expr,
    file_ctx: &FileCtx<'a>,
    is_site: &dyn Fn(&str) -> bool,
    gate: &dyn Fn() -> Option<FileId>,
    out: &mut WalkOutput,
    force_mixed: bool,
) {
    match &lhs.peeled().kind {
        ExprKind::Variable(name) if is_site(name) => {
            let contribution = if force_mixed {
                SiteContribution::WholeValue(Type::mixed())
            } else if let Some(fields) = array_merge_self_fields(name, rhs, file_ctx.registry) {
                SiteContribution::ShapeFields(fields)
            } else {
                SiteContribution::WholeValue(resolve_static_expr_type(rhs, file_ctx.registry))
            };
            out.sites.push(StagedSite {
                gate: gate(),
                name: name.clone(),
                contribution,
            });
        }
        ExprKind::Index(base, key_opt) => {
            if let Some(key) = key_opt
                && let ExprKind::Variable(name) = &base.peeled().kind
                && is_site(name)
                && let Some(key_str) = literal_string_key(key)
            {
                // `$name[<key>] = <rhs>;` - a direct, single-level write:
                // the field's own value is `<rhs>`'s real resolved type.
                let ty = if force_mixed {
                    Type::mixed()
                } else {
                    resolve_static_expr_type(rhs, file_ctx.registry)
                };
                out.sites.push(StagedSite {
                    gate: gate(),
                    name: name.clone(),
                    contribution: SiteContribution::ShapeField(key_str, ty),
                });
            } else if let Some((name, key_str)) = root_variable_and_first_key(lhs)
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
                out.sites.push(StagedSite {
                    gate: gate(),
                    name: name.clone(),
                    contribution: SiteContribution::ShapeField(key_str, Type::mixed()),
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
fn root_variable_and_first_key(expr: &Expr) -> Option<(&Name, String)> {
    syntax::stack::guard(|| {
        let ExprKind::Index(base, key) = &expr.peeled().kind else {
            return None;
        };
        match &base.peeled().kind {
            ExprKind::Variable(name) => key
                .as_deref()
                .and_then(literal_string_key)
                .map(|k| (name, k)),
            ExprKind::Index(..) => root_variable_and_first_key(base),
            _ => None,
        }
    })
}

/// `$name = array_merge($name, <rhs>);` - real PHP folds `<rhs>`'s own
/// keys into `$name`, so if `<rhs>` resolves to an array-shaped type,
/// merge its fields in rather than discarding them. `None` for anything
/// else (not this exact self-referencing shape, or `<rhs>` didn't resolve
/// to anything array-shaped) - the caller falls back to its own ordinary
/// whole-value handling in that case.
fn array_merge_self_fields(
    name: &Name,
    rhs: &Expr,
    registry: &Registry,
) -> Option<Vec<FieldContribution>> {
    let ExprKind::Call(callee, args) = &rhs.peeled().kind else {
        return None;
    };
    let ExprKind::Name(callee_name) = &callee.peeled().kind else {
        return None;
    };
    let is_array_merge = registry
        .called_function_name(callee_name)
        .eq_ignore_ascii_case("array_merge");
    if !is_array_merge || args.len() != 2 {
        return None;
    }
    let ExprKind::Variable(first_name) = &args[0].value.peeled().kind else {
        return None;
    };
    if first_name != name {
        return None;
    }
    let shape = resolve_static_expr_type(&args[1].value, registry)
        .as_array_shape()?
        .clone();
    Some(
        shape
            .into_fields()
            .into_iter()
            .map(|f| FieldContribution {
                key: f.key.to_string(),
                ty: f.ty,
            })
            .collect(),
    )
}

fn literal_string_key(expr: &Expr) -> Option<String> {
    match &expr.peeled().kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        ExprKind::Literal(Literal::Int(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Resolves a self-contained expression to a `Type`, using only registry
/// lookups - no `Checker`/`Locals`, neither exists yet at this point in
/// the pipeline.
fn resolve_static_expr_type(expr: &Expr, registry: &Registry) -> Type {
    syntax::stack::guard(|| {
        let expr = expr.peeled();
        match &expr.kind {
            ExprKind::New(callee, _args) => match &callee.peeled().kind {
                ExprKind::Name(name) => name
                    .resolved_class()
                    .map_or(Type::mixed(), |class| Type::object(class.into(), vec![])),
                _ => Type::mixed(),
            },
            ExprKind::ArrayLiteral(items) => {
                let mut fields = Vec::new();
                for item in items {
                    let Some(key_expr) = &item.key else {
                        continue;
                    };
                    let Some(key) = literal_string_key(key_expr) else {
                        continue;
                    };
                    let ty = resolve_static_expr_type(&item.value, registry);
                    fields.push((key, ty, false));
                }
                Type::array_shape(Shape::from_named(fields))
            }
            ExprKind::Call(callee, _args) => {
                let ExprKind::Name(name) = &callee.peeled().kind else {
                    return Type::mixed();
                };
                registry
                    .resolve_function(name)
                    .map(|(_, sig)| sig.return_type.clone())
                    .unwrap_or(Type::mixed())
            }
            ExprKind::StaticCall(target, method, _args) => target
                .as_name()
                .and_then(QualifiedName::resolved_class)
                .and_then(|class| registry.lookup_method(class, method))
                .map(|sig| sig.return_type.clone())
                .unwrap_or(Type::mixed()),
            ExprKind::ClassConstFetch(StaticTarget::Name(class), name) => class
                .resolved_class()
                .and_then(|class| registry.lookup_const(class, name))
                .map(|c| c.ty.clone())
                .unwrap_or(Type::mixed()),
            ExprKind::MethodCall(recv, method, _args, _) => {
                let recv_ty = resolve_static_expr_type(recv, registry);
                recv_ty
                    .as_object()
                    .and_then(|(class, _)| registry.lookup_method(class, method))
                    .map(|sig| sig.return_type.clone())
                    .unwrap_or(Type::mixed())
            }
            _ => infer_const_type(expr),
        }
    })
}
