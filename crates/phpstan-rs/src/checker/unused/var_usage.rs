use rustc_hash::FxHashSet;
use std::sync::Arc;

use syntax::ast::*;
use syntax::visit::{child_exprs, child_stmts};

// ---------------------------------------------------------------------
// Variable-name usage (constructor parameters / closure `use(...)`)
// ---------------------------------------------------------------------

/// The result of walking one function/closure body for "which variable
/// names does it reference anywhere" - any occurrence (read, write, or a
/// nested closure's own `use(...)` capture of the same name), not just a
/// provably-read one: a false "unused" report is treated as much worse
/// than a missed one, and a parameter that's merely reassigned without
/// ever being read again is a subtler bug than an unused-parameter check
/// is meant to catch.
pub(crate) enum VarUsage {
    Names(FxHashSet<Arc<str>>),
    /// Something that reads variables by a name this walk can't resolve
    /// statically was seen anywhere in the body - `func_get_args()`,
    /// `compact(...)` with a non-literal argument, `get_defined_vars()`,
    /// `include`/`require` (which can define arbitrary new variables in
    /// the current scope, real PHP's own reason `UnusedFunctionParametersCheck`
    /// treats them the same way), or a `${expr}`/`$$x` dynamic variable
    /// whose name isn't a literal string. Every candidate parameter/use
    /// should be treated as used once this is set - this project's own
    /// `extract()`/`eval()` detection already discards a whole function
    /// body's diagnostics outright (`Checker::saw_dynamic_var_construct`),
    /// so those two aren't duplicated here.
    Escaped,
}

impl VarUsage {
    pub(crate) fn contains(&self, name: &str) -> bool {
        match self {
            VarUsage::Names(names) => names.contains(name),
            VarUsage::Escaped => true,
        }
    }
}

pub(crate) fn collect_var_usage(stmts: &[Stmt]) -> VarUsage {
    let mut names = FxHashSet::default();
    let mut escaped = false;
    for stmt in stmts {
        walk_stmt_for_vars(stmt, &mut names, &mut escaped);
    }
    if escaped {
        VarUsage::Escaped
    } else {
        VarUsage::Names(names)
    }
}

fn is_call_to(callee: &Expr, target: &str) -> bool {
    matches!(&callee.kind, ExprKind::Name(n) if n.trim_start_matches('\\').eq_ignore_ascii_case(target))
}

fn walk_stmt_for_vars(stmt: &Stmt, names: &mut FxHashSet<Arc<str>>, escaped: &mut bool) {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => walk_expr_for_vars(e, names, escaped),
        StmtKind::Return(Some(e)) => walk_expr_for_vars(e, names, escaped),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                walk_expr_for_vars(e, names, escaped);
            }
        }
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => walk_expr_for_vars(cond, names, escaped),
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                walk_expr_for_vars(e, names, escaped);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            walk_expr_for_vars(subject, names, escaped);
            if let Some(k) = key {
                walk_expr_for_vars(k, names, escaped);
            }
            walk_expr_for_vars(value, names, escaped);
        }
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                walk_expr_for_vars(&c.value, names, escaped);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    walk_expr_for_vars(e, names, escaped);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            walk_expr_for_vars(subject, names, escaped);
            for case in cases {
                if let Some(t) = &case.test {
                    walk_expr_for_vars(t, names, escaped);
                }
            }
        }
        _ => {}
    }
    for child in child_stmts(stmt) {
        walk_stmt_for_vars(child, names, escaped);
    }
}

fn walk_expr_for_vars(expr: &Expr, names: &mut FxHashSet<Arc<str>>, escaped: &mut bool) {
    match &expr.kind {
        ExprKind::Variable(name) => {
            if **name != *"this" {
                names.insert(name.clone());
            }
        }
        ExprKind::DynamicVariable(inner) => {
            if let ExprKind::Literal(Literal::String(s)) = &inner.kind {
                names.insert(s.clone());
            } else {
                *escaped = true;
            }
        }
        ExprKind::Call(callee, args) => {
            if is_call_to(callee, "func_get_args")
                || is_call_to(callee, "get_defined_vars")
                || is_call_to(callee, "include")
                || is_call_to(callee, "include_once")
                || is_call_to(callee, "require")
                || is_call_to(callee, "require_once")
                || is_call_to(callee, "eval")
            {
                *escaped = true;
            } else if is_call_to(callee, "compact") {
                for arg in args {
                    collect_compact_names(&arg.value, names, escaped);
                }
            }
        }
        // Mirrors real PHPStan's own `UnusedFunctionParametersCheck::
        // getUsedVariables`: only a closure's own `use(...)` list counts
        // as a reference to the *enclosing* scope's variable of the same
        // name - its `params`/`body` have their own, independent scope,
        // and naively recursing into them risks reading a same-named but
        // unrelated inner parameter/local as if it were a use of the
        // outer one. `child_exprs` already excludes `Closure`'s
        // children entirely (see `visit`'s own module doc comment), so
        // this is the only place its variables are ever collected from.
        ExprKind::Closure { uses, .. } => {
            for u in uses {
                names.insert(u.name.clone());
            }
        }
        // Unlike `Closure`, an arrow function implicitly captures every
        // outer variable it references by value - so, unlike above, its
        // `params`/`body` genuinely do need a normal recursive walk here.
        // `child_exprs` excludes `ArrowFn`'s children too, so this is
        // the only place they're walked from.
        ExprKind::ArrowFn { params, body, .. } => {
            for p in params {
                if let Some(d) = &p.default {
                    walk_expr_for_vars(d, names, escaped);
                }
            }
            walk_expr_for_vars(body, names, escaped);
        }
        // Real PHP anonymous classes don't implicitly capture the
        // enclosing scope's variables at all - only the constructor
        // arguments (evaluated in the enclosing scope before being passed
        // in) can reference an outer variable. `child_exprs` excludes
        // `NewAnonymousClass`'s children entirely, so the constructor args
        // need an explicit walk here; the class's own method bodies
        // deliberately aren't recursed into (a separate, unrelated scope).
        ExprKind::NewAnonymousClass(_, args) => {
            for a in args {
                walk_expr_for_vars(&a.value, names, escaped);
            }
        }
        _ => {}
    }
    for child in child_exprs(expr) {
        walk_expr_for_vars(child, names, escaped);
    }
}

/// `compact('a', 'b')` / `compact(['a', 'b'])` reads the named variables
/// by string, dynamically - a literal string (nested one level into an
/// array literal, PHP allows both forms) resolves to a real, precise name;
/// anything else (a variable holding the name, a function call, `...$x`
/// spread, ...) has no static name to resolve here, so it escalates to the
/// same blanket "everything is used" fallback as `func_get_args()`/
/// `get_defined_vars()` rather than risk a false "unused" report.
fn collect_compact_names(e: &Expr, names: &mut FxHashSet<Arc<str>>, escaped: &mut bool) {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => {
            names.insert(s.clone());
        }
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                if item.spread {
                    *escaped = true;
                    continue;
                }
                collect_compact_names(&item.value, names, escaped);
            }
        }
        _ => *escaped = true,
    }
}
