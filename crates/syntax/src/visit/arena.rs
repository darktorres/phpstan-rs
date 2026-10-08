//! Traversal for arena-backed syntax trees.
//!
//! The owner is passed at every edge so a node ID cannot be read without
//! the file whose arena it indexes.

use crate::ast::*;

pub trait Visitor {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        walk_stmt(self, arenas, id);
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        walk_expr(self, arenas, id);
    }

    fn visit_function_decl(&mut self, arenas: &NodeArenas, function: &FunctionDecl) {
        walk_function_decl(self, arenas, function);
    }

    fn visit_class_decl(&mut self, arenas: &NodeArenas, class: &ClassDecl) {
        walk_class_decl(self, arenas, class);
    }

    fn visit_class_def(&mut self, arenas: &NodeArenas, class: &ClassDef) {
        walk_class_def(self, arenas, class);
    }

    fn visit_method_decl(&mut self, arenas: &NodeArenas, method: &MethodDecl) {
        walk_method_decl(self, arenas, method);
    }

    fn visit_property_decl(&mut self, arenas: &NodeArenas, property: &PropertyDecl) {
        walk_property_decl(self, arenas, property);
    }

    fn visit_class_const(&mut self, arenas: &NodeArenas, constant: &ClassConst) {
        walk_class_const(self, arenas, constant);
    }

    fn visit_enum_case(&mut self, arenas: &NodeArenas, case: &EnumCase) {
        walk_enum_case(self, arenas, case);
    }

    fn visit_param(&mut self, arenas: &NodeArenas, param: &Param) {
        walk_param(self, arenas, param);
    }

    fn visit_property_hooks(&mut self, arenas: &NodeArenas, hooks: &PropertyHooks) {
        walk_property_hooks(self, arenas, hooks);
    }

    fn visit_attribute(&mut self, arenas: &NodeArenas, attribute: &Attribute) {
        walk_attribute(self, arenas, attribute);
    }

    fn visit_arg(&mut self, arenas: &NodeArenas, arg: &Arg) {
        walk_arg(self, arenas, arg);
    }

    fn visit_type_hint(&mut self, arenas: &NodeArenas, hint: &TypeHint) {
        walk_type_hint(self, arenas, hint);
    }

    fn visit_name(&mut self, _name: &QualifiedName) {}

    fn visit_closure_use(&mut self, _closure_use: &ClosureUse) {}

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        walk_catch_clause(self, arenas, clause);
    }

    fn visit_switch_case(&mut self, arenas: &NodeArenas, case: &SwitchCase) {
        walk_switch_case(self, arenas, case);
    }

    fn visit_elseif(&mut self, arenas: &NodeArenas, elseif: &ElseIf) {
        walk_elseif(self, arenas, elseif);
    }
}

pub fn walk_stmts<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, stmts: &[StmtId]) {
    for &stmt in stmts {
        v.visit_stmt(arenas, stmt);
    }
}

