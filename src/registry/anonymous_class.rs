//! Stable synthetic naming for `new class { ... }` (anonymous class)
//! expressions, so one can be indexed into the registry under a real name
//! the same way any other class is - mirrors real PHPStan's own
//! `AnonymousClassNameHelper`/`AnonymousClassVisitor` exactly
//! (`vendor/phpstan-src/src/Broker/AnonymousClassNameHelper.php`,
//! `vendor/phpstan-src/src/Parser/AnonymousClassVisitor.php`):
//! `"AnonymousClass" . md5("{file}:{line}[:{line_index}]")`, where
//! `line_index` is a 1-based, document-order position among every
//! anonymous class starting on that exact line - present only when more
//! than one shares a line, absent (not `0`) otherwise, since it changes
//! the hashed string.
//!
//! `collect_anonymous_classes` is the one place that computes these names
//! at all - both the registry entry (`BuiltDecl`) and the checker's own
//! inference site (`checker::expr::ExprKind::NewAnonymousClass`) get the
//! identical name because the checker never recomputes it: it just looks
//! its own expression's span up in the `Span -> name` map this also
//! returns, populated once, here, at parse time. This sidesteps the two
//! independent-computation-sites-must-agree risk a design that
//! recomputed the name at both places would carry.
//!
//! Unlike `registry::build_declarations` (top-level declarations only),
//! this walks every function/method/closure/arrow-fn body too - an
//! anonymous class can appear anywhere an expression can.

use std::collections::HashMap;

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::registry::{BuiltDecl, ClassInfo, NameContext};
use crate::token::Span;

pub(crate) fn synthetic_name(
    relative_file: &str,
    start_line: u32,
    line_index: Option<u32>,
) -> String {
    let hashed = match line_index {
        Some(i) => format!("{relative_file}:{start_line}:{i}"),
        None => format!("{relative_file}:{start_line}"),
    };
    format!("AnonymousClass{:x}", md5::compute(hashed.as_bytes()))
}

/// See the module doc comment. `relative_file` is whatever string this
/// project already uses to identify the file elsewhere (the same `Path`
/// `Project::check_file`'s own caller prints diagnostics against) - only
/// needs to be stable per file across one analysis run, not to match real
/// PHPStan's own path convention exactly, since nothing compares the two.
pub(crate) fn collect_anonymous_classes(
    stmts: &[Stmt],
    relative_file: &str,
) -> (Vec<BuiltDecl>, HashMap<Span, String>) {
    let ctx = NameContext::build(stmts);
    let mut found: Vec<(&Expr, &ClassDecl)> = Vec::new();
    for stmt in stmts {
        walk_stmt(stmt, &mut found);
    }

    let mut per_line: HashMap<u32, u32> = HashMap::new();
    for (expr, _) in &found {
        *per_line.entry(expr.span.start.line).or_insert(0) += 1;
    }

    let mut seen_on_line: HashMap<u32, u32> = HashMap::new();
    let mut decls = Vec::new();
    let mut names = HashMap::new();
    for (expr, decl) in found {
        let line = expr.span.start.line;
        let line_index = if per_line[&line] > 1 {
            let counter = seen_on_line.entry(line).or_insert(0);
            *counter += 1;
            Some(*counter)
        } else {
            None
        };
        let name = synthetic_name(relative_file, line, line_index);
        names.insert(expr.span, name.clone());
        let info = ClassInfo::from_decl(decl, &ctx);
        // Not `intern()`: every anonymous class in the whole corpus gets
        // its own unique md5-hashed name (file+line+index), so there is
        // no repeated text here to deduplicate - only a permanent,
        // never-evicted interner-table entry to avoid.
        decls.push(BuiltDecl::Class {
            name: std::sync::Arc::from(name),
            conditional: false,
            info,
        });
    }
    (decls, names)
}

