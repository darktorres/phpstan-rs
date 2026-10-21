//! Which parameters and globals a same-file function invokes on every path,
//! before anything else it could observe happens.
//!
//! A callable the function invokes that way runs during the call, in the state
//! the call sees: the statements before the invocation do nothing observable,
//! so they do not change what it sees. A closure passed as such an argument, or
//! held in such a global, is therefore checked at the call.

use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{
    Arg, BinOp, CatchClause, Expr, ExprKind, FunctionDecl, GlobalVar, Literal, MethodDecl, Param,
    Stmt, StmtKind, UnaryOp,
};
use syntax::visit::{self, Visitor};

use super::closure_flow::is_dynamic_function;

/// The built-in functions that call a callable they are given: a name passed
/// to one of them, computed at run time, can run a same-file function from
/// inside another function.
const CALLABLE_TAKERS: &[&str] = &[
    "array_filter",
    "array_map",
    "array_reduce",
    "array_walk",
    "array_walk_recursive",
    "call_user_func",
    "call_user_func_array",
    "forward_static_call",
    "forward_static_call_array",
    "iterator_apply",
    "ob_start",
    "preg_replace_callback",
    "preg_replace_callback_array",
    "register_shutdown_function",
    "set_error_handler",
    "set_exception_handler",
    "spl_autoload_register",
    "uasort",
    "uksort",
    "usort",
];

