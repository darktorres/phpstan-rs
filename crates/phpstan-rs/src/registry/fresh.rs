//! Whether a function gives back an object nothing else holds: every value
//! it returns is made by the body itself (`new`, `clone`) or by a call that
//! gives back such an object, and the body does nothing with the object but
//! set its properties and return it. The caller of such a function owns
//! the result, as it owns one it made with `new`.
//!
//! A body decides what it returns itself (`FreshReturns`); the registry
//! settles the calls it names (`Registry::resolve_fresh`): a function is
//! fresh when its body is and every function it calls for a result is.

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::arena::{self, Visitor};

use crate::registry::bodies::BodySource;
use crate::registry::{FnId, MethodRef, Registry};

/// A call whose result a body returns, or assigns to a variable it returns.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum FreshCall {
    /// A function called by name.
    Function(QualifiedName),
    /// A static method called through a class named in the call.
    Static(QualifiedName, Name),
}

/// The calls a body that returns only objects it made depends on.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FreshReturns {
    pub(crate) calls: Box<[FreshCall]>,
}

impl FreshReturns {
    /// What a function with the parameters `params` and the body `body`
    /// returns, when it returns only objects it made.
    pub(crate) fn of(
        arenas: &NodeArenas,
        params: &[Param],
        body: &[StmtId],
    ) -> Option<Box<FreshReturns>> {
        if arena::body_contains_yield(arenas, body) {
            return None;
        }
        let mut walk = Returns::default();
        for param in params {
            walk.bad.insert(param.name.name.clone());
        }
        arena::walk_stmts(&mut walk, arenas, body);
        walk.finish()
    }
}

/// How a value a body makes or returns is made.
enum Source {
    /// An object made by the body or a value that is no object.
    Made,
    /// What a call gives back.
    Call(FreshCall),
}

#[derive(Default)]
struct Returns {
    /// A return is of something that may be held elsewhere.
    spoiled: bool,
    /// The variables that are returned.
    returned: FxHashSet<Name>,
    /// What each variable is assigned.
    assigned: FxHashMap<Name, Vec<Source>>,
    /// The variables bound some way other than by being assigned an object
    /// the body made, or used some way that lets the object leave.
    bad: FxHashSet<Name>,
    /// What the body returns directly.
    direct: Vec<Source>,
}

impl Returns {
    fn finish(self) -> Option<Box<FreshReturns>> {
        if self.spoiled {
            return None;
        }
        let mut calls = Vec::new();
        let mut sources = self.direct;
        for name in &self.returned {
            if self.bad.contains(name) {
                return None;
            }
            sources.extend(self.assigned.get(name)?.iter().map(|source| match source {
                Source::Made => Source::Made,
                Source::Call(call) => Source::Call(call.clone()),
            }));
        }
        for source in sources {
            if let Source::Call(call) = source {
                calls.push(call);
            }
        }
        Some(Box::new(FreshReturns {
            calls: calls.into_boxed_slice(),
        }))
    }

    /// What the expression `id` gives back, when it is an object the body
    /// makes, a value that is no object, or a call.
    fn source(arenas: &NodeArenas, id: ExprId) -> Option<Source> {
        let e = arenas.expr(arenas.peeled(id)?)?;
        match &e.kind {
            ExprKind::New(..)
            | ExprKind::NewAnonymousClass(_)
            | ExprKind::Clone(..)
            | ExprKind::Literal(_) => Some(Source::Made),
            ExprKind::Call(callee, _) => match &arenas.expr(arenas.peeled(*callee)?)?.kind {
                ExprKind::Name(name) => Some(Source::Call(FreshCall::Function(
                    arenas.qualified_name(name)?.clone(),
                ))),
                _ => None,
            },
            ExprKind::StaticCall(target, method, _) => {
                let class = target.as_name()?;
                let written = class.written.trim_start_matches('\\');
                if ["self", "static", "parent"]
                    .iter()
                    .any(|relative| written.eq_ignore_ascii_case(relative))
                {
                    return None;
                }
                Some(Source::Call(FreshCall::Static(
                    class.clone(),
                    method.name.clone(),
                )))
            }
            _ => None,
        }
    }

