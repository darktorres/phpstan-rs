use std::collections::HashSet;

use crate::ast::*;

/// Best-effort, deliberately over-approximating set of every function/class
/// name one file's parse tree could reference externally - used purely to
/// decide which *other* files might need re-checking when a symbol they
/// declare changes. Missing a real reference here would let a stale
/// diagnostic survive an edit elsewhere; collecting an extra name that
/// turns out unrelated just costs one harmless re-check, so this errs
/// toward over-collecting rather than trying to be precise.
///
/// Names are kept exactly as the AST spells them (no backslash-stripping,
/// no namespace resolution) to match how the checker/registry themselves
/// already resolve these fields - `is_subclass_of`/`lookup_function` do a
/// plain exact-string lookup too, since namespaces aren't modeled anywhere
/// in this project. Type hints are the one exception: they're strings that
/// get resolved through `types::parse_type_string`, which does strip a
/// leading backslash, so the names pulled out of them are stripped the
/// same way to stay consistent with that.
pub fn referenced_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    for stmt in stmts {
        collect_stmt(stmt, &mut names);
    }
    names
}

fn is_primitive_type_name(name: &str) -> bool {
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

fn collect_param(p: &Param, names: &mut HashSet<String>) {
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
    }
}

fn collect_function(f: &FunctionDecl, names: &mut HashSet<String>) {
    for p in &f.params {
        collect_param(p, names);
    }
    if let Some(rt) = &f.return_type {
        collect_type_hint(rt, names);
    }
    for s in &f.body {
        collect_stmt(s, names);
    }
}

fn collect_class(c: &ClassDecl, names: &mut HashSet<String>) {
    names.extend(c.extends.iter().cloned());
    names.extend(c.implements.iter().cloned());
    names.extend(c.uses.iter().cloned());
    for p in &c.properties {
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
        collect_expr(&cst.value, names);
    }
}

fn collect_enum(e: &EnumDecl, names: &mut HashSet<String>) {
    names.extend(e.implements.iter().cloned());
    names.extend(e.uses.iter().cloned());
    for m in &e.methods {
        collect_function(&m.function, names);
    }
    for cst in &e.consts {
        collect_expr(&cst.value, names);
    }
    for case in &e.cases {
        if let Some(v) = &case.value {
            collect_expr(v, names);
        }
    }
}

fn collect_stmt(stmt: &Stmt, names: &mut HashSet<String>) {
    match stmt {
        Stmt::InlineHtml(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Namespace(_)
        | Stmt::Use(_)
        | Stmt::Global(_)
        | Stmt::Goto(_)
        | Stmt::Label(_) => {}
        Stmt::Expression(e) | Stmt::Throw(e) => collect_expr(e, names),
        Stmt::Return(e) => {
            if let Some(e) = e {
                collect_expr(e, names);
            }
        }
        Stmt::Echo(exprs) => {
            for e in exprs {
                collect_expr(e, names);
            }
        }
        Stmt::Block(stmts) => {
            for s in stmts {
                collect_stmt(s, names);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr(cond, names);
            collect_stmt(then_branch, names);
            if let Some(e) = else_branch {
                collect_stmt(e, names);
            }
        }
        Stmt::While { cond, body } => {
            collect_expr(cond, names);
            collect_stmt(body, names);
        }
        Stmt::DoWhile { body, cond } => {
            collect_stmt(body, names);
            collect_expr(cond, names);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            for e in init.iter().chain(cond).chain(step) {
                collect_expr(e, names);
            }
            collect_stmt(body, names);
        }
        Stmt::Foreach {
            subject,
            key,
            value,
            body,
            ..
        } => {
            collect_expr(subject, names);
            if let Some(k) = key {
                collect_expr(k, names);
            }
            collect_expr(value, names);
            collect_stmt(body, names);
        }
        Stmt::FunctionDecl(f) => collect_function(f, names),
        Stmt::ClassDecl(c) => collect_class(c, names),
        Stmt::EnumDecl(e) => collect_enum(e, names),
        Stmt::GlobalConst(consts) => {
            for c in consts {
                collect_expr(&c.value, names);
            }
        }
        Stmt::StaticVar(vars) => {
            for (_, d) in vars {
                if let Some(e) = d {
                    collect_expr(e, names);
                }
            }
        }
        Stmt::Switch { subject, cases } => {
            collect_expr(subject, names);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_expr(t, names);
                }
                for s in &case.body {
                    collect_stmt(s, names);
                }
            }
        }
        Stmt::Try {
            body,
            catches,
            finally,
        } => {
            for s in body {
                collect_stmt(s, names);
            }
            for c in catches {
                names.extend(c.types.iter().cloned());
                for s in &c.body {
                    collect_stmt(s, names);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    collect_stmt(s, names);
                }
            }
        }
    }
}

