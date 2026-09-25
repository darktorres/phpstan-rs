//! Shared, exhaustively-matched "what are this node's direct children"
//! helpers for `Expr`/`Stmt` - built specifically so a full-tree walk (like
//! `deps::collect_expr`) doesn't need its own independently-hand-maintained
//! copy of this same recursion structure. Before this module existed, this
//! exact traversal shape was hand-written three separate times
//! (`checker::check_expr`, `checker::infer_expr`, `deps::collect_expr`) -
//! any `Arg`/`ArrayItem` AST change would otherwise have to update all
//! three in lockstep purely because of that duplication.
//!
//! **Scope, load-bearing for correctness**: `expr_children` is exhaustive
//! over all of `ExprKind`'s variants in the sense that every variant gets a
//! real match arm, but three of them - `Closure`, `ArrowFn`,
//! `NewAnonymousClass` - deliberately return no children at all, not
//! because they have none, but because what they embed doesn't fit this
//! function's `Vec<&Expr>` return type (`Closure`'s `body: Vec<Stmt>`,
//! `NewAnonymousClass`'s `Box<ClassDecl>`) or needs scope-aware handling a
//! generic child-list can't express (`ArrowFn`'s body is checked against a
//! *cloned* `locals`, not the enclosing scope by reference). Every consumer
//! keeps its own explicit, hand-written arm for these three, exactly as
//! `checker.rs` and `deps.rs` already do today. The same reasoning applies
//! to `stmt_children`'s `FunctionDecl`/`ClassDecl`/`EnumDecl` - each already
//! has a dedicated declaration-walker (`checker::check_function`,
//! `deps::collect_class`/`collect_enum`) rather than being flattened into
//! an ordinary statement list.
//!
//! `stmt_children` only enumerates *nested statements* - its `Vec<&Stmt>`
//! return type can't represent the `Expr` fields most `StmtKind` variants
//! also carry (a condition, a subject, a return value, ...). A caller that
//! wants those needs its own field access for them, the same way it always
//! has - this helper only removes the risk of *forgetting a branch that
//! contains further statements*, not the need to look at a statement's own
//! non-statement fields.

use crate::ast::{Expr, ExprKind, InstanceofTarget, StaticTarget, Stmt, StmtKind};

/// This `Expr`'s direct child expressions - see the module doc comment for
/// the three variants (`Closure`/`ArrowFn`/`NewAnonymousClass`) that
/// deliberately return none.
pub fn expr_children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::Name(_) => vec![],
        ExprKind::ClassConstFetch(target, _)
        | ExprKind::ClassNameFetch(target)
        | ExprKind::StaticPropertyFetch(target, _) => target.expr().into_iter().collect(),
        ExprKind::DynamicStaticPropertyFetch(target, name)
        | ExprKind::DynamicClassConstFetch(target, name) => target
            .expr()
            .into_iter()
            .chain(std::iter::once(name.as_ref()))
            .collect(),
        ExprKind::DynamicVariable(inner)
        | ExprKind::Unary(_, inner)
        | ExprKind::Cast(_, inner)
        | ExprKind::Grouping(inner)
        | ExprKind::FirstClassCallable(inner)
        | ExprKind::ShellExec(inner)
        | ExprKind::Throw(inner)
        | ExprKind::YieldFrom(inner)
        | ExprKind::PropertyAccess(inner, _, _) => vec![inner],
        ExprKind::ArrayLiteral(items) => items
            .iter()
            .flat_map(|item| item.key.iter().chain(std::iter::once(&item.value)))
            .collect(),
        ExprKind::Binary(_, l, r)
        | ExprKind::Assign(l, r)
        | ExprKind::AssignRef(l, r)
        | ExprKind::CompoundAssign(_, l, r)
        | ExprKind::DynamicPropertyAccess(l, r) => vec![l, r],
        ExprKind::Ternary(cond, then_, else_) => {
            let mut v = vec![cond.as_ref()];
            if let Some(t) = then_ {
                v.push(t);
            }
            v.push(else_);
            v
        }
        ExprKind::Call(callee, args) | ExprKind::New(callee, args) => {
            let mut v = vec![callee.as_ref()];
            v.extend(args.iter().map(|a| &a.value));
            v
        }
        ExprKind::MethodCall(recv, _, args, _) => {
            let mut v = vec![recv.as_ref()];
            v.extend(args.iter().map(|a| &a.value));
            v
        }
        ExprKind::DynamicMethodCall(recv, name_expr, args) => {
            let mut v = vec![recv.as_ref(), name_expr.as_ref()];
            v.extend(args.iter().map(|a| &a.value));
            v
        }
        ExprKind::StaticCall(target, _, args) => {
            let mut v = Vec::new();
            if let StaticTarget::Expr(t) = target {
                v.push(t.as_ref());
            }
            v.extend(args.iter().map(|a| &a.value));
            v
        }
        ExprKind::DynamicStaticCall(target, name, args) => {
            let mut v: Vec<&Expr> = target.expr().into_iter().collect();
            v.push(name);
            v.extend(args.iter().map(|a| &a.value));
            v
        }
        ExprKind::Index(base, idx) => {
            let mut v = vec![base.as_ref()];
            if let Some(i) = idx {
                v.push(i);
            }
            v
        }
        ExprKind::Instanceof(inner, target) => {
            let mut v = vec![inner.as_ref()];
            if let InstanceofTarget::Expr(t) = target {
                v.push(t);
            }
            v
        }
        ExprKind::Clone(inner, overrides) => {
            let mut v = vec![inner.as_ref()];
            if let Some(o) = overrides {
                v.push(o);
            }
            v
        }
        ExprKind::Yield(k, v) => k.iter().chain(v.iter()).map(|b| b.as_ref()).collect(),
        ExprKind::Interpolated(parts) => parts.iter().collect(),
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            let mut v = vec![subject.as_ref()];
            for (conds, result) in arms {
                v.extend(conds.iter());
                v.push(result);
            }
            if let Some(d) = default {
                v.push(d);
            }
            v
        }
        // See the module doc comment - deliberately no automatic child
        // list for these three.
        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } | ExprKind::NewAnonymousClass(..) => {
            vec![]
        }
    }
}