/// The name a function is looked up by: its last segment, lowercased.
pub(crate) fn short_name(name: &str) -> String {
    name.rsplit('\\')
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

/// What the same-file functions invoke on every call.
#[derive(Default)]
pub(crate) struct MustInvoke {
    /// Each function's parameters, by short name: whether it invokes each.
    parameters: FxHashMap<String, Vec<bool>>,
    /// Each function's globals it invokes on every call, by short name. Only
    /// functions every call of which is at the top level have any.
    globals: FxHashMap<String, Vec<String>>,
    /// The globals every declaration of which is in a function that invokes
    /// them: no other code reads or writes them.
    covered: FxHashSet<String>,
}

impl MustInvoke {
    pub(crate) fn new(
        program: &[Stmt],
        functions: &FxHashMap<String, Option<Rc<FunctionDecl>>>,
    ) -> MustInvoke {
        let mut counts: FxHashMap<String, usize> = FxHashMap::default();
        for name in functions.keys() {
            *counts.entry(short_name(name)).or_insert(0) += 1;
        }
        let top_only = top_level_calls_only(program);
        let dynamic = names_dynamically(program);
        let declared = count_global_declarations(program);
        let mut out = MustInvoke::default();
        let mut covered_declarations: FxHashMap<String, usize> = FxHashMap::default();
        for (name, decl) in functions {
            let Some(decl) = decl else { continue };
            let short = short_name(name);
            if counts.get(&short) != Some(&1) {
                continue;
            }
            let body = decl.body_stmts();
            let params: Vec<String> = decl
                .params
                .iter()
                .map(|param| param.name.name.as_str().to_string())
                .collect();
            let parameters: Vec<bool> = params
                .iter()
                .map(|param| invokes_first(body, param, &params))
                .collect();
            if parameters.contains(&true) {
                out.parameters.insert(short.clone(), parameters);
            }
            if dynamic || !top_only.contains(&short) {
                continue;
            }
            let invoked: Vec<String> = declared_globals(body)
                .into_iter()
                .filter(|global| invokes_first(body, global, &[]) && only_invoked(body, global))
                .collect();
            for global in &invoked {
                *covered_declarations.entry(global.clone()).or_insert(0) += 1;
            }
            if !invoked.is_empty() {
                let mut globals = invoked;
                globals.sort();
                globals.dedup();
                out.globals.insert(short, globals);
            }
        }
        if !dynamic {
            out.covered = declared
                .into_iter()
                .filter(|(global, count)| covered_declarations.get(global) == Some(count))
                .map(|(global, _)| global)
                .collect();
        }
        out
    }

    /// Which parameters of the function `name` it invokes on every path, when
    /// the file declares it once.
    pub(crate) fn parameters(&self, name: &str) -> Option<&[bool]> {
        self.parameters.get(&short_name(name)).map(Vec::as_slice)
    }

    /// The globals the function `name` invokes on every call, when every call
    /// of it is at the top level.
    pub(crate) fn globals(&self, name: &str) -> &[String] {
        self.globals
            .get(&short_name(name))
            .map_or(&[], Vec::as_slice)
    }

    /// Whether every declaration of the global `name` is in a function that
    /// invokes it, so no other code reads or writes it.
    pub(crate) fn covers_global(&self, name: &str) -> bool {
        self.covered.contains(name)
    }
}

/// Whether `body` invokes the callable `name` on every path, before anything
/// the body does could be observed. `defined` are the variables already set
/// when the body starts. A stmt that may leave the body first stops the search.
fn invokes_first(body: &[Stmt], name: &str, defined: &[String]) -> bool {
    let mut defined = defined.to_vec();
    for stmt in body {
        if invokes_in(stmt, name, &defined) {
            return true;
        }
        if !inert(stmt, &defined) || leaves(stmt) {
            return false;
        }
        if let StmtKind::Global(vars) = &stmt.kind {
            defined.extend(vars.iter().filter_map(global_name));
        }
    }
    false
}

/// Whether `stmt` invokes `name` on every path through it, with nothing before
/// the invocation that could be observed.
fn invokes_in(stmt: &Stmt, name: &str, defined: &[String]) -> bool {
    match &stmt.kind {
        StmtKind::Expression(expr) => is_invocation(expr, name, defined),
        StmtKind::Block(body) => invokes_first(body, name, defined),
        StmtKind::If(branches) => {
            inert_expr(&branches.cond, defined)
                && branches
                    .elseifs
                    .iter()
                    .all(|elseif| inert_expr(&elseif.cond, defined))
                && invokes_in(&branches.then_branch, name, defined)
                && branches
                    .elseifs
                    .iter()
                    .all(|elseif| invokes_in(&elseif.body, name, defined))
                && branches
                    .else_branch
                    .as_deref()
                    .is_some_and(|other| invokes_in(other, name, defined))
        }
        // The finally block runs on every way out of the try, so it alone
        // decides, once the try's own body and catches have no effect.
        StmtKind::Try {
            body,
            catches,
            finally: Some(finally),
        } => {
            inert_all(body, defined)
                && catches
                    .iter()
                    .all(|catch| catch.var.is_none() && inert_all(&catch.body, defined))
                && invokes_first(finally, name, defined)
        }
        _ => false,
    }
}

/// Whether `expr` is a call of the variable `name` whose arguments are inert.
fn is_invocation(expr: &Expr, name: &str, defined: &[String]) -> bool {
    let ExprKind::Call(callee, args) = &expr.peeled().kind else {
        return false;
    };
    matches!(&callee.peeled().kind, ExprKind::Variable(variable) if variable.as_str() == name)
        && args
            .iter()
            .all(|arg| !arg.spread && inert_expr(&arg.value, defined))
}

/// Whether running `stmt` changes nothing a call could observe, and can neither
/// warn nor throw: no call, assignment, dynamic name or other effect, and no
/// leaving the body except by `return` or `throw` of an inert value.
fn inert(stmt: &Stmt, defined: &[String]) -> bool {
    match &stmt.kind {
        StmtKind::Expression(expr) => inert_expr(expr, defined),
        StmtKind::Return(value) => value
            .as_ref()
            .is_none_or(|value| inert_expr(value, defined)),
        StmtKind::Throw(expr) => inert_expr(expr, defined),
        StmtKind::Break(_) | StmtKind::Continue(_) => true,
        StmtKind::Block(body) => inert_all(body, defined),
        StmtKind::If(branches) => {
            inert_expr(&branches.cond, defined)
                && branches
                    .elseifs
                    .iter()
                    .all(|elseif| inert_expr(&elseif.cond, defined) && inert(&elseif.body, defined))
                && inert(&branches.then_branch, defined)
                && branches
                    .else_branch
                    .as_deref()
                    .is_none_or(|other| inert(other, defined))
        }
        StmtKind::Global(vars) => vars.iter().all(|var| matches!(var, GlobalVar::Name(_))),
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            inert_all(body, defined)
                && catches
                    .iter()
                    .all(|catch| catch.var.is_none() && inert_all(&catch.body, defined))
                && finally
                    .as_deref()
                    .is_none_or(|finally| inert_all(finally, defined))
        }
        _ => false,
    }
}