fn collect_expr(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::Literal(_) | Expr::Variable(_) => {}
        Expr::DynamicVariable(e) => collect_expr(e, names),
        Expr::ArrayLiteral(items) => {
            for (k, v) in items {
                if let Some(k) = k {
                    collect_expr(k, names);
                }
                collect_expr(v, names);
            }
        }
        Expr::Unary(_, e)
        | Expr::Grouping(e)
        | Expr::Cast(_, e)
        | Expr::Throw(e)
        | Expr::YieldFrom(e)
        | Expr::FirstClassCallable(e) => collect_expr(e, names),
        Expr::Binary(_, l, r) | Expr::Assign(l, r) | Expr::CompoundAssign(_, l, r) => {
            collect_expr(l, names);
            collect_expr(r, names);
        }
        Expr::Ternary(c, t, e) => {
            collect_expr(c, names);
            if let Some(t) = t {
                collect_expr(t, names);
            }
            collect_expr(e, names);
        }
        Expr::Call(callee, args) => {
            collect_expr(callee, names);
            for a in args {
                collect_expr(a, names);
            }
        }
        Expr::New(target, args) => {
            collect_expr(target, names);
            for a in args {
                collect_expr(a, names);
            }
        }
        Expr::PropertyAccess(base, _) => collect_expr(base, names),
        Expr::MethodCall(recv, _, args) => {
            collect_expr(recv, names);
            for a in args {
                collect_expr(a, names);
            }
        }
        Expr::DynamicPropertyAccess(base, name_expr) => {
            collect_expr(base, names);
            collect_expr(name_expr, names);
        }
        Expr::DynamicMethodCall(recv, name_expr, args) => {
            collect_expr(recv, names);
            collect_expr(name_expr, names);
            for a in args {
                collect_expr(a, names);
            }
        }
        Expr::StaticAccess(class, _) => {
            names.insert(class.clone());
        }
        Expr::StaticCall(class, _, args) => {
            names.insert(class.clone());
            for a in args {
                collect_expr(a, names);
            }
        }
        Expr::Index(base, idx) => {
            collect_expr(base, names);
            if let Some(i) = idx {
                collect_expr(i, names);
            }
        }
        Expr::Name(n) => {
            names.insert(n.clone());
        }
        Expr::Instanceof(inner, target) => {
            collect_expr(inner, names);
            match target {
                InstanceofTarget::Name(n) => {
                    names.insert(n.clone());
                }
                InstanceofTarget::Expr(e) => collect_expr(e, names),
            }
        }
        Expr::Clone(inner, overrides) => {
            collect_expr(inner, names);
            if let Some(o) = overrides {
                collect_expr(o, names);
            }
        }
        Expr::NewAnonymousClass(decl, args) => {
            collect_class(decl, names);
            for a in args {
                collect_expr(a, names);
            }
        }
        Expr::Yield(k, v) => {
            if let Some(k) = k {
                collect_expr(k, names);
            }
            if let Some(v) = v {
                collect_expr(v, names);
            }
        }
        Expr::Interpolated(parts) => {
            for p in parts {
                collect_expr(p, names);
            }
        }
        Expr::Closure {
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
        Expr::ArrowFn {
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
        Expr::Match {
            subject,
            arms,
            default,
        } => {
            collect_expr(subject, names);
            for (conds, result) in arms {
                for c in conds {
                    collect_expr(c, names);
                }
                collect_expr(result, names);
            }
            if let Some(d) = default {
                collect_expr(d, names);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn refs(src: &str) -> HashSet<String> {
        let tokens = Lexer::new(src).tokenize();
        let ast = Parser::new(tokens).parse_program();
        referenced_names(&ast)
    }

    #[test]
    fn collects_extends_implements_and_trait_use() {
        let names = refs("<?php class Foo extends Base implements Iface { use SomeTrait; }");
        assert!(names.contains("Base"));
        assert!(names.contains("Iface"));
        assert!(names.contains("SomeTrait"));
    }

    #[test]
    fn collects_calls_new_static_access_and_instanceof() {
        let names = refs(
            r#"<?php
            function f($x) {
                helper();
                new Thing();
                Other::bar();
                Other::CONST;
                if ($x instanceof Checked) {}
            }
        "#,
        );
        assert!(names.contains("helper"));
        assert!(names.contains("Thing"));
        assert!(names.contains("Other"));
        assert!(names.contains("Checked"));
    }

    #[test]
    fn collects_type_hints_but_skips_primitives() {
        let names = refs("<?php function f(int $a, ?Foo $b, Bar|Baz $c): Qux {}");
        assert!(names.contains("Foo"));
        assert!(names.contains("Bar"));
        assert!(names.contains("Baz"));
        assert!(names.contains("Qux"));
        assert!(!names.contains("int"));
    }

    #[test]
    fn collects_from_closures_and_arrow_functions() {
        let names = refs(
            r#"<?php
            $f = function () { return new InClosure(); };
            $g = fn() => new InArrow();
        "#,
        );
        assert!(names.contains("InClosure"));
        assert!(names.contains("InArrow"));
    }
}
