//! Which parameters of a function may be invoked by a call, given the values
//! its arguments are known to have.
//!
//! A parameter that is never invoked cannot run a closure passed as it, so
//! the closure's writes do not happen during the call. The walk binds each
//! parameter to its index, follows the values literal arguments carry through
//! the body, and takes only the branches those values let run. A parameter
//! read anywhere it may still be read is invoked; so is one passed to a
//! function the file does not declare, or to one that may invoke it.
//!
//! The same walk gives the branches of a closure body that a call's literal
//! arguments make dead, so its call-mode check reports nothing from them.

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{
    Arg, BinOp, CatchClause, ClosureExpr, ClosureUse, Expr, ExprKind, FunctionDecl, GlobalVar,
    IfStmt, Literal, NativeType, Param, Stmt, StmtKind, TypeHint, TypeHintKind, UnaryOp,
};
use syntax::span::Span;
use syntax::visit::{self, Visitor};

use super::closure_flow::is_dynamic_function;
use super::global_writes::key;

/// How many same-file calls deep a summary follows. A call past it is
/// assumed to invoke every parameter it is passed.
const DEPTH_LIMIT: usize = 16;

/// A value PHP is known to hold: a literal, or an operation on known values.
/// Floats are not modelled, and neither is a value a parameter's declared
/// type would coerce.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Known {
    Null,
    Bool(bool),
    Int(i64),
    Str(Vec<u8>),
}

/// What a variable holds at a point of the walk.
#[derive(Clone, Default, PartialEq)]
struct Binding {
    /// The parameters, by index, the variable may still hold unchanged.
    params: Vec<usize>,
    /// The value the variable holds, when it is known.
    value: Option<Known>,
    /// Whether a write from outside the walk may change the variable: a
    /// `global`, a `static`, or a by-reference capture. A call may make such a
    /// write, so the value is not kept across one.
    shared: bool,
}

type Env = FxHashMap<String, Binding>;

/// The functions of a program, for the summaries of their calls.
#[derive(Default)]
pub(crate) struct Invocations {
    /// Each declared function, by its `global_writes::key`. A name declared
    /// twice maps to `None`: a call to it could be either.
    functions: FxHashMap<String, Option<Rc<FunctionDecl>>>,
    /// How many distinct keys end in each unqualified name. A call whose
    /// unqualified name is shared by two declarations may resolve to either.
    short_names: FxHashMap<String, usize>,
    summaries: RefCell<FxHashMap<Call, Rc<Vec<bool>>>>,
    /// The calls whose summaries are being computed, for cycle detection.
    active: RefCell<Vec<Call>>,
}

/// A call as a summary is keyed: the function's `global_writes::key` and the
/// values of its arguments.
type Call = (String, Vec<Option<Known>>);

impl Invocations {
    pub(crate) fn new(program: &[Stmt]) -> Invocations {
        let mut collect = Collect::default();
        for stmt in program {
            collect.visit_stmt(stmt);
        }
        let mut short_names = FxHashMap::default();
        for name in collect.functions.keys() {
            let short = name.rsplit('\\').next().unwrap_or(name.as_str());
            *short_names.entry(short.to_string()).or_insert(0) += 1;
        }
        Invocations {
            functions: collect.functions,
            short_names,
            ..Invocations::default()
        }
    }

    /// The declaration a call of `name` calls, when the file declares exactly
    /// one function it can be.
    fn declaration(&self, name: &str) -> Option<(String, Rc<FunctionDecl>)> {
        let name = key(name);
        let short = name.rsplit('\\').next().unwrap_or(name.as_str());
        if self.short_names.get(short).copied().unwrap_or(0) > 1 {
            return None;
        }
        let decl = self.functions.get(&name)?.clone()?;
        Some((name, decl))
    }

    /// For a call of the function `name` with arguments of values `values`
    /// (`None` where unknown), which of its parameters may be invoked. `None`
    /// when no single function of the file is the callee.
    pub(crate) fn invoked_by_call(
        &self,
        name: &str,
        values: &[Option<Known>],
    ) -> Option<Rc<Vec<bool>>> {
        let (name, decl) = self.declaration(name)?;
        let mut values = values.to_vec();
        values.resize(decl.params.len(), None);
        Some(self.summary(name, &decl, values))
    }

