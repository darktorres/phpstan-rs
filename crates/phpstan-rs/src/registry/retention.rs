//! Whether a call keeps what it is given: a function releases the value of
//! a parameter when nothing of it outlives the call, so the caller's value
//! can be reached by no code but its own once the call has returned.
//!
//! A body decides what it does with each parameter itself (`ParamUses`):
//! it keeps the value, or only calls it, tests it, reads a property of it,
//! or passes it on to a function. The registry settles the passes on
//! (`Registry::resolve_retention`): a parameter is released when its body
//! keeps nothing and every function it is passed to releases it there, a
//! builtin by the table `tools/builtin-runs/record.php` makes, a function
//! of the program by this pass. A chain of calls that returns to itself
//! and keeps nothing on the way releases it.

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::arena::{self, Visitor};

use crate::registry::bodies::BodySource;
use crate::registry::effects::builtin_releases;
use crate::registry::{FnId, Registry};

/// What a body does with one parameter.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ParamUse {
    /// The body keeps the parameter's value past the call, or may.
    pub(crate) kept: bool,
    /// The functions the body passes the parameter to, each with the
    /// position the parameter takes there.
    pub(crate) passed: Vec<(QualifiedName, u32)>,
}

/// What a body does with its parameters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ParamUses {
    pub(crate) params: Box<[ParamUse]>,
}

impl ParamUses {
    /// What the body `body` of a function with the parameters `params` does
    /// with them; `None` when it has none.
    pub(crate) fn of(
        arenas: &NodeArenas,
        params: &[Param],
        body: &[StmtId],
    ) -> Option<Box<ParamUses>> {
        if params.is_empty() {
            return None;
        }
        let mut uses = Uses {
            index: FxHashMap::default(),
            uses: params
                .iter()
                .map(|param| ParamUse {
                    kept: param.by_ref || param.variadic || param.promotion.is_some(),
                    passed: Vec::new(),
                })
                .collect(),
            reads_all: arena::body_contains_yield(arenas, body),
        };
        for (at, param) in params.iter().enumerate() {
            uses.index.insert(param.name.name.clone(), at);
        }
        arena::walk_stmts(&mut uses, arenas, body);
        if uses.reads_all {
            for used in &mut uses.uses {
                used.kept = true;
            }
        }
        Some(Box::new(ParamUses {
            params: uses.uses.into_boxed_slice(),
        }))
    }
}

struct Uses {
    index: FxHashMap<Name, usize>,
    uses: Vec<ParamUse>,
    /// The body reads its variables or its arguments some way a use of the
    /// name does not show, or its frame outlives the call.
    reads_all: bool,
}

impl Uses {
    fn keep(&mut self, name: &Name) {
        if let Some(&at) = self.index.get(name) {
            self.uses[at].kept = true;
        }
    }

    /// The variable `id` is, when it is a plain one.
    fn variable(arenas: &NodeArenas, id: ExprId) -> Option<&Name> {
        match &arenas.expr(arenas.peeled(id)?)?.kind {
            ExprKind::Variable(name) => Some(name),
            _ => None,
        }
    }

    /// A use of `id` that keeps nothing of the value it evaluates to: the
    /// base of a property access, an identity or type test.
    fn kept_by_none(&mut self, arenas: &NodeArenas, id: ExprId) {
        if Self::variable(arenas, id).is_none() {
            self.visit_expr(arenas, id);
        }
    }

    fn call(&mut self, arenas: &NodeArenas, id: ExprId, callee: ExprId, args: &ArgRange) {
        let args = arenas.args(args.clone());
        match arenas.peeled(callee).and_then(|id| arenas.expr(id)) {
            Some(ExprNode {
                kind: ExprKind::Variable(_),
                ..
            }) => {
                // Calling the value keeps nothing of it.
                for arg in args {
                    self.visit_expr(arenas, arg.value);
                }
            }
            Some(ExprNode {
                kind: ExprKind::Name(name),
                ..
            }) => {
                let Some(name) = arenas.qualified_name(name) else {
                    arena::walk_expr(self, arenas, id);
                    return;
                };
                if reads_variables_by_name(&name.written) {
                    self.reads_all = true;
                }
                let mut positional = true;
                for (at, arg) in args.iter().enumerate() {
                    positional &= !arg.spread;
                    match Self::variable(arenas, arg.value) {
                        Some(variable)
                            if positional
                                && arg.name.is_none()
                                && self.index.contains_key(variable) =>
                        {
                            let param = self.index[variable];
                            self.uses[param].passed.push((name.clone(), at as u32));
                        }
                        _ => self.visit_expr(arenas, arg.value),
                    }
                }
            }
            _ => arena::walk_expr(self, arenas, id),
        }
    }
}

/// Whether the function written as `name` reads the variables or the
/// arguments of the body that calls it.
fn reads_variables_by_name(name: &str) -> bool {
    let name = name.trim_start_matches('\\');
    [
        "compact",
        "extract",
        "get_defined_vars",
        "func_get_args",
        "func_get_arg",
        "debug_backtrace",
        "debug_print_backtrace",
        "parse_str",
        "mb_parse_str",
    ]
    .iter()
    .any(|reader| name.eq_ignore_ascii_case(reader))
}