    fn variable(arenas: &NodeArenas, id: ExprId) -> Option<&Name> {
        match &arenas.expr(arenas.peeled(id)?)?.kind {
            ExprKind::Variable(name) => Some(name),
            _ => None,
        }
    }

    /// A use of `id` that lets nothing leave the object it evaluates to.
    fn kept(&mut self, arenas: &NodeArenas, id: ExprId) {
        if Self::variable(arenas, id).is_none() {
            self.visit_expr(arenas, id);
        }
    }
}

impl Visitor for Returns {
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        if let Some(var) = &clause.var {
            self.bad.insert(var.name.clone());
        }
        arena::walk_stmts(self, arenas, arenas.stmt_ids(&clause.body));
    }

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::Return(Some(value)) => match Self::variable(arenas, *value) {
                Some(name) => {
                    self.returned.insert(name.clone());
                }
                None => match Self::source(arenas, *value) {
                    Some(source) => {
                        self.direct.push(source);
                        // The arguments of what is returned are evaluated.
                        arena::walk_stmt(self, arenas, id);
                    }
                    None => self.spoiled = true,
                },
            },
            StmtKind::Global(vars) => {
                for var in arenas.global_vars(vars.clone()) {
                    match var {
                        GlobalVar::Name(name) => {
                            self.bad.insert(name.name.clone());
                        }
                        GlobalVar::Dynamic(_) => self.spoiled = true,
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for variable in arenas.static_vars(vars.clone()) {
                    self.bad.insert(variable.name.name.clone());
                }
            }
            StmtKind::Unset(targets) => {
                for target in arenas.expr_ids(targets) {
                    self.kept(arenas, *target);
                }
            }
            _ => arena::walk_stmt(self, arenas, id),
        }
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Variable(name) => {
                self.bad.insert(name.clone());
            }
            ExprKind::PropertyAccess(base, _, _) => self.kept(arenas, *base),
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.kept(arenas, *base);
                self.visit_expr(arenas, *name);
            }
            ExprKind::Assign(lhs, rhs) => {
                match (Self::variable(arenas, *lhs), Self::source(arenas, *rhs)) {
                    (Some(name), Some(source)) => {
                        self.assigned.entry(name.clone()).or_default().push(source);
                    }
                    (Some(name), None) => {
                        self.bad.insert(name.clone());
                    }
                    (None, _) => self.visit_expr(arenas, *lhs),
                }
                self.visit_expr(arenas, *rhs);
            }
            ExprKind::Binary(BinOp::Identical | BinOp::NotIdentical, a, b) => {
                self.kept(arenas, *a);
                self.kept(arenas, *b);
            }
            ExprKind::Instanceof(subject, target) => {
                self.kept(arenas, *subject);
                if let InstanceofTarget::Expr(target) = target {
                    self.visit_expr(arenas, *target);
                }
            }
            ExprKind::Isset(items) => {
                for item in arenas.expr_ids(items) {
                    self.kept(arenas, *item);
                }
            }
            ExprKind::Empty(item) => self.kept(arenas, *item),
            ExprKind::Closure(closure) => {
                if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure) {
                    for u in arenas.closure_uses(&closure.uses) {
                        self.bad.insert(u.name.name.clone());
                    }
                }
            }
            ExprKind::DynamicVariable(_) | ExprKind::Eval(_) | ExprKind::Include(..) => {
                self.spoiled = true;
            }
            _ => arena::walk_expr(self, arenas, id),
        })
    }
}

/// A function or static method whose body returns only objects it made.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Declaration {
    Function(FnId),
    Method(MethodRef),
}

struct Node {
    declaration: Declaration,
    calls: Vec<Declaration>,
}