fn inert_all(body: &[Stmt], defined: &[String]) -> bool {
    body.iter().all(|stmt| inert(stmt, defined))
}

/// Whether evaluating `expr` changes nothing a call could observe, and can
/// neither warn nor throw. Only literals, variables already set, and operators
/// that never convert a value qualify: a read of an unset variable warns, and a
/// warning can run a user error handler; a concatenation or an index can run
/// `__toString` or `offsetGet`.
fn inert_expr(expr: &Expr, defined: &[String]) -> bool {
    match &expr.kind {
        ExprKind::Literal(_) => true,
        ExprKind::Variable(name) => defined.iter().any(|set| set == name.as_str()),
        ExprKind::Grouping(inner) | ExprKind::Unary(UnaryOp::Not, inner) => {
            inert_expr(inner, defined)
        }
        ExprKind::Binary(op, left, right) => {
            matches!(
                op,
                BinOp::Identical
                    | BinOp::NotIdentical
                    | BinOp::And
                    | BinOp::Or
                    | BinOp::LogicalAnd
                    | BinOp::LogicalOr
                    | BinOp::LogicalXor
            ) && inert_expr(left, defined)
                && inert_expr(right, defined)
        }
        ExprKind::Ternary(cond, then, otherwise) => {
            inert_expr(cond, defined)
                && then.as_deref().is_none_or(|then| inert_expr(then, defined))
                && inert_expr(otherwise, defined)
        }
        _ => false,
    }
}

/// The name a `global` statement's variable imports, or `None` for a computed
/// one.
fn global_name(var: &GlobalVar) -> Option<String> {
    match var {
        GlobalVar::Name(ident) => Some(ident.name.as_str().to_string()),
        GlobalVar::Dynamic(_) => None,
    }
}

/// Whether `stmt` may leave the body: a `return`, `throw`, `break`, `continue`
/// or `goto` anywhere in it, outside any nested function or closure.
fn leaves(stmt: &Stmt) -> bool {
    let mut finder = Leaves { found: false };
    finder.visit_stmt(stmt);
    finder.found
}

struct Leaves {
    found: bool,
}

impl<'ast> Visitor<'ast> for Leaves {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Return(_)
            | StmtKind::Throw(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Goto(_) => self.found = true,
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
            _ => visit::walk_expr(self, expr),
        }
    }
}

/// Whether the body uses the variable `name` only as the callee of a call:
/// no other read, write, reference, capture, or static declaration of it.
fn only_invoked(body: &[Stmt], name: &str) -> bool {
    let mut finder = OnlyInvoked { name, ok: true };
    for stmt in body {
        finder.visit_stmt(stmt);
    }
    finder.ok
}

struct OnlyInvoked<'n> {
    name: &'n str,
    ok: bool,
}

impl<'ast> Visitor<'ast> for OnlyInvoked<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::StaticVar(vars)
                if vars
                    .iter()
                    .any(|(ident, _)| ident.name.as_str() == self.name) =>
            {
                self.ok = false;
            }
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Call(callee, args) = &expr.kind
            && matches!(&callee.peeled().kind, ExprKind::Variable(variable) if variable.as_str() == self.name)
        {
            for arg in args {
                self.visit_expr(&arg.value);
            }
            return;
        }
        match &expr.kind {
            ExprKind::Variable(variable) if variable.as_str() == self.name => self.ok = false,
            ExprKind::Closure(closure)
                if closure
                    .uses
                    .iter()
                    .any(|closure_use| closure_use.name.name.as_str() == self.name) =>
            {
                self.ok = false;
            }
            _ => {}
        }
        visit::walk_expr(self, expr);
    }
}

/// The names a function's own body declares `global`, each as often as it
/// does. A nested function, class or closure has its own.
fn declared_globals(body: &[Stmt]) -> Vec<String> {
    let mut finder = Declared::default();
    for stmt in body {
        finder.visit_stmt(stmt);
    }
    finder.names
}

#[derive(Default)]
struct Declared {
    names: Vec<String>,
}

