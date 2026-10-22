//! The counters of a loop: variables its condition, steps and body write
//! only by adding or subtracting literals. A process runs a bounded number
//! of iterations (`PlatformLimits::loop_iterations`), and each moves a
//! counter by no more than the sum of its steps, so a counter that starts
//! in a bounded range stays within that many steps of it.

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::visit::{self, Visitor};

use super::assignment::assigned_part_of_variable;
use super::stmt::assigned_variable_names;

/// How far one iteration can move a counter: up and down.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct Steps {
    pub(crate) up: i64,
    pub(crate) down: i64,
}

/// A loop's counters, by name.
#[derive(Debug, Default)]
pub(crate) struct LoopCounters(FxHashMap<String, Steps>);

impl LoopCounters {
    pub(crate) fn get(&self, name: &str) -> Option<Steps> {
        self.0.get(name).copied()
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// The counters of a loop that writes `targets` each iteration (a
    /// `foreach`'s key and value), and runs these expressions (its
    /// condition and steps) and statements (its body). A by-reference
    /// write through a call is not seen here; the checker logs those as it
    /// runs the body.
    pub(crate) fn of(targets: &[&Expr], exprs: &[&Expr], stmts: &[&Stmt]) -> LoopCounters {
        let mut scan = Scan::default();
        for target in targets {
            scan.write(target);
        }
        for e in exprs {
            scan.visit_expr(e);
        }
        for s in stmts {
            scan.visit_stmt(s);
        }
        if scan.anything {
            return LoopCounters::default();
        }
        let Scan { steps, other, .. } = scan;
        LoopCounters(
            steps
                .into_iter()
                .filter(|(name, _)| !other.contains(name))
                .collect(),
        )
    }
}

#[derive(Default)]
struct Scan {
    steps: FxHashMap<String, Steps>,
    /// Variables written some other way.
    other: FxHashSet<String>,
    /// Whether any variable may be written in a way the scan can't name
    /// (`$$name`, `$GLOBALS`, `extract()`, `include`, `eval`).
    anything: bool,
}

impl Scan {
    fn step(&mut self, name: &str, by: i64) {
        let steps = self.steps.entry(name.to_string()).or_default();
        if by >= 0 {
            steps.up = steps.up.saturating_add(by);
        } else {
            steps.down = steps.down.saturating_add(by.saturating_neg());
        }
    }

    fn write(&mut self, target: &Expr) {
        if let Some(name) = assigned_part_of_variable(target) {
            self.other.insert(name.to_string());
        }
        self.other.extend(assigned_variable_names(target));
        if super::assignment::writes_dynamic_variable(target) {
            self.anything = true;
        }
    }

    /// A bare variable passed to a call whose parameters aren't known
    /// here may be written by reference.
    fn passed_to_unknown_callee(&mut self, args: &[Arg]) {
        for arg in args {
            if let ExprKind::Variable(name) = &arg.value.peeled().kind {
                self.other.insert(name.to_string());
            }
        }
    }
}

/// The literal an expression is, `-` applied.
fn literal_int(e: &Expr) -> Option<i64> {
    match &e.peeled().kind {
        ExprKind::Literal(Literal::Int(n)) => Some(*n),
        ExprKind::Unary(UnaryOp::Neg, inner) => literal_int(inner)?.checked_neg(),
        _ => None,
    }
}

fn variable_name(e: &Expr) -> Option<&str> {
    match &e.peeled().kind {
        ExprKind::Variable(name) => Some(name.as_ref()),
        _ => None,
    }
}

/// The step `$name op k` takes `$name` by, for `+` and `-` with a literal.
fn step_of(name: &str, value: &Expr) -> Option<i64> {
    let ExprKind::Binary(op, lhs, rhs) = &value.peeled().kind else {
        return None;
    };
    match op {
        BinOp::Add if variable_name(lhs) == Some(name) => literal_int(rhs),
        BinOp::Add if variable_name(rhs) == Some(name) => literal_int(lhs),
        BinOp::Sub if variable_name(lhs) == Some(name) => literal_int(rhs)?.checked_neg(),
        _ => None,
    }
}

impl<'ast> Visitor<'ast> for Scan {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Global(vars) => {
                for var in vars {
                    match var {
                        GlobalVar::Name(name) => {
                            self.other.insert(name.to_string());
                        }
                        GlobalVar::Dynamic(_) => self.anything = true,
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                self.other
                    .extend(vars.iter().map(|(name, _)| name.to_string()));
            }
            StmtKind::Unset(targets) => {
                for target in targets {
                    self.write(target);
                }
            }
            StmtKind::Foreach { key, value, .. } => {
                if let Some(key) = key {
                    self.write(key);
                }
                self.write(value);
            }
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => return,
            _ => {}
        }
        visit::walk_stmt(self, stmt);
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        if let Some(var) = &clause.var {
            self.other.insert(var.to_string());
        }
        visit::walk_catch_clause(self, clause);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Unary(op, target) => {
                let by = match op {
                    UnaryOp::PreInc | UnaryOp::PostInc => Some(1),
                    UnaryOp::PreDec | UnaryOp::PostDec => Some(-1),
                    _ => None,
                };
                if let Some(by) = by {
                    match variable_name(target) {
                        Some(name) => self.step(name, by),
                        None => self.write(target),
                    }
                }
            }
            ExprKind::CompoundAssign(op, target, value) => {
                let by = match op {
                    BinOp::Add => literal_int(value),
                    BinOp::Sub => literal_int(value).and_then(i64::checked_neg),
                    _ => None,
                };
                match (variable_name(target), by) {
                    (Some(name), Some(by)) => self.step(name, by),
                    _ => self.write(target),
                }
            }
            ExprKind::Assign(target, value) => {
                match variable_name(target).and_then(|name| Some((name, step_of(name, value)?))) {
                    Some((name, by)) => self.step(name, by),
                    None => self.write(target),
                }
            }
            ExprKind::AssignRef(target, source) => {
                self.write(target);
                self.write(source);
            }
            ExprKind::DynamicVariable(_) | ExprKind::Eval(_) | ExprKind::Include(..) => {
                self.anything = true;
            }
            ExprKind::Variable(name) if name.as_ref() == "GLOBALS" => self.anything = true,
            ExprKind::Call(callee, args) => match &callee.peeled().kind {
                ExprKind::Name(name) => {
                    let (name, fallback) = name.candidates();
                    if std::iter::once(name)
                        .chain(fallback)
                        .any(|n| n.trim_start_matches('\\').eq_ignore_ascii_case("extract"))
                    {
                        self.anything = true;
                    }
                }
                _ => self.passed_to_unknown_callee(args),
            },
            ExprKind::DynamicMethodCall(_, _, args, _)
            | ExprKind::DynamicStaticCall(_, _, args) => {
                self.passed_to_unknown_callee(args);
            }
            ExprKind::Closure(closure) => {
                for captured in closure.uses.iter().filter(|u| u.by_ref) {
                    self.other.insert(captured.name.to_string());
                }
                return;
            }
            ExprKind::ArrowFn(_) => return,
            ExprKind::NewAnonymousClass(class) => {
                visit::walk_args(self, &class.args);
                return;
            }
            _ => {}
        }
        visit::walk_expr(self, expr);
    }
}
