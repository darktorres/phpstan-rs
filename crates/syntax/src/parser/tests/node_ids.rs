//! Parsed statements and expressions reserve indices in their typed arenas.

use super::super::*;
use super::helpers::*;
use crate::visit::{Visitor, walk_expr, walk_stmt};

/// Every statement's and expression's id.
#[derive(Default)]
struct Ids {
    statements: Vec<(StmtId, crate::span::Span)>,
    expressions: Vec<(ExprId, crate::span::Span)>,
}

impl<'ast> Visitor<'ast> for Ids {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.statements.push((stmt.id, stmt.span));
        walk_stmt(self, stmt);
    }
    fn visit_expr(&mut self, e: &'ast Expr) {
        self.expressions.push((e.id, e.span));
        walk_expr(self, e);
    }
}

#[test]
fn statements_and_expressions_have_dense_typed_ids() {
    let parsed = crate::parse(
        r#"<?php
            $a = 1 + 2;
            if ($a > 1): echo "x"; elseif ($a): echo "y"; else: echo "z"; endif;
            echo "value {$a} and $b->c and $d[0] and {$e->f($a + 1)}";
            foreach ([1, 2] as $k => $v) { $a += $v; }
        "#,
    )
    .unwrap();
    let mut ids = Ids::default();
    crate::visit::walk_stmts(&mut ids, &parsed.stmts);
    ids.statements
        .sort_by_key(|(id, _)| u32::from(id.into_raw()));
    ids.expressions
        .sort_by_key(|(id, _)| u32::from(id.into_raw()));
    assert_eq!(parsed.stmt_spans.len(), ids.statements.len());
    assert_eq!(parsed.expr_spans.len(), ids.expressions.len());
    for (expected, (id, span)) in ids.statements.iter().enumerate() {
        assert_eq!(u32::from(id.into_raw()) as usize, expected);
        assert_eq!(parsed.stmt_span(*id), Some(*span));
    }
    for (expected, (id, span)) in ids.expressions.iter().enumerate() {
        assert_eq!(u32::from(id.into_raw()) as usize, expected);
        assert_eq!(parsed.expr_span(*id), Some(*span));
    }
}
