use std::collections::HashMap;

use crate::ast::*;
use crate::registry::{FunctionSig, Registry};
use crate::types::{self, Type};

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub location: String,
    pub message: String,
}

/// Walks function/method bodies, inferring a best-effort type for each
/// expression and flagging argument/return type mismatches against the
/// registry. Deliberately conservative: `Type::Mixed` is overloaded here to
/// mean both PHP's real `mixed` type AND "we don't know" for anything this
/// prototype can't infer (property access, closures, dynamic calls, ...) -
/// so every check below treats an inferred `Mixed` as inconclusive and
/// skips reporting, rather than risk false positives. This is flow-
/// insensitive: a variable's tracked type is simply overwritten on each
/// assignment seen, with no control-flow merging (no narrowing on `if`,
/// no re-widening after a loop, etc).
pub struct Checker<'a> {
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Checker {
            registry,
            diagnostics: Vec::new(),
        }
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    pub fn check_program(&mut self, program: &[Stmt]) {
        let mut top_locals: HashMap<String, Type> = HashMap::new();
        for stmt in program {
            match stmt {
                Stmt::FunctionDecl(f) => self.check_function(f, None),
                Stmt::ClassDecl(c) => {
                    for m in &c.methods {
                        self.check_function(&m.function, Some(&c.name));
                    }
                }
                other => self.check_stmt(other, &mut top_locals, None, "<main>"),
            }
        }
    }

    fn check_function(&mut self, f: &FunctionDecl, class_name: Option<&str>) {
        let mut locals: HashMap<String, Type> = HashMap::new();
        for p in &f.params {
            let ty = p
                .type_hint
                .as_deref()
                .map(types::parse_type_string)
                .unwrap_or(Type::Mixed);
            locals.insert(p.name.clone(), ty);
        }
        if let Some(c) = class_name {
            locals.insert("this".to_string(), Type::Object(c.to_string()));
        }

        let declared_return = f.return_type.as_deref().map(types::parse_type_string);
        let label = match class_name {
            Some(c) => format!("{c}::{}", f.name),
            None => f.name.clone(),
        };

        for stmt in &f.body {
            self.check_stmt(stmt, &mut locals, declared_return.as_ref(), &label);
        }
    }

    fn check_block(
        &mut self,
        stmts: &[Stmt],
        locals: &mut HashMap<String, Type>,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        for stmt in stmts {
            self.check_stmt(stmt, locals, declared_return, label);
        }
    }

    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        locals: &mut HashMap<String, Type>,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        match stmt {
            Stmt::Expression(e) => {
                self.check_expr(e, locals, label);
                self.track_assignment(e, locals);
            }
            Stmt::Return(Some(e)) => {
                self.check_expr(e, locals, label);
                if let Some(expected) = declared_return {
                    let actual = self.infer_expr(e, locals);
                    if actual != Type::Mixed && !self.registry.is_assignable(&actual, expected) {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!(
                                "return type mismatch: expected {expected}, got {actual}"
                            ),
                        });
                    }
                }
            }
            Stmt::Echo(exprs) => {
                for e in exprs {
                    self.check_expr(e, locals, label);
                }
            }
            Stmt::Block(stmts) => self.check_block(stmts, locals, declared_return, label),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(cond, locals, label);
                self.check_stmt(then_branch, locals, declared_return, label);
                if let Some(e) = else_branch {
                    self.check_stmt(e, locals, declared_return, label);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.check_expr(cond, locals, label);
                self.check_stmt(body, locals, declared_return, label);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                for e in init.iter().chain(cond).chain(step) {
                    self.check_expr(e, locals, label);
                }
                self.check_stmt(body, locals, declared_return, label);
            }
            Stmt::Foreach {
                subject,
                key,
                value,
                body,
                ..
            } => {
                self.check_expr(subject, locals, label);
                if let Some(k) = key {
                    self.check_expr(k, locals, label);
                }
                self.check_expr(value, locals, label);
                self.check_stmt(body, locals, declared_return, label);
            }
            Stmt::Switch { subject, cases } => {
                self.check_expr(subject, locals, label);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.check_expr(t, locals, label);
                    }
                    self.check_block(&case.body, locals, declared_return, label);
                }
            }
            Stmt::Try {
                body,
                catches,
                finally,
            } => {
                self.check_block(body, locals, declared_return, label);
                for c in catches {
                    self.check_block(&c.body, locals, declared_return, label);
                }
                if let Some(f) = finally {
                    self.check_block(f, locals, declared_return, label);
                }
            }
            Stmt::Throw(e) => self.check_expr(e, locals, label),
            _ => {}
        }
    }

    fn track_assignment(&self, e: &Expr, locals: &mut HashMap<String, Type>) {
        match e {
            Expr::Assign(lhs, rhs) => {
                if let Expr::Variable(name) = lhs.as_ref() {
                    let ty = self.infer_expr(rhs, locals);
                    locals.insert(name.clone(), ty);
                }
            }
            Expr::CompoundAssign(op, lhs, rhs) => {
                if let Expr::Variable(name) = lhs.as_ref() {
                    let ty = self.infer_binary(op, lhs, rhs, locals);
                    locals.insert(name.clone(), ty);
                }
            }
            _ => {}
        }
    }

    fn check_expr(&mut self, e: &Expr, locals: &HashMap<String, Type>, label: &str) {
        match e {
            Expr::Call(callee, args) => {
                for a in args {
                    self.check_expr(a, locals, label);
                }
                if let Expr::Name(name) = callee.as_ref()
                    && let Some(sig) = self.registry.lookup_function(name)
                {
                    self.check_args(name, sig, args, locals, label);
                }
            }
            Expr::MethodCall(recv, method, args) => {
                self.check_expr(recv, locals, label);
                for a in args {
                    self.check_expr(a, locals, label);
                }
                if let Type::Object(class) = self.infer_expr(recv, locals)
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let qualified = format!("{class}::{method}");
                    self.check_args(&qualified, sig, args, locals, label);
                }
            }
            Expr::Binary(_, l, r) => {
                self.check_expr(l, locals, label);
                self.check_expr(r, locals, label);
            }
            Expr::Unary(_, inner) | Expr::Grouping(inner) | Expr::Clone(inner) => {
                self.check_expr(inner, locals, label);
            }
            Expr::Assign(lhs, rhs) | Expr::CompoundAssign(_, lhs, rhs) => {
                self.check_expr(lhs, locals, label);
                self.check_expr(rhs, locals, label);
            }
            Expr::Ternary(cond, then_, else_) => {
                self.check_expr(cond, locals, label);
                if let Some(t) = then_ {
                    self.check_expr(t, locals, label);
                }
                self.check_expr(else_, locals, label);
            }
            Expr::ArrayLiteral(items) => {
                for (k, v) in items {
                    if let Some(k) = k {
                        self.check_expr(k, locals, label);
                    }
                    self.check_expr(v, locals, label);
                }
            }
            Expr::Index(base, idx) => {
                self.check_expr(base, locals, label);
                if let Some(i) = idx {
                    self.check_expr(i, locals, label);
                }
            }
            Expr::New(_, args) => {
                for a in args {
                    self.check_expr(a, locals, label);
                }
            }
            Expr::PropertyAccess(base, _) => self.check_expr(base, locals, label),
            Expr::Interpolated(parts) => {
                for p in parts {
                    self.check_expr(p, locals, label);
                }
            }
            _ => {}
        }
    }

    fn check_args(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Expr],
        locals: &HashMap<String, Type>,
        label: &str,
    ) {
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.has_default)
            .count();
        if args.len() < required {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{callee_label}() expects at least {required} argument(s), {} given",
                    args.len()
                ),
            });
        }

        for (i, arg) in args.iter().enumerate() {
            let param = match sig.params.get(i) {
                Some(p) => p,
                None => match sig.params.last() {
                    Some(p) if p.variadic => p,
                    _ => break,
                },
            };
            let arg_ty = self.infer_expr(arg, locals);
            if arg_ty == Type::Mixed {
                continue;
            }
            if !self.registry.is_assignable(&arg_ty, &param.ty) {
                self.diagnostics.push(Diagnostic {
                    location: label.to_string(),
                    message: format!(
                        "{callee_label}() argument #{} (${}) expects {}, {} given",
                        i + 1,
                        param.name,
                        param.ty,
                        arg_ty
                    ),
                });
            }
        }
    }

    fn infer_expr(&self, e: &Expr, locals: &HashMap<String, Type>) -> Type {
        match e {
            Expr::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.clone()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            Expr::Variable(name) => locals.get(name).cloned().unwrap_or(Type::Mixed),
            Expr::Grouping(inner) | Expr::Clone(inner) => self.infer_expr(inner, locals),
            Expr::Unary(UnaryOp::Not, _) => Type::Bool,
            Expr::Unary(_, inner) => self.infer_expr(inner, locals),
            Expr::Binary(op, l, r) => self.infer_binary(op, l, r, locals),
            Expr::Assign(_, rhs) => self.infer_expr(rhs, locals),
            // `self`/`parent`/`static` aren't lexed as keywords (see the
            // parser: they're indistinguishable from any other identifier),
            // so a literal `Expr::Name("self")` has to be special-cased
            // here rather than showing up as an Object of that name.
            Expr::New(class_expr, _) => match class_expr.as_ref() {
                Expr::Name(name) => match name.as_str() {
                    "self" => Type::SelfRef,
                    "parent" => Type::ParentRef,
                    "static" => Type::StaticRef,
                    _ => Type::Object(name.clone()),
                },
                _ => Type::Mixed,
            },
            Expr::Call(callee, _) => match callee.as_ref() {
                Expr::Name(name) => self
                    .registry
                    .lookup_function(name)
                    .map(|s| s.return_type.clone())
                    .unwrap_or(Type::Mixed),
                _ => Type::Mixed,
            },
            Expr::MethodCall(recv, method, _) => match self.infer_expr(recv, locals) {
                Type::Object(class) => self
                    .registry
                    .lookup_method(&class, method)
                    .map(|s| s.return_type.clone())
                    .unwrap_or(Type::Mixed),
                _ => Type::Mixed,
            },
            Expr::Ternary(_, then_, else_) => {
                let else_ty = self.infer_expr(else_, locals);
                match then_ {
                    Some(t) => {
                        let then_ty = self.infer_expr(t, locals);
                        // Never let an unknown branch compose into a partial
                        // Union that looks more precise than it is.
                        if then_ty == Type::Mixed || else_ty == Type::Mixed {
                            Type::Mixed
                        } else if then_ty == else_ty {
                            then_ty
                        } else {
                            Type::Union(vec![then_ty, else_ty])
                        }
                    }
                    None => Type::Mixed,
                }
            }
            Expr::ArrayLiteral(_) => Type::Array(Box::new(Type::Mixed)),
            Expr::Interpolated(_) => Type::String,
            _ => Type::Mixed,
        }
    }

    fn infer_binary(&self, op: &BinOp, l: &Expr, r: &Expr, locals: &HashMap<String, Type>) -> Type {
        match op {
            BinOp::Concat => Type::String,
            BinOp::Eq
            | BinOp::Identical
            | BinOp::NotEq
            | BinOp::NotIdentical
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::LtEq
            | BinOp::GtEq
            | BinOp::And
            | BinOp::Or
            | BinOp::Spaceship => Type::Bool,
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else {
                    Type::Int
                }
            }
            // Real PHP division returns int or float depending on the
            // runtime values, not just their static types - not modeled.
            BinOp::Div => Type::Mixed,
            BinOp::Coalesce => self.infer_expr(r, locals),
        }
    }
}