/// This `Stmt`'s direct child *statements* - see the module doc comment
/// for why this can't also expose a statement's own `Expr` fields, and for
/// the three declaration variants that deliberately return none.
pub fn stmt_children(s: &Stmt) -> Vec<&Stmt> {
    match &s.kind {
        StmtKind::InlineHtml(_)
        | StmtKind::Expression(_)
        | StmtKind::Echo(_)
        | StmtKind::Return(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Global(_)
        | StmtKind::Namespace(_)
        | StmtKind::Use(_)
        | StmtKind::GlobalConst(_)
        | StmtKind::StaticVar(_)
        | StmtKind::Throw(_)
        | StmtKind::Goto(_)
        | StmtKind::Label(_)
        | StmtKind::Declare { .. }
        // Dedicated declaration-walkers already exist for these three
        // (`checker::check_function`, `deps::collect_class`/
        // `collect_enum`) - not flattened into an ordinary statement list.
        | StmtKind::FunctionDecl(_)
        | StmtKind::ClassDecl(_)
        | StmtKind::EnumDecl(_) => vec![],
        StmtKind::Block(stmts) => stmts.iter().collect(),
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let mut v = vec![then_branch.as_ref()];
            if let Some(e) = else_branch {
                v.push(e);
            }
            v
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::Foreach { body, .. } => vec![body],
        StmtKind::Switch { cases, .. } => cases.iter().flat_map(|c| c.body.iter()).collect(),
        StmtKind::Try {
            body,
            catches,
            finally,
        } => body
            .iter()
            .chain(catches.iter().flat_map(|c| c.body.iter()))
            .chain(finally.iter().flatten())
            .collect(),
    }
}

/// This statement's own direct expression fields (not nested statements -
/// `stmt_children` already covers those). Mirrors `deps::collect_stmt`'s
/// own match shape: each `StmtKind` variant's non-statement fields need
/// their own handling, a generic child-list can't express them.
pub fn stmt_own_exprs(stmt: &Stmt) -> Vec<&Expr> {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => vec![e],
        StmtKind::Return(e) => e.iter().collect(),
        StmtKind::Echo(exprs) => exprs.iter().collect(),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => {
            vec![cond]
        }
        StmtKind::For {
            init, cond, step, ..
        } => init.iter().chain(cond).chain(step).collect(),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            let mut v = vec![subject];
            if let Some(k) = key {
                v.push(k);
            }
            v.push(value);
            v
        }
        StmtKind::Switch { subject, cases } => {
            let mut v = vec![subject];
            for case in cases {
                if let Some(t) = &case.test {
                    v.push(t);
                }
            }
            v
        }
        StmtKind::GlobalConst(consts) => consts.iter().map(|c| &c.value).collect(),
        StmtKind::StaticVar(vars) => vars.iter().filter_map(|(_, d)| d.as_ref()).collect(),
        _ => vec![],
    }
}