impl<'ast> Visitor<'ast> for Declared {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Global(vars) => self.names.extend(vars.iter().filter_map(global_name)),
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
            _ => visit::walk_expr(self, expr),
        }
    }
}

/// How many `global` declarations of each name the whole program holds.
fn count_global_declarations(program: &[Stmt]) -> FxHashMap<String, usize> {
    let mut counter = Declarations::default();
    for stmt in program {
        counter.visit_stmt(stmt);
    }
    counter.counts
}

#[derive(Default)]
struct Declarations {
    counts: FxHashMap<String, usize>,
}

impl<'ast> Visitor<'ast> for Declarations {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let StmtKind::Global(vars) = &stmt.kind {
            for name in vars.iter().filter_map(global_name) {
                *self.counts.entry(name).or_insert(0) += 1;
            }
        }
        visit::walk_stmt(self, stmt);
    }
}

/// Whether the program names variables in a way no walk can follow, in any
/// scope: a variable variable, `eval`, `include`, `$GLOBALS`, a computed
/// `global`, `goto`, or a call of `compact`, `extract` or `func_get_args`.
fn names_dynamically(program: &[Stmt]) -> bool {
    let mut finder = Dynamic { found: false };
    for stmt in program {
        finder.visit_stmt(stmt);
    }
    finder.found
}

struct Dynamic {
    found: bool,
}

impl<'ast> Visitor<'ast> for Dynamic {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Goto(_) | StmtKind::Label(_) => self.found = true,
            StmtKind::Global(vars) if vars.iter().any(|var| global_name(var).is_none()) => {
                self.found = true;
            }
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::DynamicVariable(_) | ExprKind::Eval(_) | ExprKind::Include(..) => {
                self.found = true;
            }
            ExprKind::Variable(name) if name.as_str() == "GLOBALS" => self.found = true,
            ExprKind::Call(callee, _) => {
                if let ExprKind::Name(name) = &callee.kind
                    && (is_dynamic_function(name.written.as_str())
                        || name.written.as_str().eq_ignore_ascii_case("func_get_args")
                        || name.written.as_str().eq_ignore_ascii_case("func_get_arg"))
                {
                    self.found = true;
                }
                visit::walk_expr(self, expr);
            }
            _ => visit::walk_expr(self, expr),
        }
    }
}

/// The names some write other than assigning a closure literal reaches,
/// anywhere in the program. A name outside this set holds only closure
/// literals, or nothing, so calling it cannot run a function by a name.
fn names_written_otherwise(program: &[Stmt]) -> FxHashSet<String> {
    let mut writes = WriteSites::default();
    for stmt in program {
        writes.visit_stmt(stmt);
    }
    writes.names
}

#[derive(Default)]
struct WriteSites {
    names: FxHashSet<String>,
}

impl WriteSites {
    /// Records every variable a write target names.
    fn target(&mut self, target: &Expr) {
        let mut variables = VariableNames::default();
        variables.visit_expr(target);
        self.names.extend(variables.names);
    }
}

impl<'ast> Visitor<'ast> for WriteSites {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Foreach { key, value, .. } => {
                if let Some(key) = key {
                    self.target(key);
                }
                self.target(value);
            }
            StmtKind::StaticVar(vars) => {
                self.names.extend(
                    vars.iter()
                        .map(|(ident, _)| ident.name.as_str().to_string()),
                );
            }
            _ => {}
        }
        visit::walk_stmt(self, stmt);
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        if let Some(var) = &clause.var {
            self.names.insert(var.name.as_str().to_string());
        }
        visit::walk_catch_clause(self, clause);
    }

    fn visit_param(&mut self, param: &'ast Param) {
        self.names.insert(param.name.name.as_str().to_string());
        visit::walk_param(self, param);
    }

    fn visit_arg(&mut self, arg: &'ast Arg) {
        // A variable passed as an argument may be written by a by-reference
        // parameter.
        if let ExprKind::Variable(name) = &arg.value.kind {
            self.names.insert(name.as_str().to_string());
        }
        visit::walk_arg(self, arg);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Assign(lhs, rhs) => {
                let closure = matches!(lhs.kind, ExprKind::Variable(_))
                    && matches!(rhs.kind, ExprKind::Closure(_) | ExprKind::ArrowFn(_));
                if !closure {
                    self.target(lhs);
                }
            }
            ExprKind::AssignRef(lhs, rhs) => {
                self.target(lhs);
                self.target(rhs);
            }
            ExprKind::CompoundAssign(_, lhs, _) => self.target(lhs),
            ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                operand,
            ) => self.target(operand),
            ExprKind::Closure(closure) => {
                self.names.extend(
                    closure
                        .uses
                        .iter()
                        .filter(|closure_use| closure_use.by_ref)
                        .map(|closure_use| closure_use.name.name.as_str().to_string()),
                );
            }
            _ => {}
        }
        visit::walk_expr(self, expr);
    }
}