    /// The values the arguments of a call have, as far as literals and
    /// operations on them tell. Nothing when an argument is spread or named,
    /// since then the arguments do not map to parameters by position.
    pub(crate) fn argument_values(&self, args: &[Arg]) -> Vec<Option<Known>> {
        if args.iter().any(|arg| arg.spread || arg.name.is_some()) {
            return vec![None; args.len()];
        }
        let mut walk = Walk::new(self, 0);
        let mut env = Env::default();
        args.iter()
            .map(|arg| walk.expr(&arg.value, &mut env))
            .collect()
    }

    /// The spans of the branches of `closure`'s body that cannot run when its
    /// parameters hold `values`. Nothing when the body names variables
    /// dynamically, since a write by name can change any of them.
    pub(crate) fn dead_branches(
        &self,
        closure: &ClosureExpr,
        values: &[Option<Known>],
    ) -> Vec<Span> {
        if has_dynamic_names(&closure.body) {
            return Vec::new();
        }
        let mut walk = Walk::new(self, closure.params.len());
        let env = walk.entry(&closure.params, values, &closure.uses);
        walk.stmts(&closure.body, env);
        walk.dead
    }

    fn summary(
        &self,
        name: String,
        decl: &FunctionDecl,
        values: Vec<Option<Known>>,
    ) -> Rc<Vec<bool>> {
        let id = (name, values);
        if let Some(found) = self.summaries.borrow().get(&id) {
            return Rc::clone(found);
        }
        let cyclic =
            self.active.borrow().contains(&id) || self.active.borrow().len() >= DEPTH_LIMIT;
        if cyclic {
            return Rc::new(vec![true; decl.params.len()]);
        }
        self.active.borrow_mut().push(id.clone());
        let invoked = Rc::new(self.analyse(decl, &id.1));
        self.active.borrow_mut().pop();
        self.summaries.borrow_mut().insert(id, Rc::clone(&invoked));
        invoked
    }

    fn analyse(&self, decl: &FunctionDecl, values: &[Option<Known>]) -> Vec<bool> {
        if has_dynamic_names(decl.body_stmts()) {
            return vec![true; decl.params.len()];
        }
        let mut walk = Walk::new(self, decl.params.len());
        let env = walk.entry(&decl.params, values, &[]);
        walk.stmts(decl.body_stmts(), env);
        walk.invoked
    }
}

/// The functions a program declares, at any depth outside another function's
/// body or class.
#[derive(Default)]
struct Collect {
    functions: FxHashMap<String, Option<Rc<FunctionDecl>>>,
}

impl<'ast> Visitor<'ast> for Collect {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let StmtKind::FunctionDecl(function) = &stmt.kind {
            let name = key(function.fq_name());
            let decl = Rc::new((**function).clone());
            self.functions
                .entry(name)
                .and_modify(|slot| *slot = None)
                .or_insert(Some(decl));
        }
        visit::walk_stmt(self, stmt);
    }
}

/// One walk over a body: the parameters it may invoke, and the branches its
/// known values leave dead.
struct Walk<'a> {
    invocations: &'a Invocations,
    invoked: Vec<bool>,
    dead: Vec<Span>,
}

