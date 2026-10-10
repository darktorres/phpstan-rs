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
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::arena::{self, Visitor};

use smallvec::SmallVec;

use crate::registry::bodies::BodySource;
use crate::registry::body_passes::{Pass, Wants};
use crate::registry::effects::builtin_releases;
use crate::registry::purity::ObjectLeaks;
use crate::registry::{ClassId, FnId, FunctionReflection, Registry};

/// What a body does with one parameter.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ParamUse {
    /// The body keeps the parameter's value past the call, or may.
    pub(crate) kept: bool,
    /// The functions the body passes the parameter to, each with the
    /// position the parameter takes there.
    pub(crate) passed: Vec<(QualifiedName, u32)>,
}

/// A method a body calls on `$this`, which then runs with the same object.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ThisCall {
    /// `$this->m()`, `static::m()`: looked up on the class of the object.
    Method(Name),
    /// `self::m()`: looked up on the class the body is declared in.
    SelfMethod(Name),
    /// `parent::m()`: looked up on the parent of the class the body is
    /// declared in.
    ParentMethod(Name),
}

/// What a method body does with `$this`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ThisUse {
    /// The body keeps the object past the call, or may: it is returned,
    /// stored, passed on to code not known, or bound in a closure.
    pub(crate) kept: bool,
    /// The functions the body passes the object to, each with the position
    /// it takes there.
    pub(crate) passed: Vec<(QualifiedName, u32)>,
    /// The methods the body calls on the object.
    pub(crate) calls: Vec<ThisCall>,
    /// The body reads or writes a property of the object, which a magic
    /// accessor or a hook may run code for.
    pub(crate) accesses_properties: bool,
}

impl ThisUse {
    fn is_empty(&self) -> bool {
        !self.kept && self.passed.is_empty() && self.calls.is_empty() && !self.accesses_properties
    }
}

/// What a body does with its parameters and with `$this`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ParamUses {
    pub(crate) params: Box<[ParamUse]>,
    pub(crate) this: ThisUse,
}

impl ParamUses {
    /// What the body `body` of a function with the parameters `params` does
    /// with them, by a walk of its own; `None` when it has none.
    #[cfg(debug_assertions)]
    pub(crate) fn reference(
        arenas: &NodeArenas,
        params: &[Param],
        body: &[StmtId],
        yields: bool,
    ) -> Option<Box<ParamUses>> {
        let mut uses = Uses::new(params, yields);
        arena::walk_stmts(&mut uses, arenas, body);
        uses.finish(params)
    }
}

impl Uses {
    /// The walk's state for a function with the parameters `params`.
    /// `yields` is whether its body has a `yield`.
    pub(crate) fn new(params: &[Param], yields: bool) -> Uses {
        let mut uses = Uses {
            this: ThisUse::default(),
            in_callable: false,
            callables: Vec::new(),
            index: FxHashMap::default(),
            uses: params
                .iter()
                .map(|param| ParamUse {
                    kept: param.by_ref || param.variadic || param.promotion.is_some(),
                    passed: Vec::new(),
                })
                .collect(),
            reads_all: yields,
        };
        for (at, param) in params.iter().enumerate() {
            uses.index.insert(param.name.name.clone(), at);
        }
        uses
    }

    /// What the body does with the parameters `params`, once walked; `None`
    /// when it has none.
    pub(crate) fn finish(mut self, params: &[Param]) -> Option<Box<ParamUses>> {
        if self.reads_all {
            for used in &mut self.uses {
                used.kept = true;
            }
            self.this.kept = true;
        }
        if params.is_empty() && self.this.is_empty() {
            return None;
        }
        Some(Box::new(ParamUses {
            params: self.uses.into_boxed_slice(),
            this: self.this,
        }))
    }
}

pub(crate) struct Uses {
    index: FxHashMap<Name, usize>,
    uses: Vec<ParamUse>,
    this: ThisUse,
    /// The expression being read is the target of a first-class callable,
    /// which binds `$this`.
    in_callable: bool,
    /// The first-class callables being read, with the flag they replaced.
    callables: Vec<(ExprId, bool)>,
    /// The body reads its variables or its arguments some way a use of the
    /// name does not show, or its frame outlives the call.
    reads_all: bool,
}