pub fn walk_stmt<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, id: StmtId) {
    let Some(stmt) = arenas.stmt(id) else {
        return;
    };
    crate::stack::guard(|| match &stmt.kind {
        StmtKind::InlineHtml(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Namespace(_)
        | StmtKind::Use(_)
        | StmtKind::Goto(_)
        | StmtKind::Label(_) => {}
        StmtKind::Expression(expr) | StmtKind::Throw(expr) => v.visit_expr(arenas, *expr),
        StmtKind::Echo(exprs) => walk_exprs(v, arenas, arenas.expr_ids(exprs)),
        StmtKind::Return(expr) => {
            if let Some(expr) = expr {
                v.visit_expr(arenas, *expr);
            }
        }
        StmtKind::Block(stmts) => walk_stmts(v, arenas, arenas.stmt_ids(stmts)),
        StmtKind::If(stmt) => {
            v.visit_expr(arenas, stmt.cond);
            v.visit_stmt(arenas, stmt.then_branch);
            for elseif in arenas.elseifs(stmt.elseifs.clone()) {
                v.visit_elseif(arenas, elseif);
            }
            if let Some(branch) = stmt.else_branch {
                v.visit_stmt(arenas, branch);
            }
        }
        StmtKind::While { cond, body } => {
            v.visit_expr(arenas, *cond);
            v.visit_stmt(arenas, *body);
        }
        StmtKind::DoWhile { body, cond } => {
            v.visit_stmt(arenas, *body);
            v.visit_expr(arenas, *cond);
        }
        StmtKind::For {
            init,
            cond,
            step,
            body,
        } => {
            walk_exprs(v, arenas, arenas.expr_ids(init));
            walk_exprs(v, arenas, arenas.expr_ids(cond));
            walk_exprs(v, arenas, arenas.expr_ids(step));
            v.visit_stmt(arenas, *body);
        }
        StmtKind::Foreach {
            subject,
            key,
            value,
            body,
            ..
        } => {
            v.visit_expr(arenas, *subject);
            if let Some(key) = key {
                v.visit_expr(arenas, *key);
            }
            v.visit_expr(arenas, *value);
            v.visit_stmt(arenas, *body);
        }
        StmtKind::Declare { directives, body } => {
            for directive in arenas.declare_directives(directives.clone()) {
                v.visit_expr(arenas, directive.value);
            }
            if let Some(body) = body {
                walk_stmts(v, arenas, arenas.stmt_ids(body));
            }
        }
        StmtKind::Unset(targets) => walk_exprs(v, arenas, arenas.expr_ids(targets)),
        StmtKind::Global(vars) => {
            for var in arenas.global_vars(vars.clone()) {
                if let GlobalVar::Dynamic(expr) = var {
                    v.visit_expr(arenas, *expr);
                }
            }
        }
        StmtKind::FunctionDecl(function) => {
            if let Some(function) = arenas.function(*function) {
                v.visit_function_decl(arenas, function);
            }
        }
        StmtKind::ClassDecl(class) => {
            v.visit_class_decl(arenas, arenas.class_decl(class.clone()));
        }
        StmtKind::GlobalConst(constants) => {
            for constant in arenas.class_consts(constants) {
                v.visit_class_const(arenas, constant);
            }
        }
        StmtKind::StaticVar(vars) => {
            for variable in arenas.static_vars(vars.clone()) {
                if let Some(default) = variable.default {
                    v.visit_expr(arenas, default);
                }
            }
        }
        StmtKind::Switch { subject, cases } => {
            v.visit_expr(arenas, *subject);
            for case in arenas.switch_cases(cases.clone()) {
                v.visit_switch_case(arenas, case);
            }
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            walk_stmts(v, arenas, arenas.stmt_ids(body));
            for clause in arenas.catch_clauses(catches.clone()) {
                v.visit_catch_clause(arenas, clause);
            }
            if let Some(finally) = finally {
                walk_stmts(v, arenas, arenas.stmt_ids(finally));
            }
        }
    });
}

fn walk_exprs<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, exprs: &[ExprId]) {
    for &expr in exprs {
        v.visit_expr(arenas, expr);
    }
}

pub fn walk_args<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, args: ArgRange) {
    for arg in arenas.args(args.clone()) {
        v.visit_arg(arenas, arg);
    }
}

fn walk_attributes<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, attrs: AttributeRange) {
    for attr in arenas.attributes(attrs.clone()) {
        v.visit_attribute(arenas, attr);
    }
}

fn walk_static_target<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, target: &StaticTarget) {
    match target {
        StaticTarget::Name(name) => v.visit_name(name),
        StaticTarget::Expr(expr) => v.visit_expr(arenas, *expr),
    }
}