impl<'a> Walk<'a> {
    fn new(invocations: &'a Invocations, params: usize) -> Walk<'a> {
        Walk {
            invocations,
            invoked: vec![false; params],
            dead: Vec::new(),
        }
    }

    /// The environment a body starts in: each parameter bound to its index,
    /// with the value its argument has when it fits the declared type. A
    /// by-reference or variadic parameter is invoked outright, and a by-value
    /// capture of the closure's `uses` holds a copy the body cannot change.
    fn entry(&mut self, params: &[Param], values: &[Option<Known>], uses: &[ClosureUse]) -> Env {
        let mut env = Env::default();
        for (index, param) in params.iter().enumerate() {
            if param.by_ref || param.variadic {
                self.invoked[index] = true;
                continue;
            }
            let value = values
                .get(index)
                .cloned()
                .flatten()
                .filter(|value| fits(param.type_hint.as_ref(), value));
            env.insert(
                param.name.name.as_str().to_string(),
                Binding {
                    params: vec![index],
                    value,
                    shared: false,
                },
            );
        }
        for closure_use in uses.iter().filter(|u| u.by_ref) {
            env.entry(closure_use.name.name.as_str().to_string())
                .or_default()
                .shared = true;
        }
        env
    }

    fn stmts(&mut self, stmts: &[Stmt], mut env: Env) -> Option<Env> {
        for stmt in stmts {
            env = self.stmt(stmt, env)?;
        }
        Some(env)
    }

    /// Walks `stmt`, returning the environment after it, or `None` when no
    /// path leaves it.
    fn stmt(&mut self, stmt: &Stmt, mut env: Env) -> Option<Env> {
        match &stmt.kind {
            StmtKind::InlineHtml(_)
            | StmtKind::Namespace(_)
            | StmtKind::Use(_)
            | StmtKind::FunctionDecl(_)
            | StmtKind::ClassDecl(_) => Some(env),
            StmtKind::Expression(expr) => match &expr.kind {
                ExprKind::Exit(_, args) => {
                    for arg in args.iter().flatten() {
                        self.expr(&arg.value, &mut env);
                    }
                    None
                }
                ExprKind::Throw(thrown) => {
                    self.expr(thrown, &mut env);
                    None
                }
                _ => {
                    self.expr(expr, &mut env);
                    Some(env)
                }
            },
            StmtKind::Echo(exprs) => {
                for expr in exprs {
                    self.expr(expr, &mut env);
                }
                Some(env)
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(value, &mut env);
                }
                None
            }
            StmtKind::Throw(thrown) => {
                self.expr(thrown, &mut env);
                None
            }
            StmtKind::Break(_) | StmtKind::Continue(_) => None,
            StmtKind::Block(stmts) => self.stmts(stmts, env),
            StmtKind::If(if_stmt) => self.if_stmt(if_stmt, env),
            StmtKind::While { cond, body } => {
                self.havoc(&mentions_stmt(stmt), &mut env);
                match self.expr(cond, &mut env).map(|value| truth(&value)) {
                    Some(false) => self.dead.push(body.span),
                    _ => {
                        self.stmt(body, env.clone());
                    }
                }
                Some(env)
            }
            StmtKind::DoWhile { body, cond } => {
                self.havoc(&mentions_stmt(stmt), &mut env);
                self.stmt(body, env.clone());
                self.expr(cond, &mut env);
                Some(env)
            }
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                for expr in init {
                    self.expr(expr, &mut env);
                }
                self.havoc(&mentions_stmt(stmt), &mut env);
                for expr in cond {
                    self.expr(expr, &mut env);
                }
                self.stmt(body, env.clone());
                for expr in step {
                    self.expr(expr, &mut env);
                }
                Some(env)
            }
            StmtKind::Foreach {
                subject,
                value,
                by_ref,
                body,
                ..
            } => {
                self.expr(subject, &mut env);
                self.havoc(&mentions_stmt(stmt), &mut env);
                if *by_ref {
                    self.share(&mentions_expr(value), &mut env);
                }
                self.stmt(body, env.clone());
                Some(env)
            }
            StmtKind::Declare { body, .. } => match body {
                Some(stmts) => self.stmts(stmts, env),
                None => Some(env),
            },
            StmtKind::Global(vars) => {
                for var in vars {
                    if let GlobalVar::Name(ident) = var {
                        env.insert(
                            ident.name.as_str().to_string(),
                            Binding {
                                shared: true,
                                ..Binding::default()
                            },
                        );
                    }
                }
                Some(env)
            }
            StmtKind::StaticVar(vars) => {
                for (ident, init) in vars {
                    if let Some(init) = init {
                        self.expr(init, &mut env);
                    }
                    env.insert(
                        ident.name.as_str().to_string(),
                        Binding {
                            shared: true,
                            ..Binding::default()
                        },
                    );
                }
                Some(env)
            }
            StmtKind::Unset(targets) => {
                for target in targets {
                    match &target.kind {
                        ExprKind::Variable(name) => {
                            env.remove(name.as_str());
                        }
                        _ => {
                            self.expr(target, &mut env);
                        }
                    }
                }
                Some(env)
            }
            StmtKind::Switch { subject, cases } => {
                self.expr(subject, &mut env);
                self.havoc(&mentions_stmt(stmt), &mut env);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.expr(test, &mut env.clone());
                    }
                    self.stmts(&case.body, env.clone());
                }
                Some(env)
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.havoc(&mentions_stmt(stmt), &mut env);
                self.stmts(body, env.clone());
                for clause in catches {
                    self.stmts(&clause.body, env.clone());
                }
                if let Some(finally) = finally {
                    self.stmts(finally, env.clone());
                }
                Some(env)
            }
            // Any other statement is taken as a block of unmodelled reads
            // and writes.
            _ => {
                self.havoc(&mentions_stmt(stmt), &mut env);
                self.forget_shared(&mut env);
                Some(env)
            }
        }
    }

    fn if_stmt(&mut self, stmt: &IfStmt, env: Env) -> Option<Env> {
        let mut branches: Vec<(&Expr, &Stmt)> = vec![(&stmt.cond, stmt.then_branch.as_ref())];
        branches.extend(
            stmt.elseifs
                .iter()
                .map(|elseif| (&elseif.cond, elseif.body.as_ref())),
        );
        let mut outs = None;
        let mut reached = Some(env);
        for (cond, body) in branches {
            let Some(mut env) = reached.take() else {
                self.dead.push(body.span);
                continue;
            };
            match self.expr(cond, &mut env).map(|value| truth(&value)) {
                Some(true) => outs = join(outs, self.stmt(body, env)),
                Some(false) => {
                    self.dead.push(body.span);
                    reached = Some(env);
                }
                None => {
                    outs = join(outs, self.stmt(body, env.clone()));
                    reached = Some(env);
                }
            }
        }
        match reached {
            Some(env) => {
                let rest = match &stmt.else_branch {
                    Some(branch) => self.stmt(branch, env),
                    None => Some(env),
                };
                join(outs, rest)
            }
            None => {
                if let Some(branch) = &stmt.else_branch {
                    self.dead.push(branch.span);
                }
                outs
            }
        }
    }

    /// Walks `expr`, returning its value when it is known, and marks the
    /// parameters it may invoke.
    fn expr(&mut self, expr: &Expr, env: &mut Env) -> Option<Known> {
        match &expr.kind {
            ExprKind::Literal(literal) => literal_value(literal),
            ExprKind::Variable(name) => self.read(name.as_str(), env),
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => self.expr(inner, env),
            ExprKind::Assign(lhs, rhs) => self.assign(lhs, rhs, env),
            ExprKind::AssignRef(lhs, rhs) => {
                let mut names = mentions_expr(lhs);
                names.extend(mentions_expr(rhs));
                self.share(&names, env);
                None
            }
            ExprKind::CompoundAssign(_, lhs, rhs) => {
                self.expr(rhs, env);
                self.havoc(&mentions_expr(lhs), env);
                None
            }
            ExprKind::Unary(op, operand) => self.unary(op, operand, env),
            ExprKind::Binary(op, left, right) => self.binary(*op, left, right, env),
            ExprKind::Ternary(cond, then, otherwise) => {
                self.ternary(cond, then.as_deref(), otherwise, env)
            }
            ExprKind::Call(callee, args) => self.call(callee, args, env),
            ExprKind::Closure(closure) => {
                for closure_use in &closure.uses {
                    let name = closure_use.name.name.as_str();
                    if closure_use.by_ref {
                        self.share(&FxHashSet::from_iter([name.to_string()]), env);
                    } else {
                        self.read(name, env);
                    }
                }
                None
            }
            _ => {
                self.havoc(&mentions_expr(expr), env);
                self.forget_shared(env);
                None
            }
        }
    }

    fn assign(&mut self, lhs: &Expr, rhs: &Expr, env: &mut Env) -> Option<Known> {
        match &lhs.kind {
            ExprKind::Variable(name) => {
                let value = self.expr(rhs, env);
                let shared = env.get(name.as_str()).is_some_and(|binding| binding.shared);
                env.insert(
                    name.as_str().to_string(),
                    Binding {
                        params: Vec::new(),
                        value: value.clone(),
                        shared,
                    },
                );
                value
            }
            _ => {
                self.expr(rhs, env);
                self.havoc(&mentions_expr(lhs), env);
                None
            }
        }
    }

    fn unary(&mut self, op: &UnaryOp, operand: &Expr, env: &mut Env) -> Option<Known> {
        match op {
            UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec => {
                self.havoc(&mentions_expr(operand), env);
                None
            }
            UnaryOp::Not => self
                .expr(operand, env)
                .map(|value| Known::Bool(!truth(&value))),
            UnaryOp::Neg => match self.expr(operand, env)? {
                Known::Int(int) => int.checked_neg().map(Known::Int),
                _ => None,
            },
            UnaryOp::Plus | UnaryOp::BitNot => {
                self.expr(operand, env);
                None
            }
        }
    }

    fn binary(&mut self, op: BinOp, left: &Expr, right: &Expr, env: &mut Env) -> Option<Known> {
        match op {
            BinOp::And | BinOp::LogicalAnd => match self.expr(left, env).map(|v| truth(&v)) {
                Some(false) => Some(Known::Bool(false)),
                Some(true) => self
                    .expr(right, env)
                    .map(|value| Known::Bool(truth(&value))),
                None => {
                    self.conditionally(right, env);
                    None
                }
            },
            BinOp::Or | BinOp::LogicalOr => match self.expr(left, env).map(|v| truth(&v)) {
                Some(true) => Some(Known::Bool(true)),
                Some(false) => self
                    .expr(right, env)
                    .map(|value| Known::Bool(truth(&value))),
                None => {
                    self.conditionally(right, env);
                    None
                }
            },
            BinOp::Coalesce => match self.expr(left, env) {
                Some(Known::Null) => self.expr(right, env),
                Some(value) => Some(value),
                None => {
                    self.conditionally(right, env);
                    None
                }
            },
            _ => {
                let left = self.expr(left, env);
                let right = self.expr(right, env);
                match (left, right) {
                    (Some(left), Some(right)) => compute(op, &left, &right),
                    _ => None,
                }
            }
        }
    }

    /// Walks `expr` as one that may not run: the environment is the join of
    /// the one before it and the one after.
    fn conditionally(&mut self, expr: &Expr, env: &mut Env) {
        let before = env.clone();
        self.expr(expr, env);
        *env = merge(&before, env);
    }

    fn ternary(
        &mut self,
        cond: &Expr,
        then: Option<&Expr>,
        otherwise: &Expr,
        env: &mut Env,
    ) -> Option<Known> {
        let value = self.expr(cond, env);
        match value.as_ref().map(truth) {
            Some(true) => match then {
                Some(then) => self.expr(then, env),
                None => value,
            },
            Some(false) => self.expr(otherwise, env),
            None => {
                let mut yes = env.clone();
                let yes_value = match then {
                    Some(then) => self.expr(then, &mut yes),
                    None => None,
                };
                let no_value = self.expr(otherwise, env);
                *env = merge(&yes, env);
                if yes_value.is_some() && yes_value == no_value {
                    yes_value
                } else {
                    None
                }
            }
        }
    }

    /// A call. A call of a function the file declares passes each argument
    /// that is a variable to that function's summary: the parameters it holds
    /// are invoked only when the function may invoke the argument. Any other
    /// call invokes every argument.
    fn call(&mut self, callee: &Expr, args: &[Arg], env: &mut Env) -> Option<Known> {
        let function = match &callee.kind {
            ExprKind::Name(name) => Some(name.written.as_str()),
            _ => {
                self.expr(callee, env);
                None
            }
        };
        let mut values = Vec::with_capacity(args.len());
        let mut holds: Vec<Vec<usize>> = Vec::with_capacity(args.len());
        for arg in args {
            match &arg.value.kind {
                ExprKind::Variable(name) => {
                    let binding = env.get(name.as_str());
                    values.push(binding.and_then(|binding| binding.value.clone()));
                    holds.push(
                        binding
                            .map(|binding| binding.params.clone())
                            .unwrap_or_default(),
                    );
                }
                _ => {
                    values.push(self.expr(&arg.value, env));
                    holds.push(Vec::new());
                }
            }
        }
        let plain = args.iter().all(|arg| !arg.spread && arg.name.is_none());
        let summary = match function {
            Some(name) if plain => self.invocations.invoked_by_call(name, &values),
            _ => None,
        };
        for (index, arg) in args.iter().enumerate() {
            let invoked = summary
                .as_ref()
                .and_then(|summary| summary.get(index))
                .copied()
                .unwrap_or(true);
            if !invoked {
                continue;
            }
            self.mark(&holds[index]);
            if let ExprKind::Variable(name) = &arg.value.kind {
                self.havoc_name(name.as_str(), env);
            }
        }
        self.forget_shared(env);
        None
    }

    /// A read of a variable: any parameter it may hold is invoked, since the
    /// read may pass it on or call it.
    fn read(&mut self, name: &str, env: &Env) -> Option<Known> {
        let binding = env.get(name)?;
        self.mark(&binding.params);
        binding.value.clone()
    }

    fn mark(&mut self, params: &[usize]) {
        for &index in params {
            self.invoked[index] = true;
        }
    }

    /// A variable the walk loses track of: a parameter it may hold is invoked,
    /// and its value is forgotten.
    fn havoc(&mut self, names: &FxHashSet<String>, env: &mut Env) {
        for name in names {
            self.havoc_name(name, env);
        }
    }

    fn havoc_name(&mut self, name: &str, env: &mut Env) {
        if let Some(binding) = env.get_mut(name) {
            let params = std::mem::take(&mut binding.params);
            self.mark(&params);
            binding.value = None;
        }
    }

    /// Havocs `names`, and records that a write from outside the walk may
    /// change them from here on.
    fn share(&mut self, names: &FxHashSet<String>, env: &mut Env) {
        self.havoc(names, env);
        for name in names {
            env.entry(name.clone()).or_default().shared = true;
        }
    }

    /// Forgets the value of every shared variable, since a call may have
    /// written it.
    fn forget_shared(&mut self, env: &mut Env) {
        for binding in env.values_mut() {
            if binding.shared {
                binding.value = None;
            }
        }
    }
}

