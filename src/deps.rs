use std::collections::HashSet;

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};

/// Best-effort, deliberately over-approximating set of every function/class
/// name one file's parse tree could reference externally - used purely to
/// decide which *other* files might need re-checking when a symbol they
/// declare changes. Missing a real reference here would let a stale
/// diagnostic survive an edit elsewhere; collecting an extra name that
/// turns out unrelated just costs one harmless re-check, so this errs
/// toward over-collecting rather than trying to be precise.
///
/// Names are kept exactly as the AST spells them (no backslash-stripping,
/// no namespace/`use`-import resolution) - a deliberate, still-safe
/// simplification even though the registry itself is namespace-aware
/// (`registry::NameContext`): this is only ever used to decide
/// which files *might* need re-checking, never to look anything up in the
/// registry directly, so a same-named-but-different-namespace false
/// match just costs one harmless extra re-check (this function already
/// over-approximates on purpose, per the doc above) rather than a wrong
/// diagnostic. Type hints are the one exception: they're strings that get
/// resolved through `types::parse_type_string`, which does strip a
/// leading backslash, so the names pulled out of them are stripped the
/// same way to stay consistent with that.
pub fn referenced_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    for stmt in stmts {
        collect_stmt(stmt, &mut names);
    }
    names
}

pub(crate) fn is_primitive_type_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int"
            | "integer"
            | "float"
            | "double"
            | "real"
            | "string"
            | "bool"
            | "boolean"
            | "array"
            | "object"
            | "mixed"
            | "void"
            | "never"
            | "noreturn"
            | "never-return"
            | "never-returns"
            | "no-return"
            | "null"
            | "true"
            | "false"
            | "self"
            | "static"
            | "parent"
            | "callable"
            | "iterable"
    )
}

/// Pulls candidate class names out of a type-hint string (`"?Foo"`,
/// `"Foo|Bar"`, `"array<Baz>"`, `"(A&B)|C"`, ...) by splitting on anything
/// that isn't part of a bare/qualified identifier and discarding the
/// primitive type keywords - good enough for "does this file mention that
/// name anywhere", which is all this is used for.
fn collect_type_hint(hint: &str, names: &mut HashSet<String>) {
    let mut current = String::new();
    for c in hint.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' || c == '\\' {
            current.push(c);
        } else if !current.is_empty() {
            let name = current.trim_start_matches('\\');
            if !name.is_empty() && !is_primitive_type_name(name) {
                names.insert(name.to_string());
            }
            current.clear();
        }
    }
}

/// `#[Name(args), ...]` - a class used only as an attribute target
/// (`#[MyAttribute]` on some other declaration) has no `Name`/`New`/
/// `StaticAccess` node anywhere pointing at it, so without this it would
/// look identically unreferenced to a genuinely dead class.
fn collect_attrs(attrs: &[Attribute], names: &mut HashSet<String>) {
    for a in attrs {
        names.insert(a.name.clone());
        for arg in &a.args {
            collect_expr(&arg.value, names);
        }
    }
}

fn collect_param(p: &Param, names: &mut HashSet<String>) {
    collect_attrs(&p.attrs, names);
    if let Some(h) = &p.type_hint {
        collect_type_hint(h, names);
    }
    if let Some(d) = &p.default {
        collect_expr(d, names);
    }
    if let Some(hooks) = &p.hooks {
        collect_property_hooks(hooks, names);
    }
}

fn collect_property_hooks(hooks: &PropertyHooks, names: &mut HashSet<String>) {
    collect_attrs(&hooks.get_attrs, names);
    collect_attrs(&hooks.set_attrs, names);
    if let Some(body) = &hooks.get {
        collect_hook_body(body, names);
    }
    if let Some((param, body)) = &hooks.set {
        if let Some(p) = param {
            collect_param(p, names);
        }
        collect_hook_body(body, names);
    }
}

fn collect_hook_body(body: &PropertyHookBody, names: &mut HashSet<String>) {
    match body {
        PropertyHookBody::Arrow(e) => collect_expr(e, names),
        PropertyHookBody::Block(stmts) => {
            for s in stmts {
                collect_stmt(s, names);
            }
        }
        // No real body at all - nothing to collect from.
        PropertyHookBody::Abstract => {}
    }
}

fn collect_function(f: &FunctionDecl, names: &mut HashSet<String>) {
    collect_attrs(&f.attrs, names);
    for p in &f.params {
        collect_param(p, names);
    }
    if let Some(rt) = &f.return_type {
        collect_type_hint(rt, names);
    }
    for s in f.body.iter() {
        collect_stmt(s, names);
    }
}

fn collect_class(c: &ClassDecl, names: &mut HashSet<String>) {
    collect_attrs(&c.attrs, names);
    names.extend(c.extends.iter().cloned());
    names.extend(c.implements.iter().cloned());
    names.extend(c.uses.iter().cloned());
    for p in &c.properties {
        collect_attrs(&p.attrs, names);
        if let Some(h) = &p.type_hint {
            collect_type_hint(h, names);
        }
        if let Some(d) = &p.default {
            collect_expr(d, names);
        }
        if let Some(hooks) = &p.hooks {
            collect_property_hooks(hooks, names);
        }
    }
    for m in &c.methods {
        collect_function(&m.function, names);
    }
    for cst in &c.consts {
        collect_attrs(&cst.attrs, names);
        collect_expr(&cst.value, names);
    }
}