impl Registry {
    /// Settles which functions and static methods in scope return only
    /// objects they made.
    pub(crate) fn resolve_fresh(&mut self, bodies: &dyn BodySource) {
        let functions = self.scoped_function_names();
        let registry: &Registry = self;
        let mut nodes: Vec<Node> = functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|name| {
                let (id, _, sig) = registry.function(name)?;
                registry.fresh_node(bodies, Declaration::Function(id), sig)
            })
            .collect();
        let classes = self.scoped_class_names();
        let registry: &Registry = self;
        let methods: Vec<Node> = classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|class| {
                let mut found = Vec::new();
                let Some(class_id) = registry.class_ids.get(class).copied() else {
                    return found;
                };
                for method in registry.method_names(class) {
                    let Some(sig) = registry.lookup_own_method(class, method) else {
                        continue;
                    };
                    let Some(slot) = registry.member_slot(class, method) else {
                        continue;
                    };
                    if !registry.method_is_static(class, method) {
                        continue;
                    }
                    let declaration = Declaration::Method(MethodRef::new(class_id, slot));
                    found.extend(registry.fresh_node(bodies, declaration, sig));
                }
                found
            })
            .collect();
        nodes.extend(methods);
        let index_of: FxHashMap<Declaration, usize> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.declaration, index))
            .collect();
        let mut called_by: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        let mut spoiled: Vec<usize> = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            for call in &node.calls {
                if self.declaration_is_fresh(*call) {
                    continue;
                }
                match index_of.get(call) {
                    Some(&target) => called_by[target].push(index),
                    None => spoiled.push(index),
                }
            }
        }
        // A function that calls one that is spoiled is spoiled; a chain of
        // calls that only returns to itself is not.
        let mut is_spoiled = vec![false; nodes.len()];
        while let Some(index) = spoiled.pop() {
            if std::mem::replace(&mut is_spoiled[index], true) {
                continue;
            }
            spoiled.extend(called_by[index].iter().copied());
        }
        for (node, spoiled) in nodes.iter().zip(is_spoiled) {
            if spoiled {
                continue;
            }
            match node.declaration {
                Declaration::Function(id) => {
                    self.purity_results.fresh_functions.insert(id);
                }
                Declaration::Method(method) => {
                    self.purity_results.fresh_methods.insert(method);
                }
            }
        }
    }

    fn declaration_is_fresh(&self, declaration: Declaration) -> bool {
        match declaration {
            Declaration::Function(id) => self.purity_results.fresh_functions.contains(&id),
            Declaration::Method(method) => self.purity_results.fresh_methods.contains(&method),
        }
    }

    /// The node of the declaration whose signature is `sig`, when its body
    /// returns only objects it made and every call it depends on is to a
    /// declaration of the program.
    fn fresh_node(
        &self,
        bodies: &dyn BodySource,
        declaration: Declaration,
        sig: &crate::registry::FunctionReflection,
    ) -> Option<Node> {
        if sig.is_stub {
            return None;
        }
        let returns = bodies.summary(&sig.provenance)?.fresh.as_deref()?;
        let mut calls = Vec::with_capacity(returns.calls.len());
        for call in returns.calls.iter() {
            calls.push(self.fresh_call(call)?);
        }
        Some(Node { declaration, calls })
    }

    /// The declaration of the program that `call` reaches.
    fn fresh_call(&self, call: &FreshCall) -> Option<Declaration> {
        match call {
            FreshCall::Function(name) => {
                let (declared, sig) = self.resolve_function(name)?;
                if sig.is_stub {
                    return None;
                }
                let (id, _, _) = self.function(declared)?;
                Some(Declaration::Function(id))
            }
            FreshCall::Static(class, method) => {
                let (class_name, _) = self.lookup_class(class.resolved_class()?)?;
                let (owner, sig) = self.lookup_method_with_owner(class_name, method)?;
                if sig.is_stub || !self.method_is_static(&owner, method) {
                    return None;
                }
                let class_id = *self.class_ids.get(&owner)?;
                let slot = self.member_slot(&owner, method)?;
                Some(Declaration::Method(MethodRef::new(class_id, slot)))
            }
        }
    }

    /// Whether the function a call names gives back only objects it made.
    pub(crate) fn function_returns_fresh(&self, callee: &QualifiedName) -> bool {
        self.fresh_call(&FreshCall::Function(callee.clone()))
            .is_some_and(|declaration| self.declaration_is_fresh(declaration))
    }

    /// Whether the static method `class::method` gives back only objects it
    /// made.
    pub(crate) fn static_method_returns_fresh(&self, class: &QualifiedName, method: &Name) -> bool {
        self.fresh_call(&FreshCall::Static(class.clone(), method.clone()))
            .is_some_and(|declaration| self.declaration_is_fresh(declaration))
    }
}