pub fn walk_expr<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, id: ExprId) {
    let Some(expr) = arenas.expr(id) else {
        return;
    };
    crate::stack::guard(|| match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) => {}
        ExprKind::Name(name) => {
            if let Some(name) = arenas.qualified_name(name) {
                v.visit_name(name);
            }
        }
        ExprKind::Isset(items) => walk_exprs(v, arenas, arenas.expr_ids(items)),
        ExprKind::Interpolated(items) => walk_exprs(v, arenas, arenas.expr_ids(items)),
        ExprKind::Exit(_, args) => {
            if let Some(args) = args {
                walk_args(v, arenas, args.clone());
            }
        }
        ExprKind::DynamicVariable(inner)
        | ExprKind::Unary(_, inner)
        | ExprKind::Cast(_, inner)
        | ExprKind::Grouping(inner)
        | ExprKind::ErrorSuppress(inner)
        | ExprKind::FirstClassCallable(inner)
        | ExprKind::ShellExec(inner)
        | ExprKind::Print(inner)
        | ExprKind::Empty(inner)
        | ExprKind::Eval(inner)
        | ExprKind::Include(_, inner)
        | ExprKind::Throw(inner)
        | ExprKind::YieldFrom(inner)
        | ExprKind::PropertyAccess(inner, ..) => v.visit_expr(arenas, *inner),
        ExprKind::ArrayLiteral(items) => {
            for item in arenas.array_items(items.clone()) {
                if let Some(key) = item.key {
                    v.visit_expr(arenas, key);
                }
                v.visit_expr(arenas, item.value);
            }
        }
        ExprKind::Destructure(items, _) => {
            for item in arenas.destructure_items(items) {
                if let Some(key) = item.key {
                    v.visit_expr(arenas, key);
                }
                if let Some(target) = item.target {
                    v.visit_expr(arenas, target);
                }
            }
        }
        ExprKind::Binary(_, lhs, rhs)
        | ExprKind::Assign(lhs, rhs)
        | ExprKind::AssignRef(lhs, rhs)
        | ExprKind::CompoundAssign(_, lhs, rhs)
        | ExprKind::DynamicPropertyAccess(lhs, rhs, _) => {
            v.visit_expr(arenas, *lhs);
            v.visit_expr(arenas, *rhs);
        }
        ExprKind::Ternary(cond, then, otherwise) => {
            v.visit_expr(arenas, *cond);
            if let Some(then) = then {
                v.visit_expr(arenas, *then);
            }
            v.visit_expr(arenas, *otherwise);
        }
        ExprKind::Call(callee, args) | ExprKind::New(callee, args) => {
            v.visit_expr(arenas, *callee);
            walk_args(v, arenas, args.clone());
        }
        ExprKind::MethodCall(receiver, _, args, _) => {
            v.visit_expr(arenas, *receiver);
            walk_args(v, arenas, args.clone());
        }
        ExprKind::DynamicMethodCall(receiver, name, args, _) => {
            v.visit_expr(arenas, *receiver);
            v.visit_expr(arenas, *name);
            walk_args(v, arenas, args.clone());
        }
        ExprKind::ClassConstFetch(target, _)
        | ExprKind::ClassNameFetch(target)
        | ExprKind::StaticPropertyFetch(target, _) => walk_static_target(v, arenas, target),
        ExprKind::DynamicStaticPropertyFetch(target, name)
        | ExprKind::DynamicClassConstFetch(target, name) => {
            walk_static_target(v, arenas, target);
            v.visit_expr(arenas, *name);
        }
        ExprKind::StaticCall(target, _, args) => {
            walk_static_target(v, arenas, target);
            walk_args(v, arenas, args.clone());
        }
        ExprKind::DynamicStaticCall(target, name, args) => {
            walk_static_target(v, arenas, target);
            v.visit_expr(arenas, *name);
            walk_args(v, arenas, args.clone());
        }
        ExprKind::Index(base, index) => {
            v.visit_expr(arenas, *base);
            if let Some(index) = index {
                v.visit_expr(arenas, *index);
            }
        }
        ExprKind::Instanceof(inner, target) => {
            v.visit_expr(arenas, *inner);
            match target {
                InstanceofTarget::Name(name) => {
                    if let Some(name) = arenas.qualified_name(name) {
                        v.visit_name(name);
                    }
                }
                InstanceofTarget::Expr(expr) => v.visit_expr(arenas, *expr),
            }
        }
        ExprKind::Clone(inner, overrides) => {
            v.visit_expr(arenas, *inner);
            if let Some(overrides) = overrides {
                v.visit_expr(arenas, *overrides);
            }
        }
        ExprKind::NewAnonymousClass(class) => {
            if let Some(class) = arenas.anonymous_class(*class) {
                walk_args(v, arenas, class.args.clone());
                if let Some(def) = arenas.class(class.def) {
                    v.visit_class_def(arenas, def);
                }
            }
        }
        ExprKind::Yield(key, value) => {
            if let Some(key) = key {
                v.visit_expr(arenas, *key);
            }
            if let Some(value) = value {
                v.visit_expr(arenas, *value);
            }
        }
        ExprKind::Closure(closure) => {
            if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure) {
                walk_attributes(v, arenas, closure.attrs.clone());
                for param in arenas.params(closure.params.clone()) {
                    v.visit_param(arenas, param);
                }
                for closure_use in arenas.closure_uses(&closure.uses) {
                    v.visit_closure_use(closure_use);
                }
                if let Some(hint) = &closure.return_type {
                    v.visit_type_hint(arenas, hint);
                }
                walk_stmts(v, arenas, arenas.stmt_ids(&closure.body));
            }
        }
        ExprKind::ArrowFn(arrow) => {
            if let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*arrow) {
                walk_attributes(v, arenas, arrow.attrs.clone());
                for param in arenas.params(arrow.params.clone()) {
                    v.visit_param(arenas, param);
                }
                if let Some(hint) = &arrow.return_type {
                    v.visit_type_hint(arenas, hint);
                }
                v.visit_expr(arenas, arrow.body);
            }
        }
        ExprKind::Match { subject, arms } => {
            v.visit_expr(arenas, *subject);
            for arm in arenas.match_arms(arms.clone()) {
                if let Some(conditions) = &arm.conditions {
                    walk_exprs(v, arenas, arenas.expr_ids(conditions));
                }
                v.visit_expr(arenas, arm.body);
            }
        }
    });
}