impl Uses {
    fn keep(&mut self, name: &Name) {
        if name.as_str() == "this" {
            self.this.kept = true;
        }
        if let Some(&at) = self.index.get(name) {
            self.uses[at].kept = true;
        }
    }

    /// Whether `id` is `$this`.
    fn is_this(arenas: &NodeArenas, id: ExprId) -> bool {
        Self::variable(arenas, id).is_some_and(|name| name.as_str() == "this")
    }

    /// A call of a method on `$this` or a relative class, with `args`.
    fn this_call(&mut self, arenas: &NodeArenas, call: ThisCall, args: &ArgRange) {
        if self.in_callable {
            self.this.kept = true;
        } else {
            self.this.calls.push(call);
        }
        for arg in arenas.args(args.clone()) {
            self.visit_expr(arenas, arg.value);
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
                            if positional && arg.name.is_none() && variable.as_str() == "this" =>
                        {
                            self.this.passed.push((name.clone(), at as u32));
                        }
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

impl Uses {
    /// A use of `id` that keeps nothing of the value it evaluates to, as
    /// the children it asks to have visited.
    fn kept_child(arenas: &NodeArenas, id: ExprId, wanted: &mut SmallVec<[ExprId; 4]>) {
        if Self::variable(arenas, id).is_none() {
            wanted.push(id);
        }
    }

    /// A call of a method on `$this` or a relative class, whose arguments
    /// are then read.
    fn this_call_wants(&mut self, arenas: &NodeArenas, call: ThisCall, args: &ArgRange) -> Wants {
        if self.in_callable {
            self.this.kept = true;
        } else {
            self.this.calls.push(call);
        }
        Wants::Exprs(
            arenas
                .args(args.clone())
                .iter()
                .map(|arg| arg.value)
                .collect(),
        )
    }

    fn call_wants(&mut self, arenas: &NodeArenas, callee: ExprId, args: &ArgRange) -> Wants {
        let args = arenas.args(args.clone());
        match arenas.peeled(callee).and_then(|id| arenas.expr(id)) {
            Some(ExprNode {
                kind: ExprKind::Variable(_),
                ..
            }) => {
                // Calling the value keeps nothing of it.
                Wants::Exprs(args.iter().map(|arg| arg.value).collect())
            }
            Some(ExprNode {
                kind: ExprKind::Name(name),
                ..
            }) => {
                let Some(name) = arenas.qualified_name(name) else {
                    return Wants::All;
                };
                if reads_variables_by_name(&name.written) {
                    self.reads_all = true;
                }
                let mut wanted = SmallVec::new();
                let mut positional = true;
                for (at, arg) in args.iter().enumerate() {
                    positional &= !arg.spread;
                    match Self::variable(arenas, arg.value) {
                        Some(variable)
                            if positional && arg.name.is_none() && variable.as_str() == "this" =>
                        {
                            self.this.passed.push((name.clone(), at as u32));
                        }
                        Some(variable)
                            if positional
                                && arg.name.is_none()
                                && self.index.contains_key(variable) =>
                        {
                            let param = self.index[variable];
                            self.uses[param].passed.push((name.clone(), at as u32));
                        }
                        _ => wanted.push(arg.value),
                    }
                }
                Wants::Exprs(wanted)
            }
            _ => Wants::All,
        }
    }
}

impl Pass for Uses {
    fn stmt(&mut self, arenas: &NodeArenas, id: StmtId) -> Wants {
        let Some(stmt) = arenas.stmt(id) else {
            return Wants::None;
        };
        match &stmt.kind {
            StmtKind::Unset(targets) => {
                let mut wanted = SmallVec::new();
                for target in arenas.expr_ids(targets) {
                    Self::kept_child(arenas, *target, &mut wanted);
                }
                Wants::Exprs(wanted)
            }
            _ => Wants::All,
        }
    }

    fn leave_expr(&mut self, id: ExprId) {
        if let Some(&(callable, outer)) = self.callables.last()
            && callable == id
        {
            self.in_callable = outer;
            self.callables.pop();
        }
    }

    fn expr(&mut self, arenas: &NodeArenas, id: ExprId) -> Wants {
        let Some(e) = arenas.expr(id) else {
            return Wants::None;
        };
        match &e.kind {
            ExprKind::Variable(name) => {
                self.keep(name);
                Wants::None
            }
            ExprKind::PropertyAccess(base, _, _) => {
                self.this.accesses_properties |= Self::is_this(arenas, *base);
                let mut wanted = SmallVec::new();
                Self::kept_child(arenas, *base, &mut wanted);
                Wants::Exprs(wanted)
            }
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.this.accesses_properties |= Self::is_this(arenas, *base);
                let mut wanted = SmallVec::new();
                Self::kept_child(arenas, *base, &mut wanted);
                wanted.push(*name);
                Wants::Exprs(wanted)
            }
            ExprKind::MethodCall(base, method, args, _) if Self::is_this(arenas, *base) => {
                self.this_call_wants(arenas, ThisCall::Method(method.name.clone()), args)
            }
            ExprKind::StaticCall(target, method, args)
                if target.as_name().is_some_and(|class| {
                    ["self", "static", "parent"].iter().any(|relative| {
                        class
                            .written
                            .trim_start_matches('\\')
                            .eq_ignore_ascii_case(relative)
                    })
                }) =>
            {
                let written = target
                    .as_name()
                    .map(|class| class.written.as_str())
                    .unwrap_or("");
                let written = written.trim_start_matches('\\');
                let call = if written.eq_ignore_ascii_case("parent") {
                    ThisCall::ParentMethod(method.name.clone())
                } else if written.eq_ignore_ascii_case("self") {
                    ThisCall::SelfMethod(method.name.clone())
                } else {
                    ThisCall::Method(method.name.clone())
                };
                self.this_call_wants(arenas, call, args)
            }
            ExprKind::FirstClassCallable(inner) => {
                let outer = std::mem::replace(&mut self.in_callable, true);
                self.callables.push((id, outer));
                Wants::Exprs(SmallVec::from_slice(&[*inner]))
            }
            ExprKind::ArrowFn(closure) => {
                if let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*closure) {
                    self.this.kept |= !arrow.is_static;
                }
                Wants::All
            }
            ExprKind::Assign(lhs, rhs) => {
                let mut wanted = SmallVec::new();
                Self::kept_child(arenas, *lhs, &mut wanted);
                wanted.push(*rhs);
                Wants::Exprs(wanted)
            }
            ExprKind::Binary(BinOp::Identical | BinOp::NotIdentical, a, b) => {
                let mut wanted = SmallVec::new();
                Self::kept_child(arenas, *a, &mut wanted);
                Self::kept_child(arenas, *b, &mut wanted);
                Wants::Exprs(wanted)
            }
            ExprKind::Instanceof(subject, target) => {
                let mut wanted = SmallVec::new();
                Self::kept_child(arenas, *subject, &mut wanted);
                if let InstanceofTarget::Expr(target) = target {
                    wanted.push(*target);
                }
                Wants::Exprs(wanted)
            }
            ExprKind::Isset(items) => {
                let mut wanted = SmallVec::new();
                for item in arenas.expr_ids(items) {
                    Self::kept_child(arenas, *item, &mut wanted);
                }
                Wants::Exprs(wanted)
            }
            ExprKind::Empty(item) => {
                let mut wanted = SmallVec::new();
                Self::kept_child(arenas, *item, &mut wanted);
                Wants::Exprs(wanted)
            }
            ExprKind::Call(callee, args) => self.call_wants(arenas, *callee, args),
            // A closure's body is a scope of its own; what it captures stays
            // with it.
            ExprKind::Closure(closure) => {
                if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure) {
                    self.this.kept |= !closure.is_static;
                    for u in arenas.closure_uses(&closure.uses) {
                        self.keep(&u.name.name);
                    }
                }
                Wants::None
            }
            ExprKind::DynamicVariable(_)
            | ExprKind::Eval(_)
            | ExprKind::Include(..)
            | ExprKind::Yield(..)
            | ExprKind::YieldFrom(_) => {
                self.reads_all = true;
                Wants::All
            }
            _ => Wants::All,
        }
    }
}

