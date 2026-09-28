//! Traversal of the AST: a [`Visitor`] over shared references and a
//! [`VisitorMut`] over mutable ones.
//!
//! Every `visit_*` method's default calls the matching `walk_*` function,
//! which visits the node's children (in source order, except that a
//! class's members are visited by kind: constants, enum cases,
//! properties, methods). The walk is
//! complete: it enters closures, arrow functions, anonymous class bodies,
//! parameter defaults, property and constant initializers, property
//! hooks, attributes and type declarations. A visitor that stops at a
//! scope boundary overrides the method for that node and does not walk
//! it.

use crate::ast::*;

/// A read-only traversal. Override the `visit_*` methods for the nodes of
/// interest and call the matching `walk_*` function to continue into the
/// node's children.
pub trait Visitor<'ast> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        walk_expr(self, expr);
    }

    /// A named function or a method's function (a closure is an
    /// `ExprKind::Closure`).
    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        walk_function_decl(self, function);
    }

    /// A class, interface or trait declaration, or an anonymous class.
    fn visit_class_decl(&mut self, class: &'ast ClassDecl) {
        walk_class_decl(self, class);
    }

    fn visit_enum_decl(&mut self, decl: &'ast EnumDecl) {
        walk_enum_decl(self, decl);
    }

    fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
        walk_method_decl(self, method);
    }

    fn visit_property_decl(&mut self, property: &'ast PropertyDecl) {
        walk_property_decl(self, property);
    }

    fn visit_class_const(&mut self, constant: &'ast ClassConst) {
        walk_class_const(self, constant);
    }

    fn visit_enum_case(&mut self, case: &'ast EnumCase) {
        walk_enum_case(self, case);
    }

    fn visit_param(&mut self, param: &'ast Param) {
        walk_param(self, param);
    }

    fn visit_property_hooks(&mut self, hooks: &'ast PropertyHooks) {
        walk_property_hooks(self, hooks);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        walk_attribute(self, attribute);
    }

    fn visit_arg(&mut self, arg: &'ast Arg) {
        walk_arg(self, arg);
    }

    fn visit_type_hint(&mut self, _hint: &'ast TypeHint) {}

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        walk_catch_clause(self, clause);
    }

    fn visit_switch_case(&mut self, case: &'ast SwitchCase) {
        walk_switch_case(self, case);
    }
}

pub fn walk_stmts<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, stmts: &'ast [Stmt]) {
    for stmt in stmts {
        v.visit_stmt(stmt);
    }
}

pub fn walk_stmt<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, stmt: &'ast Stmt) {
    match &stmt.kind {
        StmtKind::InlineHtml(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Namespace(_)
        | StmtKind::Use(_)
        | StmtKind::Goto(_)
        | StmtKind::Label(_) => {}
        StmtKind::Expression(e) | StmtKind::Throw(e) => v.visit_expr(e),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                v.visit_expr(e);
            }
        }
        StmtKind::Return(e) => {
            if let Some(e) = e {
                v.visit_expr(e);
            }
        }
        StmtKind::Block(stmts) => walk_stmts(v, stmts),
        StmtKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            v.visit_expr(cond);
            v.visit_stmt(then_branch);
            if let Some(else_branch) = else_branch {
                v.visit_stmt(else_branch);
            }
        }
        StmtKind::While { cond, body } => {
            v.visit_expr(cond);
            v.visit_stmt(body);
        }
        StmtKind::DoWhile { body, cond } => {
            v.visit_stmt(body);
            v.visit_expr(cond);
        }
        StmtKind::For {
            init,
            cond,
            step,
            body,
        } => {
            for e in init.iter().chain(cond).chain(step) {
                v.visit_expr(e);
            }
            v.visit_stmt(body);
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            body,
            ..
        } => {
            v.visit_expr(subject);
            if let Some(key) = key {
                v.visit_expr(key);
            }
            v.visit_expr(value);
            v.visit_stmt(body);
        }
        StmtKind::Declare { directives, body } => {
            for (_, value) in directives {
                v.visit_expr(value);
            }
            if let Some(body) = body {
                walk_stmts(v, body);
            }
        }
        StmtKind::Unset(targets) => {
            for target in targets {
                v.visit_expr(target);
            }
        }
        StmtKind::Global(vars) => {
            for var in vars {
                if let GlobalVar::Dynamic(e) = var {
                    v.visit_expr(e);
                }
            }
        }
        StmtKind::FunctionDecl(f) => v.visit_function_decl(f),
        StmtKind::ClassDecl(c) => v.visit_class_decl(c),
        StmtKind::EnumDecl(e) => v.visit_enum_decl(e),
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                v.visit_class_const(c);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, default) in vars {
                if let Some(default) = default {
                    v.visit_expr(default);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            v.visit_expr(subject);
            for case in cases {
                v.visit_switch_case(case);
            }
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            walk_stmts(v, body);
            for clause in catches {
                v.visit_catch_clause(clause);
            }
            if let Some(finally) = finally {
                walk_stmts(v, finally);
            }
        }
    }
}