pub fn walk_function_decl<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, f: &FunctionDecl) {
    walk_attributes(v, arenas, f.attrs.clone());
    for param in arenas.params(f.params.clone()) {
        v.visit_param(arenas, param);
    }
    if let Some(hint) = &f.return_type {
        v.visit_type_hint(arenas, hint);
    }
    walk_stmts(v, arenas, f.body_stmts(arenas));
}

pub fn walk_class_decl<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, c: &ClassDecl) {
    if let Some(def) = arenas.class(c.def) {
        v.visit_class_def(arenas, def);
    }
}

pub fn walk_class_def<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, c: &ClassDef) {
    walk_attributes(v, arenas, c.attrs.clone());
    for name in arenas
        .qualified_names(&c.extends)
        .iter()
        .chain(arenas.qualified_names(&c.implements))
    {
        v.visit_name(name);
    }
    for trait_use in arenas.trait_uses(&c.trait_uses) {
        for name in arenas.qualified_names(&trait_use.traits) {
            v.visit_name(name);
        }
        for alias in arenas.trait_aliases(&trait_use.adaptations.aliases) {
            if let Some(name) = &alias.trait_name {
                v.visit_name(name);
            }
        }
        for precedence in arenas.trait_precedences(&trait_use.adaptations.precedences) {
            v.visit_name(&precedence.trait_name);
            for name in arenas.qualified_names(&precedence.instead_of) {
                v.visit_name(name);
            }
        }
    }
    if let Some(hint) = &c.backing_type {
        v.visit_type_hint(arenas, hint);
    }
    for constant in arenas.class_consts(&c.consts) {
        v.visit_class_const(arenas, constant);
    }
    for case in arenas.enum_cases(&c.enum_cases) {
        v.visit_enum_case(arenas, case);
    }
    for property in arenas.properties(&c.properties) {
        v.visit_property_decl(arenas, property);
    }
    for method in arenas.methods(c.methods.clone()) {
        v.visit_method_decl(arenas, method);
    }
}

pub fn walk_method_decl<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, m: &MethodDecl) {
    v.visit_function_decl(arenas, &m.function);
}

pub fn walk_property_decl<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, p: &PropertyDecl) {
    walk_attributes(v, arenas, p.attrs.clone());
    if let Some(hint) = &p.type_hint {
        v.visit_type_hint(arenas, hint);
    }
    if let Some(default) = p.default {
        v.visit_expr(arenas, default);
    }
    if let Some(hooks) = &p.hooks {
        v.visit_property_hooks(arenas, hooks);
    }
}

pub fn walk_class_const<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, c: &ClassConst) {
    walk_attributes(v, arenas, c.attrs.clone());
    if let Some(hint) = &c.type_hint {
        v.visit_type_hint(arenas, hint);
    }
    v.visit_expr(arenas, c.value);
}

pub fn walk_enum_case<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, c: &EnumCase) {
    walk_attributes(v, arenas, c.attrs.clone());
    if let Some(value) = c.value {
        v.visit_expr(arenas, value);
    }
}

