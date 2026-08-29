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
/// skips reporting, rather than risk false positives.
///
/// `if`/`while` fork `locals` into per-branch copies (narrowed via
/// `apply_narrowing` on the condition) and merge them back at the join
/// point (`merge_scopes`), accounting for a branch that unconditionally
/// terminates (`always_terminates`) so a guard clause's narrowing survives
/// past the `if`. Everything else (`for`/`foreach`/`switch`/`try`) still
/// shares one mutable scope with no forking or narrowing - a real but
/// bounded gap, not a soundness issue, since no narrowing ever happens
/// there rather than an incorrect one.
pub struct Checker<'a> {
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
    // The class whose method body is currently being checked, if any -
    // needed to resolve self/parent/static to a concrete class name.
    current_class: Option<String>,
}

impl<'a> Checker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Checker {
            registry,
            diagnostics: Vec::new(),
            current_class: None,
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

        let previous_class = self.current_class.take();
        self.current_class = class_name.map(|s| s.to_string());

        for stmt in &f.body {
            self.check_stmt(stmt, &mut locals, declared_return.as_ref(), &label);
        }

        self.current_class = previous_class;
    }

    /// Resolves `self`/`static`/`parent` to a concrete `Object` relative to
    /// `defining_class` - the class whose signature this type came from.
    /// `self`/`static` both collapse to the same class here since we don't
    /// track late static binding through actual runtime dispatch; `parent`
    /// resolves via the registry's extends chain, falling back to leaving
    /// it unresolved if the class has no declared parent (so it still hits
    /// the permissive Object fallback in `is_subtype`/`is_assignable`
    /// rather than being wrongly flagged).
    fn resolve_pseudo(&self, ty: &Type, defining_class: &str) -> Type {
        match ty {
            Type::SelfRef | Type::StaticRef => Type::Object(defining_class.to_string()),
            Type::ParentRef => self
                .registry
                .classes
                .get(defining_class)
                .and_then(|info| info.extends.first())
                .map(|p| Type::Object(p.clone()))
                .unwrap_or_else(|| ty.clone()),
            _ => ty.clone(),
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
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    if actual != Type::Mixed && !self.registry.is_assignable(&actual, &expected) {
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

                let mut then_locals = locals.clone();
                self.apply_narrowing(cond, &mut then_locals, true);
                self.check_stmt(then_branch, &mut then_locals, declared_return, label);

                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                if let Some(e) = else_branch {
                    self.check_stmt(e, &mut else_locals, declared_return, label);
                }

                // The classic guard clause - `if ($x === null) { return; }`
                // - depends on this: a branch that unconditionally
                // returns/throws/breaks/continues never reaches the merge
                // point, so only the surviving branch's (narrowed) scope
                // should propagate past the if. Without this, the
                // terminated branch's narrowing (here, "$x is null")
                // would incorrectly leak into the code after the guard.
                let then_terminates = always_terminates(then_branch);
                let else_terminates = else_branch.as_deref().is_some_and(always_terminates);

                *locals = match (then_terminates, else_terminates) {
                    (true, false) => else_locals,
                    (false, true) => then_locals,
                    _ => merge_scopes(&then_locals, &else_locals),
                };
            }
            Stmt::While { cond, body } => {
                self.check_expr(cond, locals, label);
                let mut body_locals = locals.clone();
                self.apply_narrowing(cond, &mut body_locals, true);
                self.check_stmt(body, &mut body_locals, declared_return, label);
                // The loop may run zero times, so what's true after it is
                // "either the pre-loop state, or whatever one narrowed
                // iteration produces" - not a real fixed point, but sound
                // enough to avoid claiming more precision than we have.
                *locals = merge_scopes(locals, &body_locals);
            }
            Stmt::DoWhile { body, cond } => {
                // The condition is only evaluated after the body has
                // already run once, so there's nothing to narrow going in.
                self.check_stmt(body, locals, declared_return, label);
                self.check_expr(cond, locals, label);
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

    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    fn apply_narrowing(&self, cond: &Expr, locals: &mut HashMap<String, Type>, branch_true: bool) {
        match cond {
            Expr::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical | BinOp::Eq);
                let is_neq = matches!(op, BinOp::NotIdentical | BinOp::NotEq);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                }
            }
            Expr::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            Expr::Grouping(inner) => self.apply_narrowing(inner, locals, branch_true),
            Expr::Call(callee, args) => {
                if let (Expr::Name(name), [Expr::Variable(var_name)]) =
                    (callee.as_ref(), args.as_slice())
                    && let Some(target_ty) = narrowing_type_for_predicate(name)
                {
                    let current = locals.get(var_name).cloned().unwrap_or(Type::Mixed);
                    let narrowed = if branch_true {
                        narrow_type_to(&current, &target_ty)
                    } else {
                        narrow_type_excluding(&current, &target_ty)
                    };
                    locals.insert(var_name.clone(), narrowed);
                }
            }
            Expr::Instanceof(inner, class_name) => {
                // The false branch would need a "not this class" type we
                // don't have a representation for, so it's left alone.
                if branch_true && let Expr::Variable(var_name) = inner.as_ref() {
                    locals.insert(var_name.clone(), Type::Object(class_name.clone()));
                }
            }
            _ => {}
        }
    }

    fn apply_equality_narrowing(
        &self,
        l: &Expr,
        r: &Expr,
        locals: &mut HashMap<String, Type>,
        matches_when_true: bool,
    ) {
        let pair = match (l, r) {
            (Expr::Variable(name), other) => Some((name.clone(), self.infer_expr(other, locals))),
            (other, Expr::Variable(name)) => Some((name.clone(), self.infer_expr(other, locals))),
            _ => None,
        };
        let Some((var_name, target_ty)) = pair else {
            return;
        };
        if target_ty == Type::Mixed {
            return;
        }
        let current = locals.get(&var_name).cloned().unwrap_or(Type::Mixed);
        let narrowed = if matches_when_true {
            narrow_type_to(&current, &target_ty)
        } else {
            narrow_type_excluding(&current, &target_ty)
        };
        locals.insert(var_name, narrowed);
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
                    self.check_args(name, sig, args, locals, label, None);
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
                    self.check_args(&qualified, sig, args, locals, label, Some(&class));
                }
            }
            Expr::Binary(_, l, r) => {
                self.check_expr(l, locals, label);
                self.check_expr(r, locals, label);
            }
            Expr::Unary(_, inner)
            | Expr::Grouping(inner)
            | Expr::Clone(inner)
            | Expr::Cast(_, inner) => {
                self.check_expr(inner, locals, label);
            }
            Expr::Assign(lhs, rhs) => {
                self.check_expr(lhs, locals, label);
                self.check_expr(rhs, locals, label);
                self.check_property_assignment(lhs, rhs, locals, label);
            }
            Expr::CompoundAssign(_, lhs, rhs) => {
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

    fn check_property_assignment(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        locals: &HashMap<String, Type>,
        label: &str,
    ) {
        let Expr::PropertyAccess(base, prop) = lhs else {
            return;
        };
        let Type::Object(class) = self.infer_expr(base, locals) else {
            return;
        };
        let Some(prop_sig) = self.registry.lookup_property(&class, prop) else {
            // Not a declared property: could be a dynamic property, which
            // is legal (if deprecated) PHP - not flagged here, only wrong
            // types on properties we do know about.
            return;
        };
        let expected = self.resolve_pseudo(&prop_sig.ty, &class);
        let actual = self.infer_expr(rhs, locals);
        if actual == Type::Mixed || self.registry.is_assignable(&actual, &expected) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!("{class}::${prop} expects {expected}, {actual} given"),
        });
    }

    fn check_args(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Expr],
        locals: &HashMap<String, Type>,
        label: &str,
        defining_class: Option<&str>,
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
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(&param.ty, c),
                None => param.ty.clone(),
            };
            if !self.registry.is_assignable(&arg_ty, &param_ty) {
                self.diagnostics.push(Diagnostic {
                    location: label.to_string(),
                    message: format!(
                        "{callee_label}() argument #{} (${}) expects {}, {} given",
                        i + 1,
                        param.name,
                        param_ty,
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
            Expr::Cast(ty, _) => match ty.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "string" => Type::String,
                "bool" => Type::Bool,
                "array" => Type::Array(Box::new(Type::Mixed)),
                "object" => Type::Object("object".to_string()),
                "unset" => Type::Null,
                _ => Type::Mixed,
            },
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
                    "self" | "parent" | "static" => {
                        let raw = match name.as_str() {
                            "self" => Type::SelfRef,
                            "parent" => Type::ParentRef,
                            _ => Type::StaticRef,
                        };
                        match &self.current_class {
                            Some(c) => self.resolve_pseudo(&raw, c),
                            None => raw,
                        }
                    }
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
                    .map(|s| self.resolve_pseudo(&s.return_type, &class))
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
            Expr::PropertyAccess(base, prop) => match self.infer_expr(base, locals) {
                Type::Object(class) => self
                    .registry
                    .lookup_property(&class, prop)
                    .map(|p| self.resolve_pseudo(&p.ty, &class))
                    .unwrap_or(Type::Mixed),
                _ => Type::Mixed,
            },
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

fn narrowing_type_for_predicate(name: &str) -> Option<Type> {
    match name {
        "is_string" => Some(Type::String),
        "is_int" | "is_integer" | "is_long" => Some(Type::Int),
        "is_float" | "is_double" => Some(Type::Float),
        "is_bool" | "is_boolean" => Some(Type::Bool),
        "is_array" => Some(Type::Array(Box::new(Type::Mixed))),
        "is_null" => Some(Type::Null),
        "is_object" => Some(Type::Object("object".to_string())),
        _ => None,
    }
}

/// Narrows `original` to `target` along the branch where a check for
/// `target` succeeded: if `original` is a union, keep only the member(s)
/// that match; otherwise just trust the more specific `target`.
fn narrow_type_to(original: &Type, target: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members.iter().filter(|m| *m == target).cloned().collect();
            match kept.len() {
                0 => target.clone(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        _ => target.clone(),
    }
}

/// Narrows `original` by removing `excluded` along the branch where a check
/// for it failed - the "not null" half of a `=== null` guard clause, the
/// most common narrowing pattern in real code.
fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members.iter().filter(|m| *m != excluded).cloned().collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        other if other == excluded => Type::Never,
        other => other.clone(),
    }
}

/// Union of two types for a scope-merge join point - flattens nested
/// unions and keeps `Mixed` "unknown" rather than let it compose into a
/// falsely-precise-looking Union (same principle as the ternary inference).
fn union_types(a: Type, b: Type) -> Type {
    if a == b {
        return a;
    }
    if a == Type::Mixed || b == Type::Mixed {
        return Type::Mixed;
    }
    let mut members = Vec::new();
    match a {
        Type::Union(m) => members.extend(m),
        other => members.push(other),
    }
    match b {
        Type::Union(m) => {
            for x in m {
                if !members.contains(&x) {
                    members.push(x);
                }
            }
        }
        other => {
            if !members.contains(&other) {
                members.push(other);
            }
        }
    }
    if members.len() == 1 {
        members.into_iter().next().unwrap()
    } else {
        Type::Union(members)
    }
}

/// Conservatively recognizes a statement that unconditionally leaves the
/// current block (return/throw/break/continue, or a block/if where every
/// path does). Only used to decide whether a branch's locals should
/// propagate past an `if` - false negatives here (missing a case that
/// really does always terminate) just mean less narrowing, never a wrong
/// answer, so loops/switch/try are deliberately not analyzed and just
/// treated as "might fall through".
fn always_terminates(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Block(stmts) => stmts.last().is_some_and(always_terminates),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            always_terminates(then_branch) && else_branch.as_deref().is_some_and(always_terminates)
        }
        _ => false,
    }
}