impl Visitor for Uses {
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::Unset(targets) => {
                for target in arenas.expr_ids(targets) {
                    self.kept_by_none(arenas, *target);
                }
            }
            _ => arena::walk_stmt(self, arenas, id),
        }
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Variable(name) => self.keep(name),
            ExprKind::PropertyAccess(base, _, _) => self.kept_by_none(arenas, *base),
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.kept_by_none(arenas, *base);
                self.visit_expr(arenas, *name);
            }
            ExprKind::Assign(lhs, rhs) => {
                self.kept_by_none(arenas, *lhs);
                self.visit_expr(arenas, *rhs);
            }
            ExprKind::Binary(BinOp::Identical | BinOp::NotIdentical, a, b) => {
                self.kept_by_none(arenas, *a);
                self.kept_by_none(arenas, *b);
            }
            ExprKind::Instanceof(subject, target) => {
                self.kept_by_none(arenas, *subject);
                if let InstanceofTarget::Expr(target) = target {
                    self.visit_expr(arenas, *target);
                }
            }
            ExprKind::Isset(items) => {
                for item in arenas.expr_ids(items) {
                    self.kept_by_none(arenas, *item);
                }
            }
            ExprKind::Empty(item) => self.kept_by_none(arenas, *item),
            ExprKind::Call(callee, args) => self.call(arenas, id, *callee, args),
            // A closure's body is a scope of its own; what it captures stays
            // with it.
            ExprKind::Closure(closure) => {
                if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure) {
                    for u in arenas.closure_uses(&closure.uses) {
                        self.keep(&u.name.name);
                    }
                }
            }
            ExprKind::DynamicVariable(_)
            | ExprKind::Eval(_)
            | ExprKind::Include(..)
            | ExprKind::Yield(..)
            | ExprKind::YieldFrom(_) => {
                self.reads_all = true;
                arena::walk_expr(self, arenas, id);
            }
            _ => arena::walk_expr(self, arenas, id),
        })
    }
}

/// One parameter of a function of the program no use of keeps by itself,
/// and the parameters it is passed to.
struct Candidate {
    key: (FnId, u32),
    passed: Vec<(FnId, u32)>,
}

impl Registry {
    /// Settles which parameters of the functions in scope are released.
    pub(crate) fn resolve_retention(&mut self, bodies: &dyn BodySource) {
        let names = self.scoped_function_names();
        let registry: &Registry = self;
        let candidates: Vec<Candidate> = names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|name| registry.retention_candidates(bodies, name))
            .collect();
        let index_of: FxHashMap<(FnId, u32), usize> = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| (candidate.key, index))
            .collect();
        let mut passed_to: Vec<Vec<usize>> = vec![Vec::new(); candidates.len()];
        let mut kept: Vec<usize> = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            for dependency in &candidate.passed {
                if self.purity_results.released_parameters.contains(dependency) {
                    continue;
                }
                match index_of.get(dependency) {
                    Some(&target) => passed_to[target].push(index),
                    None => kept.push(index),
                }
            }
        }
        // A parameter passed to one that is kept is kept; a chain that only
        // returns to itself keeps nothing.
        let mut is_kept = vec![false; candidates.len()];
        while let Some(index) = kept.pop() {
            if std::mem::replace(&mut is_kept[index], true) {
                continue;
            }
            kept.extend(passed_to[index].iter().copied());
        }
        for (candidate, kept) in candidates.iter().zip(is_kept) {
            if !kept {
                self.purity_results
                    .released_parameters
                    .insert(candidate.key);
            }
        }
    }

    /// The parameters of the function `name` that its body keeps nothing
    /// of, with what each is passed to; the ones passed to something that
    /// keeps it, or that is not known, are left out.
    fn retention_candidates(&self, bodies: &dyn BodySource, name: &Name) -> Vec<Candidate> {
        let Some((id, _, sig)) = self.function(name) else {
            return Vec::new();
        };
        if sig.is_stub {
            return Vec::new();
        }
        let Some(uses) = bodies
            .summary(&sig.provenance)
            .and_then(|summary| summary.parameters.as_deref())
        else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        'parameters: for (at, used) in uses.params.iter().enumerate() {
            if used.kept {
                continue;
            }
            let mut passed = Vec::new();
            for (callee, position) in &used.passed {
                let Some((declared, callee_sig)) = self.resolve_function(callee) else {
                    continue 'parameters;
                };
                if callee_sig.is_stub {
                    if !builtin_releases(&declared.to_ascii_lowercase(), *position) {
                        continue 'parameters;
                    }
                    continue;
                }
                let Some((callee_id, _, _)) = self.function(declared) else {
                    continue 'parameters;
                };
                passed.push((callee_id, *position));
            }
            candidates.push(Candidate {
                key: (id, at as u32),
                passed,
            });
        }
        candidates
    }

    /// Whether a call of the function `callee` leaves no code but the
    /// caller's able to reach what it is given as its argument `at`
    /// (from 0) once it has returned.
    pub(crate) fn releases_argument(&self, callee: &QualifiedName, at: u32) -> bool {
        let Some((declared, sig)) = self.resolve_function(callee) else {
            return false;
        };
        if sig.is_stub {
            return builtin_releases(&declared.to_ascii_lowercase(), at);
        }
        self.function(declared)
            .is_some_and(|(id, _, _)| self.purity_results.released_parameters.contains(&(id, at)))
    }
}