#[cfg(debug_assertions)]
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
            ExprKind::PropertyAccess(base, _, _) => {
                self.this.accesses_properties |= Self::is_this(arenas, *base);
                self.kept_by_none(arenas, *base);
            }
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.this.accesses_properties |= Self::is_this(arenas, *base);
                self.kept_by_none(arenas, *base);
                self.visit_expr(arenas, *name);
            }
            ExprKind::MethodCall(base, method, args, _) if Self::is_this(arenas, *base) => {
                self.this_call(arenas, ThisCall::Method(method.name.clone()), args);
            }
            ExprKind::StaticCall(target, method, args)
                if target.as_name().is_some_and(|class| {
                    ["self", "static", "parent"].iter().any(|relative| {
                        class
                            .written
                            .trim_start_matches('\\')
                            .eq_ignore_ascii_case(relative)
                    })
                }) =>
            {
                let written = target
                    .as_name()
                    .map(|class| class.written.as_str())
                    .unwrap_or("");
                let written = written.trim_start_matches('\\');
                let call = if written.eq_ignore_ascii_case("parent") {
                    ThisCall::ParentMethod(method.name.clone())
                } else if written.eq_ignore_ascii_case("self") {
                    ThisCall::SelfMethod(method.name.clone())
                } else {
                    ThisCall::Method(method.name.clone())
                };
                self.this_call(arenas, call, args);
            }
            ExprKind::FirstClassCallable(inner) => {
                let outer = std::mem::replace(&mut self.in_callable, true);
                self.visit_expr(arenas, *inner);
                self.in_callable = outer;
            }
            ExprKind::ArrowFn(closure) => {
                if let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*closure) {
                    self.this.kept |= !arrow.is_static;
                }
                arena::walk_expr(self, arenas, id);
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
                    self.this.kept |= !closure.is_static;
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

