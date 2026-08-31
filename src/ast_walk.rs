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
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse_expr(src: &str) -> Expr {
        let tokens = Lexer::new(&format!("<?php {src};")).tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        match program.into_iter().next().unwrap().kind {
            StmtKind::Expression(e) => e,
            other => panic!("expected an Expression statement, got {other:?}"),
        }
    }

    fn parse_stmt(src: &str) -> Stmt {
        let tokens = Lexer::new(&format!("<?php {src}")).tokenize_with_spans();
        Parser::new(tokens)
            .parse_program()
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn a_literal_has_no_children() {
        assert_eq!(expr_children(&parse_expr("42")).len(), 0);
    }

    #[test]
    fn binary_exposes_both_operands() {
        let e = parse_expr("$a + $b");
        assert_eq!(expr_children(&e).len(), 2);
    }

    #[test]
    fn array_literal_exposes_both_keys_and_values() {
        let e = parse_expr("['a' => 1, 2]");
        // 'a' key + 1 value, plus the bare positional 2 - 3 children total.
        assert_eq!(expr_children(&e).len(), 3);
    }

    #[test]
    fn ternary_exposes_condition_and_both_branches() {
        let full = parse_expr("$a ? $b : $c");
        assert_eq!(expr_children(&full).len(), 3);
        // The short-ternary `?:` form has no `then` branch to expose.
        let short = parse_expr("$a ?: $c");
        assert_eq!(expr_children(&short).len(), 2);
    }

    #[test]
    fn call_exposes_the_callee_and_every_argument() {
        let e = parse_expr("foo($a, $b, $c)");
        assert_eq!(expr_children(&e).len(), 4);
    }

    #[test]
    fn method_call_exposes_receiver_and_arguments() {
        let e = parse_expr("$obj->method($a)");
        assert_eq!(expr_children(&e).len(), 2);
    }

    #[test]
    fn index_with_no_key_exposes_only_the_base() {
        let e = parse_expr("$arr[]");
        assert_eq!(expr_children(&e).len(), 1);
    }

    #[test]
    fn index_with_a_key_exposes_base_and_key() {
        let e = parse_expr("$arr[$k]");
        assert_eq!(expr_children(&e).len(), 2);
    }

    #[test]
    fn instanceof_with_a_named_target_exposes_only_the_left_operand() {
        let e = parse_expr("$x instanceof Foo");
        assert_eq!(expr_children(&e).len(), 1);
    }

    #[test]
    fn instanceof_with_a_dynamic_target_exposes_both_operands() {
        let e = parse_expr("$x instanceof $className");
        assert_eq!(expr_children(&e).len(), 2);
    }

    #[test]
    fn match_exposes_subject_every_arm_condition_result_and_default() {
        let e = parse_expr("match ($x) { 1, 2 => 'a', default => 'b' }");
        // subject + (1, 2, 'a') + 'b' = 5
        assert_eq!(expr_children(&e).len(), 5);
    }

    #[test]
    fn closure_arrow_fn_and_new_anonymous_class_expose_no_automatic_children() {
        assert_eq!(
            expr_children(&parse_expr("function () use ($x) { return $x; }")).len(),
            0
        );
        assert_eq!(expr_children(&parse_expr("fn($x) => $x + 1")).len(), 0);
        assert_eq!(
            expr_children(&parse_expr("new class($a) { public $p; }")).len(),
            0
        );
    }

    #[test]
    fn a_bare_statement_expression_has_no_child_statements() {
        assert_eq!(stmt_children(&parse_stmt("$x = 1;")).len(), 0);
    }

    #[test]
    fn block_exposes_every_statement_in_order() {
        let s = parse_stmt("{ $a = 1; $b = 2; }");
        assert_eq!(stmt_children(&s).len(), 2);
    }

    #[test]
    fn if_with_no_else_exposes_only_the_then_branch() {
        let s = parse_stmt("if ($c) { $a = 1; }");
        assert_eq!(stmt_children(&s).len(), 1);
    }

    #[test]
    fn if_with_an_else_exposes_both_branches() {
        let s = parse_stmt("if ($c) { $a = 1; } else { $a = 2; }");
        assert_eq!(stmt_children(&s).len(), 2);
    }

    #[test]
    fn switch_flattens_every_case_bodys_own_statements() {
        let s = parse_stmt("switch ($x) { case 1: $a = 1; break; default: $a = 2; }");
        // case 1's body (assign + break) + default's body (assign) = 3
        assert_eq!(stmt_children(&s).len(), 3);
    }

    #[test]
    fn try_flattens_body_every_catch_body_and_finally() {
        let s = parse_stmt("try { $a = 1; } catch (Exception $e) { $a = 2; } finally { $a = 3; }");
        assert_eq!(stmt_children(&s).len(), 3);
    }

    #[test]
    fn nested_function_and_class_declarations_expose_no_automatic_children() {
        assert_eq!(
            stmt_children(&parse_stmt("function foo() { echo 1; }")).len(),
            0
        );
        assert_eq!(
            stmt_children(&parse_stmt("class Foo { public function bar() {} }")).len(),
            0
        );
        assert_eq!(stmt_children(&parse_stmt("enum Foo { case A; }")).len(), 0);
    }
}