/// The environment after either of two paths: each variable keeps a parameter
/// that either path may hand it, and a value only when both agree on it.
fn merge(left: &Env, right: &Env) -> Env {
    let mut merged = Env::default();
    for name in left.keys().chain(right.keys()) {
        if merged.contains_key(name) {
            continue;
        }
        let left = left.get(name).cloned().unwrap_or_default();
        let right = right.get(name).cloned().unwrap_or_default();
        let mut params = left.params.clone();
        params.extend(right.params.iter().copied());
        params.sort_unstable();
        params.dedup();
        merged.insert(
            name.clone(),
            Binding {
                params,
                value: if left.value == right.value {
                    left.value
                } else {
                    None
                },
                shared: left.shared || right.shared,
            },
        );
    }
    merged
}

/// The join of two paths, where `None` is a path that does not reach the
/// point.
fn join(left: Option<Env>, right: Option<Env>) -> Option<Env> {
    match (left, right) {
        (Some(left), Some(right)) => Some(merge(&left, &right)),
        (one, None) | (None, one) => one,
    }
}

fn literal_value(literal: &Literal) -> Option<Known> {
    Some(match literal {
        Literal::Int(int) => Known::Int(*int),
        Literal::String(string) => Known::Str(string.as_bytes().to_vec()),
        Literal::Bool(flag) => Known::Bool(*flag),
        Literal::Null => Known::Null,
        Literal::Float(_) => return None,
    })
}

