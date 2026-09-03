//! Shared, exhaustively-matched "what are this node's direct children"
//! helpers for `Expr`/`Stmt` - built specifically so a full-tree walk (like
//! `deps::collect_expr`) doesn't need its own independently-hand-maintained
//! copy of this same recursion structure. Before this module existed, this
//! exact traversal shape was hand-written three separate times
//! (`checker::check_expr`, `checker::infer_expr`, `deps::collect_expr`) -
//! this session's own `Arg`/`ArrayItem` AST migrations each had to update
//! all three in lockstep purely because of that duplication.
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

use crate::ast::{Expr, ExprKind, InstanceofTarget, Stmt, StmtKind};

/// This `Expr`'s direct child expressions - see the module doc comment for
/// the three variants (`Closure`/`ArrowFn`/`NewAnonymousClass`) that
/// deliberately return none.
pub fn expr_children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Literal(_)
        | ExprKind::Variable(_)
        | ExprKind::StaticAccess(_, _)
        | ExprKind::Name(_) => {
            vec![]
        }
        ExprKind::DynamicVariable(inner)
        | ExprKind::Unary(_, inner)
        | ExprKind::Cast(_, inner)
        | ExprKind::Grouping(inner)
        | ExprKind::FirstClassCallable(inner)
        | ExprKind::Throw(inner)
        | ExprKind::YieldFrom(inner)
        | ExprKind::PropertyAccess(inner, _, _) => vec![inner],
        ExprKind::ArrayLiteral(items) => items
            .iter()
            .flat_map(|item| item.key.iter().chain(std::iter::once(&item.value)))
            .collect(),
        ExprKind::Binary(_, l, r)
        | ExprKind::Assign(l, r)
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
        ExprKind::StaticCall(_, _, args) => args.iter().map(|a| &a.value).collect(),
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

#[cfg(test)]
mod tests;