fn collect_enum(e: &EnumDecl, names: &mut HashSet<String>) {
    collect_attrs(&e.attrs, names);
    names.extend(e.implements.iter().cloned());
    names.extend(e.uses.iter().cloned());
    for m in &e.methods {
        collect_function(&m.function, names);
    }
    for cst in &e.consts {
        collect_attrs(&cst.attrs, names);
        collect_expr(&cst.value, names);
    }
    for case in &e.cases {
        collect_attrs(&case.attrs, names);
        if let Some(v) = &case.value {
            collect_expr(v, names);
        }
    }
}

// Delegates the "walk into nested statements" recursion to
// `ast_walk::stmt_children` - this match only needs to handle each
// variant's own non-statement fields (a condition, a subject, a case's
// test, a catch's exception types, ...), which `stmt_children`'s
// `Vec<&Stmt>` return type can't expose. `FunctionDecl`/`ClassDecl`/
// `EnumDecl` are the one case where both are needed: `stmt_children`
// deliberately returns none for them (they already have their own
// dedicated declaration-walker), so this calls
// `collect_function`/`collect_class`/`collect_enum` directly instead of
// relying on the trailing loop.
fn collect_stmt(stmt: &Stmt, names: &mut HashSet<String>) {
    match &stmt.kind {
        StmtKind::Block(_)
        | StmtKind::InlineHtml(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Namespace(_)
        | StmtKind::Use(_)
        | StmtKind::Global(_)
        | StmtKind::Goto(_)
        | StmtKind::Label(_)
        | StmtKind::Declare { .. } => {}
        StmtKind::Expression(e) | StmtKind::Throw(e) => collect_expr(e, names),
        StmtKind::Return(e) => {
            if let Some(e) = e {
                collect_expr(e, names);
            }
        }
        StmtKind::Echo(exprs) => {
            for e in exprs {
                collect_expr(e, names);
            }
        }
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => {
            collect_expr(cond, names);
        }
        StmtKind::For {
            init, cond, step, ..
        } => {
            for e in init.iter().chain(cond).chain(step) {
                collect_expr(e, names);
            }
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            collect_expr(subject, names);
            if let Some(k) = key {
                collect_expr(k, names);
            }
            collect_expr(value, names);
        }
        StmtKind::FunctionDecl(f) => collect_function(f, names),
        StmtKind::ClassDecl(c) => collect_class(c, names),
        StmtKind::EnumDecl(e) => collect_enum(e, names),
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                collect_expr(&c.value, names);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    collect_expr(e, names);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            collect_expr(subject, names);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_expr(t, names);
                }
            }
        }
        StmtKind::Try { catches, .. } => {
            for c in catches {
                names.extend(c.types.iter().cloned());
            }
        }
    }
    for child in stmt_children(stmt) {
        collect_stmt(child, names);
    }
}

// Delegates the "walk into child expressions" recursion to
// `ast_walk::expr_children` - this match only needs to handle each
// variant's own name-bearing fields (a `StaticAccess`/`StaticCall`
// class name, a bare `Name`, an `instanceof` target's class name) and
// the three variants `expr_children` deliberately returns no children
// for (`Closure`/`ArrowFn`/`NewAnonymousClass`, each needing its own
// non-`Vec<&Expr>`-shaped walk - see `ast_walk`'s own module doc
// comment for why).
fn collect_expr(expr: &Expr, names: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::StaticAccess(class, _) => {
            names.insert(class.clone());
        }
        // A dynamic receiver's own name references (if any) are picked
        // up by the generic `expr_children` recursion below, the same
        // way `Instanceof`'s own `Expr` target already is.
        ExprKind::StaticCall(StaticTarget::Name(n, _), _, _) => {
            names.insert(n.clone());
        }
        ExprKind::Name(n) => {
            names.insert(n.clone());
        }
        ExprKind::Instanceof(_, InstanceofTarget::Name(n, _)) => {
            names.insert(n.clone());
        }
        ExprKind::NewAnonymousClass(decl, _) => {
            collect_class(decl, names);
        }
        ExprKind::Closure {
            params,
            body,
            return_type,
            ..
        } => {
            for p in params {
                collect_param(p, names);
            }
            if let Some(rt) = return_type {
                collect_type_hint(rt, names);
            }
            for s in body {
                collect_stmt(s, names);
            }
        }
        ExprKind::ArrowFn {
            params,
            body,
            return_type,
            ..
        } => {
            for p in params {
                collect_param(p, names);
            }
            if let Some(rt) = return_type {
                collect_type_hint(rt, names);
            }
            collect_expr(body, names);
        }
        _ => {}
    }
    for child in expr_children(expr) {
        collect_expr(child, names);
    }
}

#[cfg(test)]
mod tests;