/// The variable names an expression mentions.
#[derive(Default)]
struct VariableNames {
    names: Vec<String>,
}

impl<'ast> Visitor<'ast> for VariableNames {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Variable(name) = &expr.kind {
            self.names.push(name.as_str().to_string());
        }
        visit::walk_expr(self, expr);
    }
}

/// The short names of the functions the program calls only at the top level:
/// never from another function, a method or a closure, never named any other
/// way, and never spelled as a string that could name them. Empty when a
/// function, method or closure calls a name it computes, since that name could
/// be any of them.
fn top_level_calls_only(program: &[Stmt]) -> FxHashSet<String> {
    let mut sites = CallSites {
        written: names_written_otherwise(program),
        ..CallSites::default()
    };
    for stmt in program {
        sites.visit_stmt(stmt);
    }
    if sites.computed {
        return FxHashSet::default();
    }
    sites
        .top
        .into_iter()
        .filter(|short| !sites.elsewhere.contains(short) && !sites.strings.contains(short))
        .collect()
}

#[derive(Default)]
struct CallSites {
    depth: usize,
    top: FxHashSet<String>,
    elsewhere: FxHashSet<String>,
    strings: FxHashSet<String>,
    /// Whether a function, method or closure calls something by a name it
    /// computes, or hands a callable to a built-in that calls it.
    computed: bool,
    /// The names some write other than a closure literal reaches.
    written: FxHashSet<String>,
}

impl CallSites {
    /// Whether each argument is a literal, a closure, or a variable that holds
    /// only closures: a callable built-in given those cannot name a function.
    fn closure_only(&self, args: &[Arg]) -> bool {
        args.iter().all(|arg| match &arg.value.kind {
            ExprKind::Literal(_) | ExprKind::Closure(_) | ExprKind::ArrowFn(_) => true,
            ExprKind::Variable(name) => !self.written.contains(name.as_str()),
            _ => false,
        })
    }
}

impl<'ast> Visitor<'ast> for CallSites {
    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        self.depth += 1;
        visit::walk_function_decl(self, function);
        self.depth -= 1;
    }

    fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
        self.depth += 1;
        visit::walk_method_decl(self, method);
        self.depth -= 1;
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {
                self.depth += 1;
                visit::walk_expr(self, expr);
                self.depth -= 1;
            }
            ExprKind::Call(callee, args) => {
                match &callee.kind {
                    ExprKind::Name(name) => {
                        let short = short_name(name.written.as_str());
                        if self.depth == 0 {
                            self.top.insert(short.clone());
                        } else {
                            if CALLABLE_TAKERS.contains(&short.as_str()) && !self.closure_only(args)
                            {
                                self.computed = true;
                            }
                            self.elsewhere.insert(short);
                        }
                    }
                    ExprKind::Closure(_) | ExprKind::ArrowFn(_) => self.visit_expr(callee),
                    ExprKind::Variable(name) => {
                        if self.depth > 0 && self.written.contains(name.as_str()) {
                            self.computed = true;
                        }
                    }
                    _ => {
                        if self.depth > 0 {
                            self.computed = true;
                        }
                        self.visit_expr(callee);
                    }
                }
                visit::walk_args(self, args);
            }
            ExprKind::Name(name) => {
                self.elsewhere.insert(short_name(name.written.as_str()));
            }
            ExprKind::Literal(Literal::String(value)) => {
                if let Some(text) = value.as_str() {
                    self.strings.insert(short_name(text));
                }
            }
            _ => visit::walk_expr(self, expr),
        }
    }
}