pub fn walk_type_hint<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, hint: &TypeHint) {
    crate::stack::guard(|| match &hint.kind {
        TypeHintKind::Keyword(..) => {}
        TypeHintKind::Name(name) => {
            if let Some(name) = arenas.qualified_name(name) {
                v.visit_name(name);
            }
        }
        TypeHintKind::Nullable(inner) => v.visit_type_hint(arenas, inner),
        TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
            for member in arenas.type_hints(members) {
                v.visit_type_hint(arenas, member);
            }
        }
    });
}

pub fn walk_param<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, p: &Param) {
    walk_attributes(v, arenas, p.attrs.clone());
    if let Some(hint) = &p.type_hint {
        v.visit_type_hint(arenas, hint);
    }
    if let Some(default) = p.default {
        v.visit_expr(arenas, default);
    }
    if let Some(hooks) = &p.hooks {
        v.visit_property_hooks(arenas, hooks);
    }
}

pub fn walk_property_hooks<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, h: &PropertyHooks) {
    for hook in h.iter() {
        walk_attributes(v, arenas, hook.attrs.clone());
        if let Some(param) = &hook.param {
            v.visit_param(arenas, param);
        }
        match &hook.body {
            PropertyHookBody::Arrow(expr) => v.visit_expr(arenas, *expr),
            PropertyHookBody::Block(stmts) => walk_stmts(v, arenas, arenas.stmt_ids(stmts)),
            PropertyHookBody::Abstract => {}
        }
    }
}

pub fn walk_attribute<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, a: &Attribute) {
    v.visit_name(&a.name);
    walk_args(v, arenas, a.args.clone());
}

pub fn walk_arg<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, arg: &Arg) {
    v.visit_expr(arenas, arg.value);
}

pub fn walk_catch_clause<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, c: &CatchClause) {
    for name in arenas.qualified_names(&c.types) {
        v.visit_name(name);
    }
    walk_stmts(v, arenas, arenas.stmt_ids(&c.body));
}

pub fn walk_elseif<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, e: &ElseIf) {
    v.visit_expr(arenas, e.cond);
    v.visit_stmt(arenas, e.body);
}

pub fn walk_switch_case<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, c: &SwitchCase) {
    if let Some(test) = c.test {
        v.visit_expr(arenas, test);
    }
    walk_stmts(v, arenas, arenas.stmt_ids(&c.body));
}

pub fn walk_expr_in_scope<V: Visitor + ?Sized>(v: &mut V, arenas: &NodeArenas, id: ExprId) {
    let Some(expr) = arenas.expr(id) else {
        return;
    };
    match &expr.kind {
        ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
        ExprKind::NewAnonymousClass(class) => {
            if let Some(class) = arenas.anonymous_class(*class) {
                walk_args(v, arenas, class.args.clone());
            }
        }
        _ => walk_expr(v, arenas, id),
    }
}

/// Calls `f` on each expression directly inside `id`, in source order,
/// until `f` returns true. Nested expression scope boundaries follow
/// [`walk_expr_in_scope`].
pub fn any_child_expr(
    arenas: &NodeArenas,
    id: ExprId,
    f: impl FnMut(ExprId, &ExprNode) -> bool,
) -> bool {
    struct Children<F> {
        f: F,
        found: bool,
    }
    impl<F: FnMut(ExprId, &ExprNode) -> bool> Visitor for Children<F> {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if self.found {
                return;
            }
            if let Some(expr) = arenas.expr(id) {
                self.found = (self.f)(id, expr);
            }
        }
    }
    let mut children = Children { f, found: false };
    walk_expr_in_scope(&mut children, arenas, id);
    children.found
}

/// Calls `f` on each direct expression child of `id`, in source order.
pub fn for_each_child_expr(arenas: &NodeArenas, id: ExprId, mut f: impl FnMut(ExprId, &ExprNode)) {
    any_child_expr(arenas, id, |child_id, child| {
        f(child_id, child);
        false
    });
}

