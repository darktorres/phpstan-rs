use std::collections::HashSet;

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::registry::NameContext;

// ---------------------------------------------------------------------
// `Closure::bind`/`->bindTo` rebind targets (see `Checker::
// rebind_target_classes`'s own doc comment)
// ---------------------------------------------------------------------

/// A whole-program pre-pass (unlike every other walker in this module,
/// which stops at a nested function/class declaration - this one needs
/// to see into every one of them, since a `Closure::bind` call reaching
/// into class `Foo`'s private members can appear in a completely
/// unrelated top-level function, nowhere near `Foo`'s own declaration).
/// Returns the set of classes a literal `Foo::class` scope argument
/// names, plus a coarser "give up entirely" flag for a bind/bindTo call
/// whose scope this project can't resolve without real type inference
/// (an omitted scope, or an arbitrary expression).
pub(crate) fn scan_program_for_rebind_targets(
    program: &[Stmt],
    ctx: &NameContext,
) -> (HashSet<String>, bool) {
    let mut targets = HashSet::new();
    let mut escape_all = false;
    for stmt in program {
        scan_stmt_for_rebinds(stmt, ctx, &mut targets, &mut escape_all);
    }
    (targets, escape_all)
}

fn scan_stmts_for_rebinds(
    body: &[Stmt],
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    for s in body {
        scan_stmt_for_rebinds(s, ctx, targets, escape_all);
    }
}

fn scan_stmt_for_rebinds(
    stmt: &Stmt,
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => {
            scan_expr_for_rebinds(e, ctx, targets, escape_all)
        }
        StmtKind::Return(Some(e)) => scan_expr_for_rebinds(e, ctx, targets, escape_all),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                scan_expr_for_rebinds(e, ctx, targets, escape_all);
            }
        }
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => scan_expr_for_rebinds(cond, ctx, targets, escape_all),
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                scan_expr_for_rebinds(e, ctx, targets, escape_all);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            scan_expr_for_rebinds(subject, ctx, targets, escape_all);
            if let Some(k) = key {
                scan_expr_for_rebinds(k, ctx, targets, escape_all);
            }
            scan_expr_for_rebinds(value, ctx, targets, escape_all);
        }
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                scan_expr_for_rebinds(&c.value, ctx, targets, escape_all);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    scan_expr_for_rebinds(e, ctx, targets, escape_all);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            scan_expr_for_rebinds(subject, ctx, targets, escape_all);
            for case in cases {
                if let Some(t) = &case.test {
                    scan_expr_for_rebinds(t, ctx, targets, escape_all);
                }
            }
        }
        // Unlike every other walker in this module, these three ARE
        // recursed into here - a nested declaration can still contain a
        // `Closure::bind`/`bindTo` call reaching into some other class
        // entirely.
        StmtKind::FunctionDecl(f) => scan_stmts_for_rebinds(&f.body, ctx, targets, escape_all),
        StmtKind::ClassDecl(c) => {
            for m in &c.methods {
                scan_stmts_for_rebinds(&m.function.body, ctx, targets, escape_all);
            }
            for p in &c.properties {
                if let Some(d) = &p.default {
                    scan_expr_for_rebinds(d, ctx, targets, escape_all);
                }
            }
            for cst in &c.consts {
                scan_expr_for_rebinds(&cst.value, ctx, targets, escape_all);
            }
        }
        StmtKind::EnumDecl(e) => {
            for m in &e.methods {
                scan_stmts_for_rebinds(&m.function.body, ctx, targets, escape_all);
            }
            for cst in &e.consts {
                scan_expr_for_rebinds(&cst.value, ctx, targets, escape_all);
            }
        }
        _ => {}
    }
    for child in stmt_children(stmt) {
        scan_stmt_for_rebinds(child, ctx, targets, escape_all);
    }
}

fn scan_expr_for_rebinds(
    expr: &Expr,
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    match &expr.kind {
        // `Closure::bind($closure, $newThis, $scope)` - real, idiomatic
        // scope-widening (see `checker/expr.rs`'s own handling of the
        // identical call, which this mirrors).
        ExprKind::StaticCall(class, method, args) if class == "Closure" && method == "bind" => {
            record_rebind_scope(args.get(2), ctx, targets, escape_all);
        }
        // `$closure->bindTo($newThis, $scope)` - same idea, on the
        // receiver instead of the first argument, so the scope is
        // argument index 1, not 2.
        ExprKind::MethodCall(_, method, args, _) if **method == *"bindTo" => {
            record_rebind_scope(args.get(1), ctx, targets, escape_all);
        }
        // `expr_children` deliberately excludes these three (see
        // `ast_walk`'s own module doc comment) - walked explicitly here
        // since, unlike this module's other walkers, a rebind call can
        // legitimately live inside any one of them.
        ExprKind::Closure { body, .. } => scan_stmts_for_rebinds(body, ctx, targets, escape_all),
        ExprKind::ArrowFn { body, .. } => scan_expr_for_rebinds(body, ctx, targets, escape_all),
        ExprKind::NewAnonymousClass(decl, args) => {
            for m in &decl.methods {
                scan_stmts_for_rebinds(&m.function.body, ctx, targets, escape_all);
            }
            for a in args {
                scan_expr_for_rebinds(&a.value, ctx, targets, escape_all);
            }
        }
        _ => {}
    }
    for child in expr_children(expr) {
        scan_expr_for_rebinds(child, ctx, targets, escape_all);
    }
}

fn record_rebind_scope(
    scope_arg: Option<&Arg>,
    ctx: &NameContext,
    targets: &mut HashSet<String>,
    escape_all: &mut bool,
) {
    match scope_arg.map(|a| &a.value.kind) {
        Some(ExprKind::StaticAccess(class_name, member)) if member == "class" => {
            targets.insert(ctx.resolve(class_name));
        }
        // An omitted scope argument, or one that isn't a literal
        // `Foo::class` this project can resolve without real type
        // inference (a variable, `get_class($x)`, ...) - can't rule out
        // it reaches an arbitrary class, so every class's private-member
        // check is skipped for the rest of this file.
        _ => *escape_all = true,
    }
}