fn walk_static_target<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, target: &'ast StaticTarget) {
    if let StaticTarget::Expr(e) = target {
        v.visit_expr(e);
    }
}

fn walk_args<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, args: &'ast [Arg]) {
    for arg in args {
        v.visit_arg(arg);
    }
}

pub fn walk_expr<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, expr: &'ast Expr) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::Name(_) => {}
        ExprKind::Isset(operands) => {
            for operand in operands {
                v.visit_expr(operand);
            }
        }
        ExprKind::Exit(_, arg) => {
            if let Some(arg) = arg {
                v.visit_expr(arg);
            }
        }
        ExprKind::DynamicVariable(inner)
        | ExprKind::Unary(_, inner)
        | ExprKind::Cast(_, inner)
        | ExprKind::Grouping(inner)
        | ExprKind::FirstClassCallable(inner)
        | ExprKind::ShellExec(inner)
        | ExprKind::Print(inner)
        | ExprKind::Empty(inner)
        | ExprKind::Eval(inner)
        | ExprKind::Include(_, inner)
        | ExprKind::Throw(inner)
        | ExprKind::YieldFrom(inner)
        | ExprKind::PropertyAccess(inner, _, _) => v.visit_expr(inner),
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                if let Some(key) = &item.key {
                    v.visit_expr(key);
                }
                v.visit_expr(&item.value);
            }
        }
        ExprKind::Binary(_, l, r)
        | ExprKind::Assign(l, r)
        | ExprKind::AssignRef(l, r)
        | ExprKind::CompoundAssign(_, l, r)
        | ExprKind::DynamicPropertyAccess(l, r) => {
            v.visit_expr(l);
            v.visit_expr(r);
        }
        ExprKind::Ternary(cond, then, otherwise) => {
            v.visit_expr(cond);
            if let Some(then) = then {
                v.visit_expr(then);
            }
            v.visit_expr(otherwise);
        }
        ExprKind::Call(callee, args) | ExprKind::New(callee, args) => {
            v.visit_expr(callee);
            walk_args(v, args);
        }
        ExprKind::MethodCall(receiver, _, args, _) => {
            v.visit_expr(receiver);
            walk_args(v, args);
        }
        ExprKind::DynamicMethodCall(receiver, name, args) => {
            v.visit_expr(receiver);
            v.visit_expr(name);
            walk_args(v, args);
        }
        ExprKind::ClassConstFetch(target, _)
        | ExprKind::ClassNameFetch(target)
        | ExprKind::StaticPropertyFetch(target, _) => walk_static_target(v, target),
        ExprKind::DynamicStaticPropertyFetch(target, name)
        | ExprKind::DynamicClassConstFetch(target, name) => {
            walk_static_target(v, target);
            v.visit_expr(name);
        }
        ExprKind::StaticCall(target, _, args) => {
            walk_static_target(v, target);
            walk_args(v, args);
        }
        ExprKind::DynamicStaticCall(target, name, args) => {
            walk_static_target(v, target);
            v.visit_expr(name);
            walk_args(v, args);
        }
        ExprKind::Index(base, index) => {
            v.visit_expr(base);
            if let Some(index) = index {
                v.visit_expr(index);
            }
        }
        ExprKind::Instanceof(inner, target) => {
            v.visit_expr(inner);
            if let InstanceofTarget::Expr(target) = target {
                v.visit_expr(target);
            }
        }
        ExprKind::Clone(inner, overrides) => {
            v.visit_expr(inner);
            if let Some(overrides) = overrides {
                v.visit_expr(overrides);
            }
        }
        ExprKind::NewAnonymousClass(class, args) => {
            walk_args(v, args);
            v.visit_class_decl(class);
        }
        ExprKind::Yield(key, value) => {
            if let Some(key) = key {
                v.visit_expr(key);
            }
            if let Some(value) = value {
                v.visit_expr(value);
            }
        }
        ExprKind::Interpolated(parts) => {
            for part in parts {
                v.visit_expr(part);
            }
        }
        ExprKind::Closure {
            params,
            body,
            return_type,
            attrs,
            ..
        } => {
            for attr in attrs {
                v.visit_attribute(attr);
            }
            for param in params {
                v.visit_param(param);
            }
            if let Some(hint) = return_type {
                v.visit_type_hint(hint);
            }
            walk_stmts(v, body);
        }
        ExprKind::ArrowFn {
            params,
            body,
            return_type,
            attrs,
            ..
        } => {
            for attr in attrs {
                v.visit_attribute(attr);
            }
            for param in params {
                v.visit_param(param);
            }
            if let Some(hint) = return_type {
                v.visit_type_hint(hint);
            }
            v.visit_expr(body);
        }
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            v.visit_expr(subject);
            for (conds, result) in arms {
                for cond in conds {
                    v.visit_expr(cond);
                }
                v.visit_expr(result);
            }
            if let Some(default) = default {
                v.visit_expr(default);
            }
        }
    }
}