/// Calls `f` on each statement directly inside `id` in the same function
/// body, in source order. Function, class, and expression scopes are not
/// descended into.
pub fn any_child_stmt(
    arenas: &NodeArenas,
    id: StmtId,
    f: impl FnMut(StmtId, &StmtNode) -> bool,
) -> bool {
    struct Children<F> {
        f: F,
        found: bool,
    }
    impl<F: FnMut(StmtId, &StmtNode) -> bool> Visitor for Children<F> {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            if self.found {
                return;
            }
            if let Some(stmt) = arenas.stmt(id) {
                self.found = (self.f)(id, stmt);
            }
        }

        fn visit_expr(&mut self, _arenas: &NodeArenas, _id: ExprId) {}

        fn visit_function_decl(&mut self, _arenas: &NodeArenas, _function: &FunctionDecl) {}

        fn visit_class_def(&mut self, _arenas: &NodeArenas, _class: &ClassDef) {}

        fn visit_class_const(&mut self, _arenas: &NodeArenas, _constant: &ClassConst) {}
    }
    let mut children = Children { f, found: false };
    walk_stmt(&mut children, arenas, id);
    children.found
}

/// Calls `f` on each direct statement child of `id` in the same function
/// body, in source order.
pub fn for_each_child_stmt(arenas: &NodeArenas, id: StmtId, mut f: impl FnMut(StmtId, &StmtNode)) {
    any_child_stmt(arenas, id, |child_id, child| {
        f(child_id, child);
        false
    });
}

/// Whether `pred` holds for `id` or an expression nested within it in the
/// same scope. Closure, arrow-function and anonymous-class bodies are not
/// entered, though the expression that starts one is visited.
pub fn any_expr_in_scope(
    arenas: &NodeArenas,
    id: ExprId,
    pred: impl FnMut(&ExprNode) -> bool,
) -> bool {
    struct Search<F> {
        pred: F,
        found: bool,
    }
    impl<F: FnMut(&ExprNode) -> bool> Visitor for Search<F> {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if self.found {
                return;
            }
            let Some(expr) = arenas.expr(id) else {
                return;
            };
            if (self.pred)(expr) {
                self.found = true;
                return;
            }
            walk_expr_in_scope(self, arenas, id);
        }
    }
    let mut search = Search { pred, found: false };
    search.visit_expr(arenas, id);
    search.found
}

/// Whether a statement body contains a yield in the current scope, without
/// entering nested closures, arrow functions, or anonymous classes.
pub fn body_contains_yield(arenas: &NodeArenas, stmts: &[StmtId]) -> bool {
    any_expr_in_body(arenas, stmts, |expr| {
        matches!(expr.kind, ExprKind::Yield(..) | ExprKind::YieldFrom(_))
    })
}

/// Whether `pred` holds for an expression in the current function-like
/// body. Nested functions, classes, closures, and arrow functions are not
/// entered; anonymous-class constructor arguments remain in the current
/// scope.
pub fn any_expr_in_body(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    pred: impl FnMut(&ExprNode) -> bool,
) -> bool {
    struct Find<F> {
        pred: F,
        found: bool,
    }
    impl<F: FnMut(&ExprNode) -> bool> Visitor for Find<F> {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if self.found {
                return;
            }
            let Some(expr) = arenas.expr(id) else {
                return;
            };
            if (self.pred)(expr) {
                self.found = true;
                return;
            }
            walk_expr_in_scope(self, arenas, id);
        }

        fn visit_function_decl(&mut self, _arenas: &NodeArenas, _function: &FunctionDecl) {}

        fn visit_class_def(&mut self, _arenas: &NodeArenas, _class: &ClassDef) {}
    }
    let mut find = Find { pred, found: false };
    walk_stmts(&mut find, arenas, stmts);
    find.found
}

/// Whether `pred` holds for a statement in `id` or its nested statement
/// bodies. Expressions are scope boundaries, so nested functions, closures,
/// and anonymous classes are not entered.
pub fn any_stmt_in_body(
    arenas: &NodeArenas,
    id: StmtId,
    pred: impl FnMut(&StmtNode) -> bool,
) -> bool {
    struct Find<F> {
        pred: F,
        found: bool,
    }
    impl<F: FnMut(&StmtNode) -> bool> Visitor for Find<F> {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            if self.found {
                return;
            }
            let Some(stmt) = arenas.stmt(id) else {
                return;
            };
            if (self.pred)(stmt) {
                self.found = true;
                return;
            }
            walk_stmt(self, arenas, id);
        }

        fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
    }
    let mut find = Find { pred, found: false };
    find.visit_stmt(arenas, id);
    find.found
}