/// Non-allocating counterpart to `expr_children(e).into_iter().any(f)` -
/// mirrors the exact same match arms (verified arm-by-arm against
/// `expr_children`'s own source, not just assumed - including the same
/// three deliberately-empty `Closure`/`ArrowFn`/`NewAnonymousClass`
/// variants) without ever building a `Vec`. Short-circuits in the
/// identical order `expr_children`'s own `Vec` would iterate in, so
/// `any_expr_child(e, &mut f)` always agrees with `expr_children(e).
/// into_iter().any(f)` - just without the intermediate allocation on
/// every node visited. A generic-over-`f` helper, not one hand-
/// duplicated per caller, for the same reason `expr_children` itself
/// exists (see this module's own doc comment) - `f` returning `false`
/// unconditionally turns this into a plain "visit every child" walk
/// (used by callers that need a side effect on every node, not a
/// short-circuiting predicate).
pub fn any_expr_child(e: &Expr, mut f: &mut impl FnMut(&Expr) -> bool) -> bool {
    match &e.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::Name(_) => false,
        ExprKind::ClassConstFetch(target, _)
        | ExprKind::ClassNameFetch(target)
        | ExprKind::StaticPropertyFetch(target, _) => target.expr().is_some_and(f),
        ExprKind::DynamicStaticPropertyFetch(target, name)
        | ExprKind::DynamicClassConstFetch(target, name) => {
            target.expr().is_some_and(&mut f) || f(name)
        }
        ExprKind::DynamicVariable(inner)
        | ExprKind::Unary(_, inner)
        | ExprKind::Cast(_, inner)
        | ExprKind::Grouping(inner)
        | ExprKind::FirstClassCallable(inner)
        | ExprKind::ShellExec(inner)
        | ExprKind::Throw(inner)
        | ExprKind::YieldFrom(inner)
        | ExprKind::PropertyAccess(inner, _, _) => f(inner),
        ExprKind::ArrayLiteral(items) => items
            .iter()
            .any(|item| item.key.as_ref().is_some_and(&mut f) || f(&item.value)),
        ExprKind::Binary(_, l, r)
        | ExprKind::Assign(l, r)
        | ExprKind::AssignRef(l, r)
        | ExprKind::CompoundAssign(_, l, r)
        | ExprKind::DynamicPropertyAccess(l, r) => f(l) || f(r),
        ExprKind::Ternary(cond, then_, else_) => {
            f(cond) || then_.as_ref().is_some_and(|t| f(t)) || f(else_)
        }
        ExprKind::Call(callee, args) | ExprKind::New(callee, args) => {
            f(callee) || args.iter().any(|a| f(&a.value))
        }
        ExprKind::MethodCall(recv, _, args, _) => f(recv) || args.iter().any(|a| f(&a.value)),
        ExprKind::DynamicMethodCall(recv, name_expr, args) => {
            f(recv) || f(name_expr) || args.iter().any(|a| f(&a.value))
        }
        ExprKind::StaticCall(target, _, args) => {
            matches!(target, StaticTarget::Expr(t) if f(t)) || args.iter().any(|a| f(&a.value))
        }
        ExprKind::DynamicStaticCall(target, name, args) => {
            target.expr().is_some_and(&mut f) || f(name) || args.iter().any(|a| f(&a.value))
        }
        ExprKind::Index(base, idx) => f(base) || idx.as_ref().is_some_and(|i| f(i)),
        ExprKind::Instanceof(inner, target) => {
            f(inner) || matches!(target, InstanceofTarget::Expr(t) if f(t))
        }
        ExprKind::Clone(inner, overrides) => f(inner) || overrides.as_ref().is_some_and(|o| f(o)),
        ExprKind::Yield(k, v) => {
            k.as_ref().is_some_and(|b| f(b)) || v.as_ref().is_some_and(|b| f(b))
        }
        ExprKind::Interpolated(parts) => parts.iter().any(f),
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            f(subject)
                || arms
                    .iter()
                    .any(|(conds, result)| conds.iter().any(&mut f) || f(result))
                || default.as_ref().is_some_and(|d| f(d))
        }
        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } | ExprKind::NewAnonymousClass(..) => {
            false
        }
    }
}

