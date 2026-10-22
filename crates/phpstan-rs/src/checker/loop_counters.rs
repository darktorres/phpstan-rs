//! The counters of a loop: variables its condition, steps and body write
//! only by adding or subtracting literals. A process runs a bounded number
//! of iterations (`PlatformLimits::loop_iterations`), and each moves a
//! counter by no more than the sum of its steps, so a counter that starts
//! in a bounded range stays within that many steps of it.

use crate::registry::{FunctionReflection, Registry};
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::visit::{self, Visitor};

use super::assignment::assigned_part_of_variable;
use super::stmt::assigned_variable_names;

/// How far one iteration can move a counter: up and down, and whether
/// each step runs at most once an iteration (none in a nested loop, and
/// no `goto` in the loop).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct Steps {
    pub(crate) up: i64,
    pub(crate) down: i64,
    pub(crate) once: bool,
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
        scan.counters()
    }

    /// The counters of a function body: its statics written only by
    /// steps. A call outside a loop isn't logged, so a variable passed to
    /// a call is written unless the call is to a function `registry`
    /// declares with that parameter by value.
    pub(crate) fn of_function_body(stmts: &[Stmt], registry: &Registry) -> LoopCounters {
        let mut scan = Scan {
            args_by_ref: Some(registry),
            function_body: true,
            ..Scan::default()
        };
        visit::walk_stmts(&mut scan, stmts);
        scan.counters()
    }
}

/// The band a counter starting in `start` stays in over `iterations`
/// iterations of `steps` each, when that band and one step past it fit in
/// the `int` range.
pub(crate) fn counter_band(
    (lo, hi): (i64, i64),
    steps: Steps,
    iterations: i64,
) -> Option<(i64, i64)> {
    let band_lo = lo.checked_sub(iterations.checked_mul(steps.down)?)?;
    let band_hi = hi.checked_add(iterations.checked_mul(steps.up)?)?;
    band_lo.checked_sub(steps.down)?;
    band_hi.checked_add(steps.up)?;
    Some((band_lo, band_hi))
}

impl Scan<'_> {
    fn counters(self) -> LoopCounters {
        if self.anything {
            return LoopCounters::default();
        }
        let Scan {
            steps,
            other,
            repeated,
            jumps,
            ..
        } = self;
        LoopCounters(
            steps
                .into_iter()
                .filter(|(name, _)| !other.contains(name))
                .map(|(name, steps)| {
                    let once = !jumps && !repeated.contains(&name);
                    (name, Steps { once, ..steps })
                })
                .collect(),
        )
    }
}

#[derive(Default)]
struct Scan<'r> {
    steps: FxHashMap<String, Steps>,
    /// Variables written some other way.
    other: FxHashSet<String>,
    /// Variables stepped inside a nested loop.
    repeated: FxHashSet<String>,
    /// How many loops the scan is inside, past the scanned one.
    nested: u32,
    /// Whether the scan met a `goto` or a label, which can run a step
    /// again in the same iteration.
    jumps: bool,
    /// Whether any variable may be written in a way the scan can't name
    /// (`$$name`, `$GLOBALS`, `extract()`, `include`, `eval`).
    anything: bool,
    /// Set when a variable passed to a call may be written by reference
    /// unless the registry's function takes it by value.
    args_by_ref: Option<&'r Registry>,
    /// Whether the scan is of a function body, where a `static`
    /// statement declares its variables (its initializer runs once)
    /// rather than rebinding them.
    function_body: bool,
}

impl Scan<'_> {
    fn step(&mut self, name: &str, by: i64) {
        if self.nested > 0 {
            self.repeated.insert(name.to_string());
        }
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

    /// A bare variable passed to `sig` where it takes its argument by
    /// reference (or by a name or spread it may) is written.
    fn passed_to(&mut self, sig: &FunctionReflection, args: &[Arg]) {
        let params = sig.params();
        for (i, arg) in args.iter().enumerate() {
            let ExprKind::Variable(name) = &arg.value.peeled().kind else {
                continue;
            };
            let param = match &arg.name {
                _ if arg.spread => None,
                Some(label) => params.iter().find(|p| p.name().as_ref() == label.as_ref()),
                None => params
                    .get(i)
                    .or_else(|| params.last().filter(|p| p.variadic())),
            };
            if param.is_none_or(|p| p.by_ref()) {
                self.other.insert(name.to_string());
            }
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

impl<'ast> Visitor<'ast> for Scan<'_> {
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
            StmtKind::StaticVar(vars) if !self.function_body => {
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
            StmtKind::Goto(_) | StmtKind::Label(_) => self.jumps = true,
            _ => {}
        }
        let nested = matches!(
            stmt.kind,
            StmtKind::While { .. }
                | StmtKind::DoWhile { .. }
                | StmtKind::For { .. }
                | StmtKind::Foreach { .. }
        );
        self.nested += u32::from(nested);
        visit::walk_stmt(self, stmt);
        self.nested -= u32::from(nested);
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        if let Some(var) = &clause.var {
            self.other.insert(var.to_string());
        }
        visit::walk_catch_clause(self, clause);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Some(registry) = self.args_by_ref {
            match &expr.kind {
                ExprKind::Call(callee, args) => match &callee.peeled().kind {
                    ExprKind::Name(name) => match registry.resolve_function(name) {
                        Some((_, sig)) => self.passed_to(sig, args),
                        None => self.passed_to_unknown_callee(args),
                    },
                    _ => self.passed_to_unknown_callee(args),
                },
                ExprKind::New(_, args)
                | ExprKind::MethodCall(_, _, args, _)
                | ExprKind::StaticCall(_, _, args) => self.passed_to_unknown_callee(args),
                ExprKind::NewAnonymousClass(class) => self.passed_to_unknown_callee(&class.args),
                _ => {}
            }
        }
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