/// Whether `value` is true in a condition.
fn truth(value: &Known) -> bool {
    match value {
        Known::Null => false,
        Known::Bool(flag) => *flag,
        Known::Int(int) => *int != 0,
        Known::Str(string) => !(string.is_empty() || string.as_slice() == b"0"),
    }
}

/// The value of a binary operator on two known values, when it is known.
fn compute(op: BinOp, left: &Known, right: &Known) -> Option<Known> {
    Some(match op {
        BinOp::Identical => Known::Bool(left == right),
        BinOp::NotIdentical => Known::Bool(left != right),
        BinOp::Eq => Known::Bool(loosely_equal(left, right)?),
        BinOp::NotEq => Known::Bool(!loosely_equal(left, right)?),
        BinOp::Add => Known::Int(int(left)?.checked_add(int(right)?)?),
        BinOp::Sub => Known::Int(int(left)?.checked_sub(int(right)?)?),
        BinOp::LogicalXor => Known::Bool(truth(left) != truth(right)),
        _ => return None,
    })
}

fn int(value: &Known) -> Option<i64> {
    match value {
        Known::Int(int) => Some(*int),
        _ => None,
    }
}

/// `==` where it is decided without the string-to-number rules.
fn loosely_equal(left: &Known, right: &Known) -> Option<bool> {
    match (left, right) {
        (Known::Int(left), Known::Int(right)) => Some(left == right),
        (Known::Bool(left), Known::Bool(right)) => Some(left == right),
        (Known::Null, Known::Null) => Some(true),
        (Known::Null, Known::Bool(flag)) | (Known::Bool(flag), Known::Null) => Some(!flag),
        (Known::Str(left), Known::Str(right)) if left == right => Some(true),
        _ => None,
    }
}