pub fn walk_function_decl<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, f: &'ast FunctionDecl) {
    for attr in &f.attrs {
        v.visit_attribute(attr);
    }
    for param in &f.params {
        v.visit_param(param);
    }
    if let Some(hint) = &f.return_type {
        v.visit_type_hint(hint);
    }
    walk_stmts(v, &f.body);
}

pub fn walk_class_decl<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, c: &'ast ClassDecl) {
    for attr in &c.attrs {
        v.visit_attribute(attr);
    }
    for constant in &c.consts {
        v.visit_class_const(constant);
    }
    for case in &c.enum_cases {
        v.visit_enum_case(case);
    }
    for property in &c.properties {
        v.visit_property_decl(property);
    }
    for method in &c.methods {
        v.visit_method_decl(method);
    }
}

pub fn walk_enum_decl<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, e: &'ast EnumDecl) {
    for attr in &e.attrs {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &e.backing_type {
        v.visit_type_hint(hint);
    }
    for constant in &e.consts {
        v.visit_class_const(constant);
    }
    for case in &e.cases {
        v.visit_enum_case(case);
    }
    for method in &e.methods {
        v.visit_method_decl(method);
    }
}

pub fn walk_method_decl<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, m: &'ast MethodDecl) {
    v.visit_function_decl(&m.function);
}

pub fn walk_property_decl<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, p: &'ast PropertyDecl) {
    for attr in &p.attrs {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &p.type_hint {
        v.visit_type_hint(hint);
    }
    if let Some(default) = &p.default {
        v.visit_expr(default);
    }
    if let Some(hooks) = &p.hooks {
        v.visit_property_hooks(hooks);
    }
}

pub fn walk_class_const<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, c: &'ast ClassConst) {
    for attr in &c.attrs {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &c.type_hint {
        v.visit_type_hint(hint);
    }
    v.visit_expr(&c.value);
}

pub fn walk_enum_case<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, c: &'ast EnumCase) {
    for attr in &c.attrs {
        v.visit_attribute(attr);
    }
    if let Some(value) = &c.value {
        v.visit_expr(value);
    }
}

pub fn walk_param<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, p: &'ast Param) {
    for attr in &p.attrs {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &p.type_hint {
        v.visit_type_hint(hint);
    }
    if let Some(default) = &p.default {
        v.visit_expr(default);
    }
    if let Some(hooks) = &p.hooks {
        v.visit_property_hooks(hooks);
    }
}

fn walk_hook_body<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, body: &'ast PropertyHookBody) {
    match body {
        PropertyHookBody::Arrow(e) => v.visit_expr(e),
        PropertyHookBody::Block(stmts) => walk_stmts(v, stmts),
        PropertyHookBody::Abstract => {}
    }
}

pub fn walk_property_hooks<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, h: &'ast PropertyHooks) {
    for attr in &h.get_attrs {
        v.visit_attribute(attr);
    }
    if let Some(body) = &h.get {
        walk_hook_body(v, body);
    }
    for attr in &h.set_attrs {
        v.visit_attribute(attr);
    }
    if let Some((param, body)) = &h.set {
        if let Some(param) = param {
            v.visit_param(param);
        }
        walk_hook_body(v, body);
    }
}

pub fn walk_attribute<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, a: &'ast Attribute) {
    walk_args(v, &a.args);
}

pub fn walk_arg<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, arg: &'ast Arg) {
    v.visit_expr(&arg.value);
}