/// Joins two branch-local scopes back into one after an `if`/`while`. A
/// variable present in both branches gets the union of its two types; one
/// present in only one branch was introduced there and can't be assumed to
/// exist afterward, so it's dropped rather than carried forward unsoundly.
fn merge_scopes(
    then_locals: &HashMap<String, Type>,
    else_locals: &HashMap<String, Type>,
) -> HashMap<String, Type> {
    let mut merged = HashMap::new();
    for (name, then_ty) in then_locals {
        if let Some(else_ty) = else_locals.get(name) {
            merged.insert(name.clone(), union_types(then_ty.clone(), else_ty.clone()));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::registry::Registry;

    fn diagnostics_for(src: &str) -> Vec<Diagnostic> {
        let tokens = Lexer::new(src).tokenize();
        let program = Parser::new(tokens).parse_program();
        let mut registry = Registry::build(&program);
        registry.merge_stubs(crate::stubs::builtin_stubs());
        let mut checker = Checker::new(&registry);
        checker.check_program(&program);
        checker.into_diagnostics()
    }

    #[test]
    fn flags_wrong_argument_type() {
        let d = diagnostics_for(r#"<?php strlen(42);"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expects string"));
    }

    #[test]
    fn flags_too_few_arguments() {
        let d = diagnostics_for(
            r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            needsTwo(1);
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expects at least 2"));
    }

    #[test]
    fn flags_wrong_return_type() {
        let d = diagnostics_for(
            r#"<?php
            function addOne(int $x): int { return $x . "1"; }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("return type mismatch"));
    }

    #[test]
    fn optional_and_variadic_params_do_not_trigger_arity_errors() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $a, int $b = 1, string ...$rest): int { return $a + $b; }
            f(1);
            f(1, 2, "x", "y");
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn concrete_class_name_satisfies_self_return_type() {
        // Regression test: `new UserRepository()` and `new self()` are
        // equally valid ways to satisfy a `: self` return type declared on
        // UserRepository itself - this used to be a false positive.
        let d = diagnostics_for(
            r#"<?php
            class UserRepository {
                public static function make(): self {
                    return new UserRepository();
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn wrong_class_returned_from_self_typed_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                public static function make(): self { return new Bar(); }
            }
            class Bar {}
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expected Foo"));
        assert!(d[0].message.contains("got Bar"));
    }

    #[test]
    fn fluent_method_chain_through_self_return_type_infers_concretely() {
        let d = diagnostics_for(
            r#"<?php
            class QueryBuilder {
                public function where(): self { return $this; }
            }
            function needsQueryBuilder(QueryBuilder $q): void {}
            $qb = new QueryBuilder();
            needsQueryBuilder($qb->where());
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn flags_wrong_type_assigned_to_typed_property() {
        let d = diagnostics_for(
            r#"<?php
            class Box { public int $value = 0; }
            $box = new Box();
            $box->value = "not a number";
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("Box::$value"));
    }

    #[test]
    fn correct_property_assignment_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Box { public int $value = 0; }
            $box = new Box();
            $box->value = 5;
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn promoted_property_assignment_is_type_checked() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public function __construct(public int $x) {}
            }
            $p = new Point(1);
            $p->x = "nope";
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("Point::$x"));
    }

    #[test]
    fn dynamic_property_not_declared_on_class_is_not_flagged() {
        // Undeclared/dynamic properties are legal (if deprecated) PHP -
        // this prototype only checks types on properties it knows about.
        let d = diagnostics_for(
            r#"<?php
            class Box {}
            $box = new Box();
            $box->anything = "fine, not tracked";
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn property_read_flows_into_argument_type_check() {
        let d = diagnostics_for(
            r#"<?php
            class Box { public string $label = ""; }
            function needsInt(int $x): int { return $x; }
            $box = new Box();
            needsInt($box->label);
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn guard_clause_narrows_nullable_param_after_early_return() {
        // The single most common narrowing idiom in real PHP: without
        // recognizing that the `if` branch terminates, the narrowed-to-null
        // state from that branch would incorrectly leak past the guard.
        let d = diagnostics_for(
            r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    return "stranger";
                }
                return $name;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn guard_clause_narrows_in_else_branch_too() {
        let d = diagnostics_for(
            r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    $name = "stranger";
                } else {
                    // $name narrowed to string here already, this is a no-op
                }
                return $name;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_string_narrowing_catches_misuse_inside_the_guarded_branch() {
        // Before narrowing, an untyped param stays Mixed forever and this
        // bug would have been silently skipped rather than caught.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function process($value) {
                if (is_string($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn instanceof_narrows_to_the_checked_class() {
        let d = diagnostics_for(
            r#"<?php
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if ($animal instanceof Cat) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn scope_fork_does_not_leak_then_branch_assignment_into_else() {
        // Before real forking, then/else shared one mutable scope
        // sequentially, so an assignment in the then-branch could leak into
        // what the else-branch (wrongly) saw.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function f(bool $cond) {
                if ($cond) {
                    $x = "a string";
                } else {
                    $x = 5;
                    needsInt($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn merged_type_after_non_terminating_if_else_is_a_union() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function f(bool $cond) {
                if ($cond) {
                    $x = "a string";
                } else {
                    $x = 5;
                }
                needsInt($x);
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn phpdoc_only_param_type_catches_a_bug_previously_invisible_as_mixed() {
        // Before PHPDoc types, an untyped native param stayed Mixed forever
        // and this call would have been silently skipped.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
            double("not an int");
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("double"));
    }

    #[test]
    fn phpdoc_only_param_type_does_not_flag_correct_usage() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
            double(5);
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
}