/// Whether a value passed for a parameter of `hint` is the value it arrives
/// as. A value the type would coerce does not fit.
fn fits(hint: Option<&TypeHint>, value: &Known) -> bool {
    let Some(hint) = hint else {
        return true;
    };
    match &hint.kind {
        TypeHintKind::Keyword(native, _) => matches!(
            (native, value),
            (NativeType::Mixed, _)
                | (NativeType::Int, Known::Int(_))
                | (NativeType::String, Known::Str(_))
                | (NativeType::Bool, Known::Bool(_))
                | (NativeType::Null, Known::Null)
                | (NativeType::True, Known::Bool(true))
                | (NativeType::False, Known::Bool(false))
        ),
        TypeHintKind::Nullable(inner) => {
            matches!(value, Known::Null) || fits(Some(inner.as_ref()), value)
        }
        TypeHintKind::Union(members) => members.iter().any(|member| fits(Some(member), value)),
        _ => false,
    }
}

/// The variable names an expression mentions: each variable, each `use`, and
/// each `global` or `static` name. A plain closure's body is not walked, since
/// it has its own variables.
#[derive(Default)]
struct Mentions {
    names: FxHashSet<String>,
}

impl Mentions {
    fn insert(&mut self, name: &str) {
        self.names.insert(name.to_string());
    }
}