pub fn walk_catch_clause<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, c: &'ast CatchClause) {
    walk_stmts(v, &c.body);
}

pub fn walk_switch_case<'ast, V: Visitor<'ast> + ?Sized>(v: &mut V, c: &'ast SwitchCase) {
    if let Some(test) = &c.test {
        v.visit_expr(test);
    }
    walk_stmts(v, &c.body);
}

/// Whether an expression starts a scope of its own: a closure, an arrow
/// function or an anonymous class (its constructor arguments included).
pub fn opens_scope(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } | ExprKind::NewAnonymousClass(..)
    )
}

/// Whether `pred` holds for `stmt` or a statement nested in it within the
/// same function body: expressions (so closures) and nested function,
/// class and enum declarations are not entered. Stops at the first match.
pub fn any_stmt_in_body<'ast>(stmt: &'ast Stmt, pred: impl FnMut(&'ast Stmt) -> bool) -> bool {
    struct Search<F> {
        pred: F,
        found: bool,
    }
    impl<'ast, F: FnMut(&'ast Stmt) -> bool> Visitor<'ast> for Search<F> {
        fn visit_stmt(&mut self, stmt: &'ast Stmt) {
            if self.found {
                return;
            }
            if (self.pred)(stmt) {
                self.found = true;
                return;
            }
            walk_stmt(self, stmt);
        }
        fn visit_expr(&mut self, _: &'ast Expr) {}
        fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}
        fn visit_class_decl(&mut self, _: &'ast ClassDecl) {}
        fn visit_enum_decl(&mut self, _: &'ast EnumDecl) {}
    }
    let mut search = Search { pred, found: false };
    search.visit_stmt(stmt);
    search.found
}

/// Whether `pred` holds for `expr` or an expression nested in it within
/// the same scope: closures, arrow functions and anonymous classes are
/// not entered (though `pred` sees the expression that starts one). Stops
/// at the first match.
pub fn any_expr_in_scope<'ast>(expr: &'ast Expr, pred: impl FnMut(&'ast Expr) -> bool) -> bool {
    struct Search<F> {
        pred: F,
        found: bool,
    }
    impl<'ast, F: FnMut(&'ast Expr) -> bool> Visitor<'ast> for Search<F> {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if self.found {
                return;
            }
            if (self.pred)(expr) {
                self.found = true;
                return;
            }
            if !opens_scope(expr) {
                walk_expr(self, expr);
            }
        }
    }
    let mut search = Search { pred, found: false };
    search.visit_expr(expr);
    search.found
}

/// Calls `f` on each expression directly inside `expr` (its children,
/// not their descendants), in order, until `f` returns true. An
/// expression that opens a scope has none.
pub fn any_child_expr<'ast>(expr: &'ast Expr, f: impl FnMut(&'ast Expr) -> bool) -> bool {
    struct Children<F> {
        f: F,
        found: bool,
    }
    impl<'ast, F: FnMut(&'ast Expr) -> bool> Visitor<'ast> for Children<F> {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if !self.found && (self.f)(expr) {
                self.found = true;
            }
        }
    }
    if opens_scope(expr) {
        return false;
    }
    let mut children = Children { f, found: false };
    walk_expr(&mut children, expr);
    children.found
}

/// The expressions directly inside `expr`; see [`any_child_expr`].
pub fn child_exprs(expr: &Expr) -> Vec<&Expr> {
    let mut out = Vec::new();
    any_child_expr(expr, |e| {
        out.push(e);
        false
    });
    out
}

