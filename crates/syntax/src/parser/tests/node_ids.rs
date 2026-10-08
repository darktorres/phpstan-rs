//! Parsed statements and expressions reserve indices in their typed arenas.

use super::super::*;
use crate::ast::NodeArenas;
use crate::visit::arena::{self, Visitor};

#[test]
fn parsed_file_and_node_ids_are_send_sync_and_copy() {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_copy<T: Copy>() {}

    assert_send_sync::<crate::ParsedFile>();
    assert_copy::<ExprId>();
    assert_copy::<StmtId>();
}

/// Every statement's and expression's id.
#[derive(Default)]
struct Ids {
    statements: Vec<(StmtId, crate::span::Span)>,
    expressions: Vec<(ExprId, crate::span::Span)>,
}

impl Visitor for Ids {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(_stmt) = arenas.stmt(id) else {
            return;
        };
        self.statements.push((
            id,
            arenas
                .stmt_span(id)
                .expect("visited statement has its parallel span"),
        ));
        arena::walk_stmt(self, arenas, id);
    }
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(_expr) = arenas.expr(id) else {
            return;
        };
        self.expressions.push((
            id,
            arenas
                .expr_span(id)
                .expect("visited expression has its parallel span"),
        ));
        arena::walk_expr(self, arenas, id);
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
    arena::walk_stmts(&mut ids, &parsed.arenas, parsed.statements());
    ids.statements
        .sort_by_key(|(id, _)| u32::from(id.into_raw()));
    ids.expressions
        .sort_by_key(|(id, _)| u32::from(id.into_raw()));
    assert_eq!(parsed.stmt_count(), ids.statements.len());
    assert_eq!(parsed.expr_count(), ids.expressions.len());
    for (expected, (id, span)) in ids.statements.iter().enumerate() {
        assert_eq!(u32::from(id.into_raw()) as usize, expected);
        assert_eq!(parsed.stmt_span(*id), Some(*span));
    }
    for (expected, (id, span)) in ids.expressions.iter().enumerate() {
        assert_eq!(u32::from(id.into_raw()) as usize, expected);
        assert_eq!(parsed.expr_span(*id), Some(*span));
    }
}

#[test]
fn reused_arenas_restart_ids_and_keep_every_node_span_aligned() {
    let first = crate::parse("<?php $old = 1; if ($old) { echo $old; }").unwrap();
    let mut scratch = first.into_node_arenas();
    let parsed = crate::parse_watching_with_scratch(
        "<?php $new = 2; if ($new) { echo $new; }",
        None,
        None,
        &mut scratch,
    )
    .unwrap();

    let mut ids = Ids::default();
    arena::walk_stmts(&mut ids, &parsed.arenas, parsed.statements());
    ids.statements
        .sort_by_key(|(id, _)| u32::from(id.into_raw()));
    ids.expressions
        .sort_by_key(|(id, _)| u32::from(id.into_raw()));
    assert_eq!(parsed.stmt_count(), ids.statements.len());
    assert_eq!(parsed.expr_count(), ids.expressions.len());
    for (expected, (id, span)) in ids.statements.iter().enumerate() {
        assert_eq!(u32::from(id.into_raw()) as usize, expected);
        assert_eq!(parsed.stmt_span(*id), Some(*span));
    }
    for (expected, (id, span)) in ids.expressions.iter().enumerate() {
        assert_eq!(u32::from(id.into_raw()) as usize, expected);
        assert_eq!(parsed.expr_span(*id), Some(*span));
    }
}
