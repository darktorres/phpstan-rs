//! Node identity: every `Stmt` and `Expr` of one parsed file has its own id.

use super::super::*;
use super::helpers::*;
use crate::ast_walk::{expr_children, stmt_children, stmt_own_exprs};

fn collect_stmt(stmt: &Stmt, ids: &mut Vec<NodeId>) {
    ids.push(stmt.id);
    for e in stmt_own_exprs(stmt) {
        collect_expr(e, ids);
    }
    for s in stmt_children(stmt) {
        collect_stmt(s, ids);
    }
}

fn collect_expr(e: &Expr, ids: &mut Vec<NodeId>) {
    ids.push(e.id);
    for child in expr_children(e) {
        collect_expr(child, ids);
    }
}

#[test]
fn every_statement_and_expression_has_a_distinct_id() {
    let program = parse(
        r#"<?php
            $a = 1 + 2;
            if ($a > 1): echo "x"; elseif ($a): echo "y"; else: echo "z"; endif;
            echo "value {$a} and $b->c and $d[0] and {$e->f($a + 1)}";
            foreach ([1, 2] as $k => $v) { $a += $v; }
        "#,
    );
    let mut ids = Vec::new();
    for stmt in &program {
        collect_stmt(stmt, &mut ids);
    }
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count, "duplicate node ids");
    assert!(ids.iter().all(|id| *id != NodeId::SYNTHETIC));
}