/// Calls `f` on each statement directly inside `stmt` in the same function
/// body, in order, until `f` returns true. A function, class or enum
/// declaration has none, and statements inside expressions (closure
/// bodies) are not reached.
pub fn any_child_stmt<'ast>(stmt: &'ast Stmt, f: impl FnMut(&'ast Stmt) -> bool) -> bool {
    struct Children<F> {
        f: F,
        found: bool,
    }
    impl<'ast, F: FnMut(&'ast Stmt) -> bool> Visitor<'ast> for Children<F> {
        fn visit_stmt(&mut self, stmt: &'ast Stmt) {
            if !self.found && (self.f)(stmt) {
                self.found = true;
            }
        }
        fn visit_expr(&mut self, _: &'ast Expr) {}
        fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}
        fn visit_class_decl(&mut self, _: &'ast ClassDecl) {}
        fn visit_enum_decl(&mut self, _: &'ast EnumDecl) {}
        fn visit_class_const(&mut self, _: &'ast ClassConst) {}
    }
    let mut children = Children { f, found: false };
    walk_stmt(&mut children, stmt);
    children.found
}

/// The statements directly inside `stmt`; see [`any_child_stmt`].
pub fn child_stmts(stmt: &Stmt) -> Vec<&Stmt> {
    let mut out = Vec::new();
    any_child_stmt(stmt, |s| {
        out.push(s);
        false
    });
    out
}

/// The expressions a statement holds itself (a condition, a subject, a
/// returned value, constant and static-variable initializers), not those
/// of its nested statements, nor a declaration's.
pub fn stmt_exprs(stmt: &Stmt) -> Vec<&Expr> {
    struct Own<'ast> {
        out: Vec<&'ast Expr>,
    }
    impl<'ast> Visitor<'ast> for Own<'ast> {
        fn visit_stmt(&mut self, _: &'ast Stmt) {}
        fn visit_expr(&mut self, expr: &'ast Expr) {
            self.out.push(expr);
        }
        fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}
        fn visit_class_decl(&mut self, _: &'ast ClassDecl) {}
        fn visit_enum_decl(&mut self, _: &'ast EnumDecl) {}
        fn visit_switch_case(&mut self, case: &'ast SwitchCase) {
            if let Some(test) = &case.test {
                self.out.push(test);
            }
        }
        fn visit_catch_clause(&mut self, _: &'ast CatchClause) {}
        fn visit_class_const(&mut self, constant: &'ast ClassConst) {
            self.out.push(&constant.value);
        }
    }
    if matches!(stmt.kind, StmtKind::Declare { .. }) {
        return Vec::new();
    }
    let mut own = Own { out: Vec::new() };
    walk_stmt(&mut own, stmt);
    own.out
}

/// Whether a function, method or closure body contains a `yield` or
/// `yield from`, making it a generator. A `yield` inside a nested
/// closure, arrow function, class or function belongs to that scope.
pub fn body_contains_yield(stmts: &[Stmt]) -> bool {
    stmts
        .iter()
        .any(|stmt| any_stmt_in_body(stmt, |s| stmt_exprs(s).into_iter().any(expr_contains_yield)))
}

/// `body_contains_yield` for an arrow function's expression body.
pub fn expr_contains_yield(expr: &Expr) -> bool {
    any_expr_in_scope(expr, |e| {
        matches!(e.kind, ExprKind::Yield(..) | ExprKind::YieldFrom(_))
    })
}

/// A mutating traversal, the counterpart of [`Visitor`]. Override the `visit_*` methods for the nodes of
/// interest and call the matching `walk_*` function to continue into the
/// node's children.
pub trait VisitorMut {
    fn visit_stmt(&mut self, stmt: &mut Stmt) {
        walk_stmt_mut(self, stmt);
    }

    fn visit_expr(&mut self, expr: &mut Expr) {
        walk_expr_mut(self, expr);
    }

    /// A named function or a method's function (a closure is an
    /// `ExprKind::Closure`).
    fn visit_function_decl(&mut self, function: &mut FunctionDecl) {
        walk_function_decl_mut(self, function);
    }

    /// A class, interface or trait declaration, or an anonymous class.
    fn visit_class_decl(&mut self, class: &mut ClassDecl) {
        walk_class_decl_mut(self, class);
    }

    fn visit_enum_decl(&mut self, decl: &mut EnumDecl) {
        walk_enum_decl_mut(self, decl);
    }

    fn visit_method_decl(&mut self, method: &mut MethodDecl) {
        walk_method_decl_mut(self, method);
    }

    fn visit_property_decl(&mut self, property: &mut PropertyDecl) {
        walk_property_decl_mut(self, property);
    }

    fn visit_class_const(&mut self, constant: &mut ClassConst) {
        walk_class_const_mut(self, constant);
    }

    fn visit_enum_case(&mut self, case: &mut EnumCase) {
        walk_enum_case_mut(self, case);
    }

    fn visit_param(&mut self, param: &mut Param) {
        walk_param_mut(self, param);
    }

    fn visit_property_hooks(&mut self, hooks: &mut PropertyHooks) {
        walk_property_hooks_mut(self, hooks);
    }

    fn visit_attribute(&mut self, attribute: &mut Attribute) {
        walk_attribute_mut(self, attribute);
    }

    fn visit_arg(&mut self, arg: &mut Arg) {
        walk_arg_mut(self, arg);
    }

    fn visit_type_hint(&mut self, _hint: &mut TypeHint) {}

    fn visit_catch_clause(&mut self, clause: &mut CatchClause) {
        walk_catch_clause_mut(self, clause);
    }

    fn visit_switch_case(&mut self, case: &mut SwitchCase) {
        walk_switch_case_mut(self, case);
    }
}

pub fn walk_stmts_mut<V: VisitorMut + ?Sized>(v: &mut V, stmts: &mut [Stmt]) {
    for stmt in stmts {
        v.visit_stmt(stmt);
    }
}

pub fn walk_stmt_mut<V: VisitorMut + ?Sized>(v: &mut V, stmt: &mut Stmt) {
    match &mut stmt.kind {
        StmtKind::InlineHtml(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Namespace(_)
        | StmtKind::Use(_)
        | StmtKind::Goto(_)
        | StmtKind::Label(_) => {}
        StmtKind::Expression(e) | StmtKind::Throw(e) => v.visit_expr(e),
        StmtKind::Echo(exprs) => {
            for e in exprs {
                v.visit_expr(e);
            }
        }
        StmtKind::Return(e) => {
            if let Some(e) = e {
                v.visit_expr(e);
            }
        }
        StmtKind::Block(stmts) => walk_stmts_mut(v, stmts),
        StmtKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            v.visit_expr(cond);
            v.visit_stmt(then_branch);
            if let Some(else_branch) = else_branch {
                v.visit_stmt(else_branch);
            }
        }
        StmtKind::While { cond, body } => {
            v.visit_expr(cond);
            v.visit_stmt(body);
        }
        StmtKind::DoWhile { body, cond } => {
            v.visit_stmt(body);
            v.visit_expr(cond);
        }
        StmtKind::For {
            init,
            cond,
            step,
            body,
        } => {
            for e in init
                .iter_mut()
                .chain(cond.iter_mut())
                .chain(step.iter_mut())
            {
                v.visit_expr(e);
            }
            v.visit_stmt(body);
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            body,
            ..
        } => {
            v.visit_expr(subject);
            if let Some(key) = key {
                v.visit_expr(key);
            }
            v.visit_expr(value);
            v.visit_stmt(body);
        }
        StmtKind::Declare { directives, body } => {
            for (_, value) in directives {
                v.visit_expr(value);
            }
            if let Some(body) = body {
                walk_stmts_mut(v, body);
            }
        }
        StmtKind::Unset(targets) => {
            for target in targets {
                v.visit_expr(target);
            }
        }
        StmtKind::Global(vars) => {
            for var in vars {
                if let GlobalVar::Dynamic(e) = var {
                    v.visit_expr(e);
                }
            }
        }
        StmtKind::FunctionDecl(f) => v.visit_function_decl(f),
        StmtKind::ClassDecl(c) => v.visit_class_decl(c),
        StmtKind::EnumDecl(e) => v.visit_enum_decl(e),
        StmtKind::GlobalConst(consts) => {
            for c in consts {
                v.visit_class_const(c);
            }
        }
        StmtKind::StaticVar(vars) => {
            for (_, default) in vars {
                if let Some(default) = default {
                    v.visit_expr(default);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            v.visit_expr(subject);
            for case in cases {
                v.visit_switch_case(case);
            }
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            walk_stmts_mut(v, body);
            for clause in catches {
                v.visit_catch_clause(clause);
            }
            if let Some(finally) = finally {
                walk_stmts_mut(v, finally);
            }
        }
    }
}

fn walk_static_target_mut<V: VisitorMut + ?Sized>(v: &mut V, target: &mut StaticTarget) {
    if let StaticTarget::Expr(e) = target {
        v.visit_expr(e);
    }
}

fn walk_args_mut<V: VisitorMut + ?Sized>(v: &mut V, args: &mut [Arg]) {
    for arg in args {
        v.visit_arg(arg);
    }
}

pub fn walk_expr_mut<V: VisitorMut + ?Sized>(v: &mut V, expr: &mut Expr) {
    match &mut expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::Name(_) => {}
        ExprKind::Isset(operands) => {
            for operand in operands {
                v.visit_expr(operand);
            }
        }
        ExprKind::Exit(_, arg) => {
            if let Some(arg) = arg {
                v.visit_expr(arg);
            }
        }
        ExprKind::DynamicVariable(inner)
        | ExprKind::Unary(_, inner)
        | ExprKind::Cast(_, inner)
        | ExprKind::Grouping(inner)
        | ExprKind::FirstClassCallable(inner)
        | ExprKind::ShellExec(inner)
        | ExprKind::Print(inner)
        | ExprKind::Empty(inner)
        | ExprKind::Eval(inner)
        | ExprKind::Include(_, inner)
        | ExprKind::Throw(inner)
        | ExprKind::YieldFrom(inner)
        | ExprKind::PropertyAccess(inner, _, _) => v.visit_expr(inner),
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                if let Some(key) = &mut item.key {
                    v.visit_expr(key);
                }
                v.visit_expr(&mut item.value);
            }
        }
        ExprKind::Binary(_, l, r)
        | ExprKind::Assign(l, r)
        | ExprKind::AssignRef(l, r)
        | ExprKind::CompoundAssign(_, l, r)
        | ExprKind::DynamicPropertyAccess(l, r) => {
            v.visit_expr(l);
            v.visit_expr(r);
        }
        ExprKind::Ternary(cond, then, otherwise) => {
            v.visit_expr(cond);
            if let Some(then) = then {
                v.visit_expr(then);
            }
            v.visit_expr(otherwise);
        }
        ExprKind::Call(callee, args) | ExprKind::New(callee, args) => {
            v.visit_expr(callee);
            walk_args_mut(v, args);
        }
        ExprKind::MethodCall(receiver, _, args, _) => {
            v.visit_expr(receiver);
            walk_args_mut(v, args);
        }
        ExprKind::DynamicMethodCall(receiver, name, args) => {
            v.visit_expr(receiver);
            v.visit_expr(name);
            walk_args_mut(v, args);
        }
        ExprKind::ClassConstFetch(target, _)
        | ExprKind::ClassNameFetch(target)
        | ExprKind::StaticPropertyFetch(target, _) => walk_static_target_mut(v, target),
        ExprKind::DynamicStaticPropertyFetch(target, name)
        | ExprKind::DynamicClassConstFetch(target, name) => {
            walk_static_target_mut(v, target);
            v.visit_expr(name);
        }
        ExprKind::StaticCall(target, _, args) => {
            walk_static_target_mut(v, target);
            walk_args_mut(v, args);
        }
        ExprKind::DynamicStaticCall(target, name, args) => {
            walk_static_target_mut(v, target);
            v.visit_expr(name);
            walk_args_mut(v, args);
        }
        ExprKind::Index(base, index) => {
            v.visit_expr(base);
            if let Some(index) = index {
                v.visit_expr(index);
            }
        }
        ExprKind::Instanceof(inner, target) => {
            v.visit_expr(inner);
            if let InstanceofTarget::Expr(target) = target {
                v.visit_expr(target);
            }
        }
        ExprKind::Clone(inner, overrides) => {
            v.visit_expr(inner);
            if let Some(overrides) = overrides {
                v.visit_expr(overrides);
            }
        }
        ExprKind::NewAnonymousClass(class, args) => {
            walk_args_mut(v, args);
            v.visit_class_decl(class);
        }
        ExprKind::Yield(key, value) => {
            if let Some(key) = key {
                v.visit_expr(key);
            }
            if let Some(value) = value {
                v.visit_expr(value);
            }
        }
        ExprKind::Interpolated(parts) => {
            for part in parts {
                v.visit_expr(part);
            }
        }
        ExprKind::Closure {
            params,
            body,
            return_type,
            attrs,
            ..
        } => {
            for attr in attrs {
                v.visit_attribute(attr);
            }
            for param in params {
                v.visit_param(param);
            }
            if let Some(hint) = return_type {
                v.visit_type_hint(hint);
            }
            walk_stmts_mut(v, body);
        }
        ExprKind::ArrowFn {
            params,
            body,
            return_type,
            attrs,
            ..
        } => {
            for attr in attrs {
                v.visit_attribute(attr);
            }
            for param in params {
                v.visit_param(param);
            }
            if let Some(hint) = return_type {
                v.visit_type_hint(hint);
            }
            v.visit_expr(body);
        }
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            v.visit_expr(subject);
            for (conds, result) in arms {
                for cond in conds {
                    v.visit_expr(cond);
                }
                v.visit_expr(result);
            }
            if let Some(default) = default {
                v.visit_expr(default);
            }
        }
    }
}

