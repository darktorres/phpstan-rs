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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::{BinOp, Expr, ExprKind, Literal, StaticTarget, Stmt, StmtKind};
use crate::ast_walk::{expr_children, stmt_children, stmt_own_exprs};
use crate::checker::union_types;
use crate::registry::{NameContext, infer_const_type};
use crate::types::Type;

use super::Registry;
use crate::ast::unwrap_grouping;

/// Mirrors `checker::type_union::MAX_UNION_MEMBERS`'s own naming style -
/// a different, unrelated constant, not that one reused. Past this many
/// distinct keys, a global's accumulated shape degrades to a plain
/// `Array(Mixed)` rather than growing unboundedly.
const MAX_GLOBAL_SHAPE_FIELDS: usize = 64;

/// A single, self-contained per-name accumulator - either building up an
/// `ArrayShape` field-by-field, or folding a plain `Type` through
/// `union_types` (the object/`Mixed` case), or both at once (a genuine
/// object/array conflict, resolved in `finish`).
#[derive(Default)]
struct GlobalAccumulator {
    shape_fields: Vec<(String, Type, bool)>,
    shape_overflowed: bool,
    other: Option<Type>,
}

impl GlobalAccumulator {
    fn add_shape_field(&mut self, key: String, ty: Type) {
        if let Some((_, existing, _)) = self.shape_fields.iter_mut().find(|(k, _, _)| *k == key) {
            *existing = union_types(existing.clone(), ty);
            return;
        }
        if self.shape_fields.len() >= MAX_GLOBAL_SHAPE_FIELDS {
            self.shape_overflowed = true;
            return;
        }
        self.shape_fields.push((key, ty, true));
    }

    fn add_other(&mut self, ty: Type) {
        self.other = Some(match self.other.take() {
            Some(existing) => union_types(existing, ty),
            None => ty,
        });
    }

    fn has_shape(&self) -> bool {
        !self.shape_fields.is_empty() || self.shape_overflowed
    }

    fn shape_type(&self) -> Type {
        if self.shape_overflowed {
            Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(Type::Mixed(false)),
            )
        } else {
            Type::ArrayShape(self.shape_fields.clone())
        }
    }

    fn finish(self) -> Type {
        let shape_type = self.has_shape().then(|| self.shape_type());
        match (shape_type, self.other) {
            (None, None) => Type::Mixed(false),
            (Some(shape), None) => shape,
            (None, Some(other)) => other,
            (Some(shape), Some(other)) => union_types(shape, other),
        }
    }
}

/// One field-level contribution: `$name[<key>] = <value>`.
struct FieldContribution {
    key: String,
    ty: Type,
}

/// What a single assignment site contributes - applied to a
/// `GlobalAccumulator` only once the include-graph gate (for a top-level
/// site) has been resolved.
enum SiteContribution {
    WholeValue(Type),
    ShapeField(String, Type),
    ShapeFields(Vec<FieldContribution>),
}

impl SiteContribution {
    fn apply(self, acc: &mut GlobalAccumulator) {
        match self {
            SiteContribution::WholeValue(Type::ArrayShape(fields)) => {
                for (key, ty, _) in fields {
                    acc.add_shape_field(key, ty);
                }
            }
            SiteContribution::WholeValue(ty) => acc.add_other(ty),
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
struct StagedSite {
    gate: Option<PathBuf>,
    name: Arc<str>,
    contribution: SiteContribution,
}

/// One raw `include`-shaped call site found anywhere in the corpus.
struct IncludeSite<'a> {
    source: &'a Path,
    top_level: bool,
    target: &'a Expr,
}

/// Per-file, per-scope walk state that doesn't change as the walk
/// descends into nested scopes.
struct FileCtx<'a> {
    registry: &'a Registry,
    path: &'a Path,
}

/// Accumulates every site/include-site found across the whole walk,
/// threaded through by `&mut` rather than returned, since both
/// collections grow independently at arbitrary points in the recursion.
#[derive(Default)]
struct WalkOutput<'a> {
    sites: Vec<StagedSite>,
    include_sites: Vec<IncludeSite<'a>>,
}