/// Non-allocating counterpart to `stmt_children(s).into_iter().any(f)` -
/// see `any_expr_child`'s own doc comment for the full rationale and
/// verification approach, mirrored here for `stmt_children` instead.
pub fn any_stmt_child(s: &Stmt, mut f: &mut impl FnMut(&Stmt) -> bool) -> bool {
    match &s.kind {
        StmtKind::InlineHtml(_)
        | StmtKind::Expression(_)
        | StmtKind::Echo(_)
        | StmtKind::Return(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Global(_)
        | StmtKind::Namespace(_)
        | StmtKind::Use(_)
        | StmtKind::GlobalConst(_)
        | StmtKind::StaticVar(_)
        | StmtKind::Throw(_)
        | StmtKind::Goto(_)
        | StmtKind::Label(_)
        | StmtKind::Declare { .. }
        | StmtKind::FunctionDecl(_)
        | StmtKind::ClassDecl(_)
        | StmtKind::EnumDecl(_) => false,
        StmtKind::Block(stmts) => stmts.iter().any(f),
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => f(then_branch) || else_branch.as_ref().is_some_and(|e| f(e)),
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::Foreach { body, .. } => f(body),
        StmtKind::Switch { cases, .. } => cases.iter().any(|c| c.body.iter().any(&mut f)),
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            body.iter().any(&mut f)
                || catches.iter().any(|c| c.body.iter().any(&mut f))
                || finally.iter().flatten().any(f)
        }
    }
}

/// Does this function/method/closure body contain a `yield`/`yield from`
/// anywhere, making it a real PHP generator? A generator always returns a
/// real `Generator` object when called, regardless of whether it has an
/// explicit `return` - `return;`/`return $x;` *inside* it sets
/// `Generator::getReturn()`'s own value, not the enclosing function's
/// return value, so real PHPStan never checks either against the
/// function's own declared `Generator`/`Iterator`/`Traversable` return
/// type (only against a `@return Generator<...,...,...,TReturn>`
/// docblock's 4th argument, if present - not modeled here).
pub(crate) fn body_contains_yield(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_contains_yield)
}

/// An arrow function's own body is a single expression, not a
/// `Vec<Stmt>` `body_contains_yield` can walk - this is that same
/// walk's expression-only counterpart (`any_expr_child` already stops at
/// a nested `Closure`/`ArrowFn` boundary, the correct semantics here
/// too: a `yield` inside a nested one doesn't make *this* arrow function
/// a generator). Uses `any_expr_child` (non-allocating) rather than
/// `expr_children`/`.into_iter().any()` - this runs on every function/
/// method/closure body checked, a real, measured cost at that call
/// volume.
pub(crate) fn expr_contains_yield(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Yield(..) | ExprKind::YieldFrom(_))
        || any_expr_child(e, &mut expr_contains_yield)
}

/// Mirrors `deps::collect_stmt`'s own exhaustive per-variant shape: each
/// `StmtKind` variant's own `Expr`-bearing field(s), checked via
/// `expr_contains_yield`, plus recursing into `any_stmt_child` for nested
/// statements. `any_stmt_child` (and `any_expr_child`, transitively)
/// already stop at `Closure`/`ArrowFn`/`NewAnonymousClass`/`FunctionDecl`/
/// `ClassDecl`/`EnumDecl` boundaries by design - exactly the correct
/// semantics here too, since a `yield` inside any of those nested scopes
/// does NOT make the *outer* function a generator.
fn stmt_contains_yield(stmt: &Stmt) -> bool {
    let own = match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => expr_contains_yield(e),
        StmtKind::Return(Some(e)) => expr_contains_yield(e),
        StmtKind::Return(None) => false,
        StmtKind::Echo(exprs) => exprs.iter().any(expr_contains_yield),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => expr_contains_yield(cond),
        StmtKind::For {
            init, cond, step, ..
        } => init.iter().chain(cond).chain(step).any(expr_contains_yield),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            expr_contains_yield(subject)
                || key.as_ref().is_some_and(expr_contains_yield)
                || expr_contains_yield(value)
        }
        StmtKind::GlobalConst(consts) => consts.iter().any(|c| expr_contains_yield(&c.value)),
        StmtKind::StaticVar(vars) => vars
            .iter()
            .any(|(_, d)| d.as_ref().is_some_and(expr_contains_yield)),
        StmtKind::Switch { subject, cases } => {
            expr_contains_yield(subject)
                || cases
                    .iter()
                    .any(|c| c.test.as_ref().is_some_and(expr_contains_yield))
        }
        _ => false,
    };
    own || any_stmt_child(stmt, &mut stmt_contains_yield)
}

#[cfg(test)]
mod tests;