pub fn walk_function_decl_mut<V: VisitorMut + ?Sized>(v: &mut V, f: &mut FunctionDecl) {
    for attr in f.attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    for param in f.params.iter_mut() {
        v.visit_param(param);
    }
    if let Some(hint) = &mut f.return_type {
        v.visit_type_hint(hint);
    }
    walk_stmts_mut(v, std::sync::Arc::make_mut(&mut f.body).as_mut_slice());
}

pub fn walk_class_decl_mut<V: VisitorMut + ?Sized>(v: &mut V, c: &mut ClassDecl) {
    for attr in c.attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    for constant in c.consts.iter_mut() {
        v.visit_class_const(constant);
    }
    for case in c.enum_cases.iter_mut() {
        v.visit_enum_case(case);
    }
    for property in c.properties.iter_mut() {
        v.visit_property_decl(property);
    }
    for method in c.methods.iter_mut() {
        v.visit_method_decl(method);
    }
}

pub fn walk_enum_decl_mut<V: VisitorMut + ?Sized>(v: &mut V, e: &mut EnumDecl) {
    for attr in e.attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &mut e.backing_type {
        v.visit_type_hint(hint);
    }
    for constant in e.consts.iter_mut() {
        v.visit_class_const(constant);
    }
    for case in e.cases.iter_mut() {
        v.visit_enum_case(case);
    }
    for method in e.methods.iter_mut() {
        v.visit_method_decl(method);
    }
}