impl Registry {
    /// Builds `self.global_variables` from scratch over `files` - see the
    /// module doc comment. Called at the end of `Registry::index_and_flatten`,
    /// `Project::finish_loading`, and the equivalent point in
    /// `Project::update_file`/`update_file_cancellable` - the same settle
    /// points `precompute_member_owners`/`precompute_ancestor_names`
    /// already run at in all three places, since every function/class this
    /// pass's own resolver depends on must already be indexed.
    pub(crate) fn infer_global_variables(&mut self, files: &[(&Path, &[Stmt])]) {
        // Every read below goes through this reborrow, mirroring
        // `precompute_member_owners`'s own established shape
        // (`hierarchy.rs`): build the whole result into locals first,
        // assign into `self` only once the borrow is done.
        let registry: &Registry = self;

        let mut out = WalkOutput::default();
        for (path, stmts) in files {
            let mut ctx = NameContext::default();
            let file_ctx = FileCtx { registry, path };
            let stmt_refs: Vec<&Stmt> = stmts.iter().collect();
            walk_top_level(&stmt_refs, &mut ctx, &file_ctx, &mut out);
        }

        let global_scope_files = resolve_global_scope_files(files, &out.include_sites);

        let mut accumulators: FxHashMap<Arc<str>, GlobalAccumulator> = FxHashMap::default();
        for site in out.sites {
            if let Some(path) = &site.gate
                && !global_scope_files.contains(path)
            {
                continue;
            }
            site.contribution
                .apply(accumulators.entry(site.name).or_default());
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
fn resolve_global_scope_files<'a>(
    files: &[(&'a Path, &[Stmt])],
    include_sites: &[IncludeSite<'a>],
) -> FxHashSet<PathBuf> {
    let all_paths: Vec<&Path> = files.iter().map(|(p, _)| *p).collect();

    let mut top_level_edges: FxHashMap<PathBuf, Vec<PathBuf>> = FxHashMap::default();
    // Every resolved incoming edge, top-level or not - a file targeted
    // *only* by function-scoped includes still has a real incoming edge
    // here, and must NOT default to "root" just because none of its
    // edges happen to be top-level (that was a real bug: it made a
    // file's own top level count as included whenever its only resolved
    // includers were all inside a function, exactly backwards from the
    // "excluded" outcome that shape is supposed to get).
    let mut has_any_incoming: FxHashSet<PathBuf> = FxHashSet::default();

    for site in include_sites {
        let Some(suffix) = resolve_include_target_suffix(site.target) else {
            continue;
        };
        if suffix.is_empty() {
            continue;
        }
        let matches: Vec<&Path> = all_paths
            .iter()
            .copied()
            .filter(|p| path_ends_with_suffix(p, &suffix))
            .collect();
        let [target] = matches[..] else {
            continue;
        };
        has_any_incoming.insert(target.to_path_buf());
        if site.top_level {
            top_level_edges
                .entry(site.source.to_path_buf())
                .or_default()
                .push(target.to_path_buf());
        }
    }

    // Roots: never a resolved include target anywhere at all (a real
    // entry point) - reached only through a function-scoped include is
    // excluded, not defaulted to included; reached only through an
    // unresolved/dynamic include has no edge recorded at all, so it still
    // defaults to included, same as today.
    let mut result: FxHashSet<PathBuf> = FxHashSet::default();
    let mut queue: Vec<PathBuf> = Vec::new();
    for (path, _) in files {
        let path_buf = path.to_path_buf();
        if !has_any_incoming.contains(&path_buf) && result.insert(path_buf.clone()) {
            queue.push(path_buf);
        }
    }
    while let Some(current) = queue.pop() {
        if let Some(targets) = top_level_edges.get(&current) {
            for target in targets {
                if result.insert(target.clone()) {
                    queue.push(target.clone());
                }
            }
        }
    }
    result
}

fn path_ends_with_suffix(path: &Path, suffix: &str) -> bool {
    let path_str = path.to_string_lossy().replace('\\', "/");
    let suffix = suffix.trim_start_matches('/');
    path_str == suffix || path_str.ends_with(&format!("/{suffix}"))
}

/// Resolves an `include`/`require`-style target expression to the literal
/// path suffix it names, or `None` if it depends on anything not
/// statically known (a variable, an index, a call, ...). A bare `Name`
/// (a constant reference such as `PHPWG_ROOT_PATH`) contributes an empty
/// string and doesn't invalidate resolution - only the trailing literal
/// suffix needs to match a real file's path, never the constant's actual
/// value.
fn resolve_include_target_suffix(expr: &Expr) -> Option<String> {
    match &unwrap_grouping(expr).kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        ExprKind::Name(_) => Some(String::new()),
        ExprKind::Binary(BinOp::Concat, l, r) => {
            let l = resolve_include_target_suffix(l)?;
            let r = resolve_include_target_suffix(r)?;
            Some(l + &r)
        }
        _ => None,
    }
}

fn is_include_like_name(name: &str) -> bool {
    matches!(
        name,
        "include" | "include_once" | "require" | "require_once"
    )
}

/// Walks a file's own top-level statements - every assignment found here
/// is staged as a top-level site (gated by the include-graph result
/// applied later), regardless of any `global $x;` (file top level is
/// already, potentially, the real global scope).
fn walk_top_level<'a>(
    stmts: &[&'a Stmt],
    ctx: &mut NameContext,
    file_ctx: &FileCtx<'a>,
    out: &mut WalkOutput<'a>,
) {
    let empty = FxHashSet::default();
    walk_scope(stmts, ctx, file_ctx, true, &empty, out);
}

/// Walks one function/method/closure body - `global_names` is the
/// complete set of names any `StmtKind::Global` anywhere in this body
/// introduces (computed once, upfront, since real PHP's `global $x;`
/// binds `$x` for the rest of the whole body regardless of which branch
/// it textually appears in - no need to track this incrementally through
/// the walk with per-branch cloning). Every site found here is
/// unconditional (`gate: None`) - a function body's own scope isn't
/// subject to the include-graph check the way file top level is.
fn walk_function_body<'a>(
    stmts: &[&'a Stmt],
    ctx: &NameContext,
    file_ctx: &FileCtx<'a>,
    out: &mut WalkOutput<'a>,
) {
    let global_names = collect_global_names(stmts);
    let mut ctx = ctx.clone();
    walk_scope(stmts, &mut ctx, file_ctx, false, &global_names, out);
}

fn collect_global_names(stmts: &[&Stmt]) -> FxHashSet<Arc<str>> {
    let mut out = FxHashSet::default();
    collect_global_names_rec(stmts, &mut out);
    out
}

fn collect_global_names_rec(stmts: &[&Stmt], out: &mut FxHashSet<Arc<str>>) {
    for stmt in stmts {
        if let StmtKind::Global(names) = &stmt.kind {
            for name in names {
                out.insert(Arc::from(name.as_str()));
            }
        }
        // Don't descend into a nested scope boundary - a nested function/
        // closure/anonymous-class method needs its own `global` to
        // qualify, never inherits the enclosing body's. `global` is a
        // statement, never an expression, so a closure buried inside this
        // statement's own expressions (e.g. `$x = function () { global
        // $z; };`) is already correctly excluded just by never visiting
        // expressions here at all - nothing to search for there.
        if matches!(
            &stmt.kind,
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_)
        ) {
            continue;
        }
        collect_global_names_rec(&stmt_children(stmt), out);
    }
}

fn walk_scope<'a>(
    stmts: &[&'a Stmt],
    ctx: &mut NameContext,
    file_ctx: &FileCtx<'a>,
    top_level: bool,
    global_names: &FxHashSet<Arc<str>>,
    out: &mut WalkOutput<'a>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(imports),
            StmtKind::FunctionDecl(f) => {
                walk_function_body(&f.body.iter().collect::<Vec<_>>(), ctx, file_ctx, out);
                continue;
            }
            StmtKind::ClassDecl(c) => {
                for method in &c.methods {
                    walk_function_body(
                        &method.function.body.iter().collect::<Vec<_>>(),
                        ctx,
                        file_ctx,
                        out,
                    );
                }
                continue;
            }
            StmtKind::Expression(e) => {
                handle_top_expr_statement(e, ctx, file_ctx, top_level, global_names, out);
            }
            _ => {}
        }

        for e in stmt_own_exprs(stmt) {
            walk_expr_for_includes_and_scopes(e, ctx, file_ctx, top_level, out);
        }

        walk_scope(
            &stmt_children(stmt),
            ctx,
            file_ctx,
            top_level,
            global_names,
            out,
        );
    }
}

/// Recognizes `$name = <expr>;` / `$name[<key>] = <expr>;` / their `??=`
/// forms, and `$name = array_merge($name, <rhs>);`, staging a site when
/// `$name` is a real candidate here (file top level, or `global_names`
/// already contains it).
fn handle_top_expr_statement<'a>(
    e: &'a Expr,
    ctx: &NameContext,
    file_ctx: &FileCtx<'a>,
    top_level: bool,
    global_names: &FxHashSet<Arc<str>>,
    out: &mut WalkOutput<'a>,
) {
    let is_site = |name: &str| top_level || global_names.contains(name);
    let gate = || top_level.then(|| file_ctx.path.to_path_buf());

    match &e.kind {
        ExprKind::Assign(lhs, rhs) | ExprKind::AssignRef(lhs, rhs) => {
            handle_assignment_target(lhs, rhs, ctx, file_ctx, &is_site, &gate, out, false);
        }
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, rhs) => {
            handle_assignment_target(lhs, rhs, ctx, file_ctx, &is_site, &gate, out, false);
        }
        ExprKind::CompoundAssign(_, lhs, rhs) => {
            handle_assignment_target(lhs, rhs, ctx, file_ctx, &is_site, &gate, out, true);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_assignment_target<'a>(
    lhs: &Expr,
    rhs: &Expr,
    ctx: &NameContext,
    file_ctx: &FileCtx<'a>,
    is_site: &dyn Fn(&str) -> bool,
    gate: &dyn Fn() -> Option<PathBuf>,
    out: &mut WalkOutput<'a>,
    force_mixed: bool,
) {
    match &unwrap_grouping(lhs).kind {
        ExprKind::Variable(name) if is_site(name) => {
            let contribution = if force_mixed {
                SiteContribution::WholeValue(Type::Mixed(false))
            } else if let Some(fields) = array_merge_self_fields(name, rhs, ctx, file_ctx.registry)
            {
                SiteContribution::ShapeFields(fields)
            } else {
                SiteContribution::WholeValue(resolve_static_expr_type(rhs, file_ctx.registry, ctx))
            };
            out.sites.push(StagedSite {
                gate: gate(),
                name: Arc::from(name.as_ref()),
                contribution,
            });
        }
        ExprKind::Index(base, key_opt) => {
            if let Some(key) = key_opt
                && let ExprKind::Variable(name) = &unwrap_grouping(base).kind
                && is_site(name)
                && let Some(key_str) = literal_string_key(key)
            {
                // `$name[<key>] = <rhs>;` - a direct, single-level write:
                // the field's own value is `<rhs>`'s real resolved type.
                let ty = if force_mixed {
                    Type::Mixed(false)
                } else {
                    resolve_static_expr_type(rhs, file_ctx.registry, ctx)
                };
                out.sites.push(StagedSite {
                    gate: gate(),
                    name: Arc::from(name.as_ref()),
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
                    name: Arc::from(name.as_ref()),
                    contribution: SiteContribution::ShapeField(key_str, Type::Mixed(false)),
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
fn root_variable_and_first_key(expr: &Expr) -> Option<(&Arc<str>, String)> {
    let ExprKind::Index(base, key) = &unwrap_grouping(expr).kind else {
        return None;
    };
    match &unwrap_grouping(base).kind {
        ExprKind::Variable(name) => key
            .as_deref()
            .and_then(literal_string_key)
            .map(|k| (name, k)),
        ExprKind::Index(..) => root_variable_and_first_key(base),
        _ => None,
    }
}

/// `$name = array_merge($name, <rhs>);` - real PHP folds `<rhs>`'s own
/// keys into `$name`, so if `<rhs>` resolves to an array-shaped type,
/// merge its fields in rather than discarding them. `None` for anything
/// else (not this exact self-referencing shape, or `<rhs>` didn't resolve
/// to anything array-shaped) - the caller falls back to its own ordinary
/// whole-value handling in that case.
fn array_merge_self_fields(
    name: &Arc<str>,
    rhs: &Expr,
    ctx: &NameContext,
    registry: &Registry,
) -> Option<Vec<FieldContribution>> {
    let ExprKind::Call(callee, args) = &unwrap_grouping(rhs).kind else {
        return None;
    };
    let ExprKind::Name(callee_name) = &unwrap_grouping(callee).kind else {
        return None;
    };
    if callee_name != "array_merge" || args.len() != 2 {
        return None;
    }
    let ExprKind::Variable(first_name) = &unwrap_grouping(&args[0].value).kind else {
        return None;
    };
    if first_name != name {
        return None;
    }
    match resolve_static_expr_type(&args[1].value, registry, ctx) {
        Type::ArrayShape(fields) => Some(
            fields
                .into_iter()
                .map(|(key, ty, _)| FieldContribution { key, ty })
                .collect(),
        ),
        _ => None,
    }
}

fn literal_string_key(expr: &Expr) -> Option<String> {
    match &unwrap_grouping(expr).kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        ExprKind::Literal(Literal::Int(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Walks an expression looking only for two things: an `include`-shaped
/// call site, and a nested `Closure`/`NewAnonymousClass` scope boundary -
/// everything else just recurses via `expr_children`. Not the same as
/// `resolve_static_expr_type` below, which answers a completely different
/// question ("what type is this expression") for a very different,
/// narrower set of callers.
fn walk_expr_for_includes_and_scopes<'a>(
    expr: &'a Expr,
    ctx: &mut NameContext,
    file_ctx: &FileCtx<'a>,
    top_level: bool,
    out: &mut WalkOutput<'a>,
) {
    match &expr.kind {
        ExprKind::Closure { body, .. } => {
            walk_function_body(&body.iter().collect::<Vec<_>>(), ctx, file_ctx, out);
            return;
        }
        ExprKind::NewAnonymousClass(decl, _) => {
            for method in &decl.methods {
                walk_function_body(
                    &method.function.body.iter().collect::<Vec<_>>(),
                    ctx,
                    file_ctx,
                    out,
                );
            }
            return;
        }
        ExprKind::Call(callee, args) => {
            if let ExprKind::Name(name) = &unwrap_grouping(callee).kind
                && is_include_like_name(name)
                && let Some(target) = args.first()
            {
                out.include_sites.push(IncludeSite {
                    source: file_ctx.path,
                    top_level,
                    target: &target.value,
                });
            }
        }
        _ => {}
    }
    for child in expr_children(expr) {
        walk_expr_for_includes_and_scopes(child, ctx, file_ctx, top_level, out);
    }
}

/// Resolves a self-contained expression to a `Type`, using only registry
/// lookups and a positionally-correct `NameContext` - no `Checker`/
/// `Locals`, neither exists yet at this point in the pipeline.
fn resolve_static_expr_type(expr: &Expr, registry: &Registry, ctx: &NameContext) -> Type {
    let expr = unwrap_grouping(expr);
    match &expr.kind {
        ExprKind::New(callee, _args) => {
            if let ExprKind::Name(name) = &unwrap_grouping(callee).kind {
                return Type::Object(ctx.resolve(name), vec![]);
            }
            Type::Mixed(false)
        }
        ExprKind::ArrayLiteral(items) => {
            let mut fields = Vec::new();
            for item in items {
                let Some(key_expr) = &item.key else {
                    continue;
                };
                let Some(key) = literal_string_key(key_expr) else {
                    continue;
                };
                let ty = resolve_static_expr_type(&item.value, registry, ctx);
                fields.push((key, ty, false));
            }
            Type::ArrayShape(fields)
        }
        ExprKind::Call(callee, _args) => {
            let ExprKind::Name(name) = &unwrap_grouping(callee).kind else {
                return Type::Mixed(false);
            };
            let (primary, fallback) = ctx.resolve_function_candidates(name);
            registry
                .lookup_function(&primary)
                .or_else(|| {
                    fallback
                        .as_deref()
                        .and_then(|f| registry.lookup_function(f))
                })
                .map(|sig| sig.return_type.clone())
                .unwrap_or(Type::Mixed(false))
        }
        ExprKind::StaticCall(target, method, _args) => {
            let fqn = ctx.resolve(target.as_name().unwrap_or("?"));
            registry
                .lookup_method(&fqn, method)
                .map(|sig| sig.return_type.clone())
                .unwrap_or(Type::Mixed(false))
        }
        ExprKind::ClassConstFetch(StaticTarget::Name(class, _), name) => {
            let fqn = ctx.resolve(class);
            registry
                .lookup_const(&fqn, name)
                .map(|c| c.ty.clone())
                .unwrap_or(Type::Mixed(false))
        }
        ExprKind::MethodCall(recv, method, _args, _) => {
            match resolve_static_expr_type(recv, registry, ctx) {
                Type::Object(class, _) => registry
                    .lookup_method(&class, method)
                    .map(|sig| sig.return_type.clone())
                    .unwrap_or(Type::Mixed(false)),
                _ => Type::Mixed(false),
            }
        }
        _ => infer_const_type(expr),
    }
}
