//! Node identity: every `Stmt` and `Expr` of one parsed file has its own id.

use super::super::*;
use super::helpers::*;
use crate::visit::{Visitor, walk_expr, walk_stmt};

/// Every statement's and expression's id.
struct Ids(Vec<NodeId>);

impl<'ast> Visitor<'ast> for Ids {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.0.push(stmt.id);
        walk_stmt(self, stmt);
    }
    fn visit_expr(&mut self, e: &'ast Expr) {
        self.0.push(e.id);
        walk_expr(self, e);
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
    let mut ids = Ids(Vec::new());
    crate::visit::walk_stmts(&mut ids, &program);
    let mut ids = ids.0;
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count, "duplicate node ids");
    assert!(ids.iter().all(|id| *id != NodeId::SYNTHETIC));
}