pub fn walk_method_decl_mut<V: VisitorMut + ?Sized>(v: &mut V, m: &mut MethodDecl) {
    v.visit_function_decl(&mut m.function);
}

pub fn walk_property_decl_mut<V: VisitorMut + ?Sized>(v: &mut V, p: &mut PropertyDecl) {
    for attr in p.attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &mut p.type_hint {
        v.visit_type_hint(hint);
    }
    if let Some(default) = &mut p.default {
        v.visit_expr(default);
    }
    if let Some(hooks) = &mut p.hooks {
        v.visit_property_hooks(hooks);
    }
}

pub fn walk_class_const_mut<V: VisitorMut + ?Sized>(v: &mut V, c: &mut ClassConst) {
    for attr in c.attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &mut c.type_hint {
        v.visit_type_hint(hint);
    }
    v.visit_expr(&mut c.value);
}

pub fn walk_enum_case_mut<V: VisitorMut + ?Sized>(v: &mut V, c: &mut EnumCase) {
    for attr in c.attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    if let Some(value) = &mut c.value {
        v.visit_expr(value);
    }
}

pub fn walk_param_mut<V: VisitorMut + ?Sized>(v: &mut V, p: &mut Param) {
    for attr in p.attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    if let Some(hint) = &mut p.type_hint {
        v.visit_type_hint(hint);
    }
    if let Some(default) = &mut p.default {
        v.visit_expr(default);
    }
    if let Some(hooks) = &mut p.hooks {
        v.visit_property_hooks(hooks);
    }
}