fn walk_stmt<'a>(stmt: &'a Stmt, found: &mut Vec<(&'a Expr, &'a ClassDecl)>) {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => walk_expr(e, found),
        StmtKind::Return(Some(e)) => walk_expr(e, found),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                walk_expr(e, found);
            }
        }
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => {
            walk_expr(cond, found);
        }
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                walk_expr(e, found);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            walk_expr(subject, found);
            if let Some(k) = key {
                walk_expr(k, found);
            }
            walk_expr(value, found);
        }
        StmtKind::FunctionDecl(f) => walk_function(f, found),
        StmtKind::ClassDecl(c) => walk_class(c, found),
        StmtKind::EnumDecl(e) => walk_enum(e, found),
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                walk_expr(&c.value, found);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    walk_expr(e, found);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            walk_expr(subject, found);
            for case in cases {
                if let Some(t) = &case.test {
                    walk_expr(t, found);
                }
            }
        }
        _ => {}
    }
    for child in stmt_children(stmt) {
        walk_stmt(child, found);
    }
}

fn walk_function<'a>(f: &'a FunctionDecl, found: &mut Vec<(&'a Expr, &'a ClassDecl)>) {
    for p in &f.params {
        if let Some(d) = &p.default {
            walk_expr(d, found);
        }
        if let Some(hooks) = &p.hooks {
            walk_property_hooks(hooks, found);
        }
    }
    for s in &f.body {
        walk_stmt(s, found);
    }
}

fn walk_class<'a>(c: &'a ClassDecl, found: &mut Vec<(&'a Expr, &'a ClassDecl)>) {
    for p in &c.properties {
        if let Some(d) = &p.default {
            walk_expr(d, found);
        }
        if let Some(hooks) = &p.hooks {
            walk_property_hooks(hooks, found);
        }
    }
    for m in &c.methods {
        walk_function(&m.function, found);
    }
    for cst in &c.consts {
        walk_expr(&cst.value, found);
    }
}

fn walk_enum<'a>(e: &'a EnumDecl, found: &mut Vec<(&'a Expr, &'a ClassDecl)>) {
    for m in &e.methods {
        walk_function(&m.function, found);
    }
    for cst in &e.consts {
        walk_expr(&cst.value, found);
    }
    for case in &e.cases {
        if let Some(v) = &case.value {
            walk_expr(v, found);
        }
    }
}

fn walk_property_hooks<'a>(hooks: &'a PropertyHooks, found: &mut Vec<(&'a Expr, &'a ClassDecl)>) {
    if let Some(body) = &hooks.get {
        walk_hook_body(body, found);
    }
    if let Some((param, body)) = &hooks.set {
        if let Some(p) = param
            && let Some(d) = &p.default
        {
            walk_expr(d, found);
        }
        walk_hook_body(body, found);
    }
}

fn walk_hook_body<'a>(body: &'a PropertyHookBody, found: &mut Vec<(&'a Expr, &'a ClassDecl)>) {
    match body {
        PropertyHookBody::Arrow(e) => walk_expr(e, found),
        PropertyHookBody::Block(stmts) => {
            for s in stmts {
                walk_stmt(s, found);
            }
        }
        PropertyHookBody::Abstract => {}
    }
}

fn walk_expr<'a>(expr: &'a Expr, found: &mut Vec<(&'a Expr, &'a ClassDecl)>) {
    match &expr.kind {
        ExprKind::NewAnonymousClass(decl, args) => {
            found.push((expr, decl));
            for a in args {
                walk_expr(&a.value, found);
            }
            walk_class(decl, found);
        }
        ExprKind::Closure { params, body, .. } => {
            for p in params {
                if let Some(d) = &p.default {
                    walk_expr(d, found);
                }
            }
            for s in body {
                walk_stmt(s, found);
            }
        }
        ExprKind::ArrowFn { body, .. } => walk_expr(body, found),
        _ => {}
    }
    for child in expr_children(expr) {
        walk_expr(child, found);
    }
}