impl Registry {
    /// Settles, for each class in scope, whether the object `new` makes of
    /// it, or `clone` copies of it, stays unreachable by code but its
    /// maker's once the constructor, or `__clone`, has returned.
    pub(crate) fn resolve_constructor_leaks(&mut self, bodies: &dyn BodySource) {
        let names = self.scoped_class_names();
        let registry: &Registry = self;
        let found: Vec<(ClassId, ObjectLeaks)> = names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|class| {
                let id = registry.class_ids.get(class).copied()?;
                Some((
                    id,
                    ObjectLeaks {
                        constructor: registry.method_leaks_this(bodies, class, "__construct"),
                        clone: registry.method_leaks_this(bodies, class, "__clone"),
                    },
                ))
            })
            .collect();
        self.purity_results
            .object_leaks
            .extend(found.iter().copied());
        self.purity_results.any_object_leaks = self
            .purity_results
            .object_leaks
            .values()
            .any(|leaks| leaks.constructor || leaks.clone);
    }

    /// Whether running `method` on an object of `class` may leave code but
    /// the caller's able to reach the object; a class with no such method
    /// leaves none.
    fn method_leaks_this(&self, bodies: &dyn BodySource, class: &Name, method: &str) -> bool {
        let Some((owner, sig)) = self.lookup_method_with_owner(class, method) else {
            return false;
        };
        let mut seen = FxHashSet::default();
        self.this_leaks(bodies, class, &owner, &Name::from(method), sig, &mut seen)
    }

    /// Whether the body of `method`, declared in `owner` and run on an
    /// object of `class`, keeps the object, or calls what does.
    fn this_leaks(
        &self,
        bodies: &dyn BodySource,
        class: &Name,
        owner: &Name,
        method: &Name,
        sig: &FunctionReflection,
        seen: &mut FxHashSet<(Name, Name)>,
    ) -> bool {
        if sig.is_stub || !seen.insert((owner.lowered().clone(), method.lowered().clone())) {
            return false;
        }
        let Some(summary) = bodies.summary(&sig.provenance) else {
            return true;
        };
        let Some(uses) = summary.parameters.as_deref() else {
            return false;
        };
        let this = &uses.this;
        if this.kept
            || this
                .passed
                .iter()
                .any(|(callee, at)| !self.releases_argument(callee, *at))
        {
            return true;
        }
        let mut called: Vec<(Name, Name, &FunctionReflection)> = Vec::new();
        for call in &this.calls {
            let found = match call {
                ThisCall::Method(name) => self
                    .lookup_method_with_owner(owner, name.as_str())
                    .filter(|(_, own)| own.visibility() == Visibility::Private)
                    .or_else(|| self.lookup_method_with_owner(class, name.as_str()))
                    .map(|(declared, sig)| (declared, name, sig)),
                ThisCall::SelfMethod(name) => self
                    .lookup_method_with_owner(owner, name.as_str())
                    .map(|(declared, sig)| (declared, name, sig)),
                ThisCall::ParentMethod(name) => self
                    .class_references_by_name(owner)
                    .and_then(|references| references.extends.first())
                    .and_then(|parent| self.lookup_method_with_owner(parent, name.as_str()))
                    .map(|(declared, sig)| (declared, name, sig)),
            };
            match found {
                Some((declared, name, sig)) => called.push((declared, name.clone(), sig)),
                None => match self.lookup_method_with_owner(class, "__call") {
                    Some((declared, sig)) => called.push((declared, Name::from("__call"), sig)),
                    None => return true,
                },
            }
        }
        if this.accesses_properties {
            if self.has_property_hooks(class) {
                return true;
            }
            for magic in ["__get", "__set", "__isset", "__unset"] {
                if let Some((declared, sig)) = self.lookup_method_with_owner(class, magic) {
                    called.push((declared, Name::from(magic), sig));
                }
            }
        }
        called.into_iter().any(|(declared, name, sig)| {
            self.this_leaks(bodies, class, &declared, &name, sig, seen)
        })
    }

    /// Whether `class`, or a class it extends or uses, declares a property
    /// with a hook that has a body.
    fn has_property_hooks(&self, class: &Name) -> bool {
        let mut classes: Vec<Name> = self.all_ancestor_names_of(class).into_iter().collect();
        classes.push(class.clone());
        classes.iter().any(|class| {
            self.property_names(class.as_str()).any(|property| {
                self.lookup_property(class.as_str(), property.as_str())
                    .is_some_and(|declared| declared.get_hook_body || declared.set_hook_body)
            })
        })
    }

    /// Whether an object made by `new` of the class written as `class`
    /// (`None` for one not known: `clone`, `new $c`, `new class`) may be
    /// reachable by code but its maker's.
    pub(crate) fn new_may_leak(&self, class: Option<&QualifiedName>) -> bool {
        let Some(class) = class else {
            return self.purity_results.any_object_leaks;
        };
        match class
            .resolved_class()
            .and_then(|name| self.lookup_class(name))
        {
            Some((name, _)) => self.class_constructor_leaks(name),
            None => self.purity_results.any_object_leaks,
        }
    }

    /// Whether an object `new` makes of `class` may be reachable by code
    /// but its maker's once the constructor has returned.
    pub(crate) fn class_constructor_leaks(&self, class: &str) -> bool {
        if self.is_builtin_class(class) {
            return false;
        }
        self.class_ids
            .get(class)
            .and_then(|id| self.purity_results.object_leaks.get(id))
            .is_none_or(|leaks| leaks.constructor)
    }

    /// Whether an object made by `new` or `clone` of a class not known may
    /// be reachable by code but its maker's.
    pub(crate) fn unknown_object_may_leak(&self) -> bool {
        self.purity_results.any_object_leaks
    }
}