fn walk_hook_body_mut<V: VisitorMut + ?Sized>(v: &mut V, body: &mut PropertyHookBody) {
    match body {
        PropertyHookBody::Arrow(e) => v.visit_expr(e),
        PropertyHookBody::Block(stmts) => walk_stmts_mut(v, stmts),
        PropertyHookBody::Abstract => {}
    }
}

pub fn walk_property_hooks_mut<V: VisitorMut + ?Sized>(v: &mut V, h: &mut PropertyHooks) {
    for attr in h.get_attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    if let Some(body) = &mut h.get {
        walk_hook_body_mut(v, body);
    }
    for attr in h.set_attrs.iter_mut() {
        v.visit_attribute(attr);
    }
    if let Some((param, body)) = &mut h.set {
        if let Some(param) = param {
            v.visit_param(param);
        }
        walk_hook_body_mut(v, body);
    }
}

pub fn walk_attribute_mut<V: VisitorMut + ?Sized>(v: &mut V, a: &mut Attribute) {
    walk_args_mut(v, &mut a.args);
}

pub fn walk_arg_mut<V: VisitorMut + ?Sized>(v: &mut V, arg: &mut Arg) {
    v.visit_expr(&mut arg.value);
}

pub fn walk_catch_clause_mut<V: VisitorMut + ?Sized>(v: &mut V, c: &mut CatchClause) {
    walk_stmts_mut(v, &mut c.body);
}

pub fn walk_switch_case_mut<V: VisitorMut + ?Sized>(v: &mut V, c: &mut SwitchCase) {
    if let Some(test) = &mut c.test {
        v.visit_expr(test);
    }
    walk_stmts_mut(v, &mut c.body);
}

#[cfg(test)]
mod tests;