impl<'ast> Visitor<'ast> for Mentions {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            StmtKind::Global(vars) => {
                for var in vars {
                    if let GlobalVar::Name(ident) = var {
                        self.insert(ident.name.as_str());
                    }
                }
                visit::walk_stmt(self, stmt);
            }
            StmtKind::StaticVar(vars) => {
                for (ident, _) in vars {
                    self.insert(ident.name.as_str());
                }
                visit::walk_stmt(self, stmt);
            }
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Variable(name) => self.insert(name.as_str()),
            ExprKind::Closure(closure) => {
                for closure_use in &closure.uses {
                    self.insert(closure_use.name.name.as_str());
                }
            }
            _ => visit::walk_expr(self, expr),
        }
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        if let Some(var) = &clause.var {
            self.insert(var.name.as_str());
        }
        visit::walk_catch_clause(self, clause);
    }
}

fn mentions_expr(expr: &Expr) -> FxHashSet<String> {
    let mut mentions = Mentions::default();
    mentions.visit_expr(expr);
    mentions.names
}

fn mentions_stmt(stmt: &Stmt) -> FxHashSet<String> {
    let mut mentions = Mentions::default();
    mentions.visit_stmt(stmt);
    mentions.names
}

/// Whether a body names variables in a way the walk cannot follow: a variable
/// variable, `eval`, `include`, a call to `compact` or `extract`, `$GLOBALS`,
/// `goto`, or a `global` by name. A closure or arrow function has its own
/// variables, so its body is not looked into.
struct Dynamic {
    found: bool,
}

impl<'ast> Visitor<'ast> for Dynamic {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            StmtKind::Goto(_) | StmtKind::Label(_) => self.found = true,
            StmtKind::Global(vars)
                if vars.iter().any(|var| matches!(var, GlobalVar::Dynamic(_))) =>
            {
                self.found = true;
            }
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
            ExprKind::DynamicVariable(_) | ExprKind::Eval(_) | ExprKind::Include(..) => {
                self.found = true;
            }
            ExprKind::Variable(name) if name.as_str() == "GLOBALS" => self.found = true,
            ExprKind::Call(callee, _) => {
                if let ExprKind::Name(name) = &callee.kind
                    && (is_dynamic_function(name.written.as_str())
                        || ["func_get_args", "func_get_arg"]
                            .iter()
                            .any(|f| name.written.as_str().eq_ignore_ascii_case(f)))
                {
                    self.found = true;
                }
                visit::walk_expr(self, expr);
            }
            _ => visit::walk_expr(self, expr),
        }
    }
}

fn has_dynamic_names(body: &[Stmt]) -> bool {
    let mut dynamic = Dynamic { found: false };
    for stmt in body {
        dynamic.visit_stmt(stmt);
    }
    dynamic.found
}
