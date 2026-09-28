use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

fn parse(src: &str) -> Vec<Stmt> {
    Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .expect("parses")
}

/// The id of the first expression that is a variable named `name`.
fn var(stmts: &[Stmt], name: &str) -> NodeId {
    struct Find<'a>(&'a str, Option<NodeId>);
    impl<'ast> Visitor<'ast> for Find<'_> {
        fn visit_expr(&mut self, e: &'ast Expr) {
            if self.1.is_none()
                && let ExprKind::Variable(v) = &e.kind
                && **v == *self.0
            {
                self.1 = Some(e.id);
            }
            walk_expr(self, e);
        }
    }
    let mut find = Find(name, None);
    walk_stmts(&mut find, stmts);
    find.1.unwrap_or_else(|| panic!("${name} not found"))
}

/// The id of the first statement matching `pred`.
fn stmt(stmts: &[Stmt], pred: impl Fn(&Stmt) -> bool) -> NodeId {
    struct Find<F>(F, Option<NodeId>);
    impl<'ast, F: Fn(&Stmt) -> bool> Visitor<'ast> for Find<F> {
        fn visit_stmt(&mut self, s: &'ast Stmt) {
            if self.1.is_none() && (self.0)(s) {
                self.1 = Some(s.id);
            }
            walk_stmt(self, s);
        }
    }
    let mut find = Find(pred, None);
    walk_stmts(&mut find, stmts);
    find.1.expect("statement found")
}

#[test]
fn conditions_and_the_last_elseif_condition_are_marked() {
    let program = parse(
        "<?php if ($a) {} elseif ($b) {} elseif ($c) {}\nif ($d) {} elseif ($e) {} else {}\nwhile ($w) {} $t ? 1 : 2; $plain;",
    );
    let a = annotate(&program);
    for name in ["a", "b", "c", "d", "e", "w", "t"] {
        assert!(a.is_condition(var(&program, name)), "${name}");
    }
    assert!(!a.is_condition(var(&program, "plain")));
    assert!(a.is_last_condition(var(&program, "c")));
    for name in ["a", "b", "d", "e"] {
        assert!(!a.is_last_condition(var(&program, name)), "${name}");
    }
}

#[test]
fn a_statement_expression_is_marked_and_its_parts_are_not() {
    let program = parse("<?php foo($x); $y = $z;");
    let a = annotate(&program);
    let StmtKind::Expression(call) = &program[0].kind else {
        panic!();
    };
    assert!(a.is_statement_expression(call.id));
    assert!(!a.is_statement_expression(var(&program, "x")));
    assert_eq!(a.parent(var(&program, "x")), Some(call.id));
    assert_eq!(a.parent(call.id), Some(program[0].id));
}

#[test]
fn catch_types_cover_the_try_body_only_and_stop_at_a_closure_or_arrow_function() {
    let program = parse(
        "<?php try { $inTry; try { $inner; } catch (B $e) {} $f = function () { $inClosure; }; $g = fn () => $inArrow; } catch (A | \\C $e) { $inCatch; } finally { $inFinally; }",
    );
    let a = annotate(&program);
    let names = |v: &str| {
        a.catch_types(var(&program, v))
            .iter()
            .map(|n| n.written.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(names("inTry"), ["A", "\\C"]);
    assert_eq!(names("inner"), ["A", "\\C", "B"]);
    assert!(names("inClosure").is_empty());
    assert!(names("inArrow").is_empty());
    assert!(names("inCatch").is_empty());
    assert!(names("inFinally").is_empty());
}

#[test]
fn a_label_is_a_goto_target_only_in_its_own_function() {
    let program = parse(
        "<?php goto top; top: echo 1; unused: echo 2; function f() { other: echo 3; } function g() { goto other; }",
    );
    let a = annotate(&program);
    let label = |name: &str| {
        stmt(
            &program,
            |s| matches!(&s.kind, StmtKind::Label(l) if &**l == name),
        )
    };
    assert!(a.is_goto_target(label("top")));
    assert!(!a.is_goto_target(label("unused")));
    assert!(!a.is_goto_target(label("other")));
}

#[test]
fn the_files_first_statement_is_marked() {
    let program = parse("<?php declare(strict_types=1); declare(ticks=1);");
    let a = annotate(&program);
    assert!(a.is_first_statement(program[0].id));
    assert!(!a.is_first_statement(program[1].id));
}

#[test]
fn php_version_id_comparisons_are_read_either_way_round() {
    let program = parse(
        "<?php if (PHP_VERSION_ID >= 80100) {} if (80000 > \\PHP_VERSION_ID) {} if ($x > 1) {}",
    );
    let a = annotate(&program);
    let cond = |i: usize| match &program[i].kind {
        StmtKind::If(IfStmt { cond, .. }) => cond.id,
        _ => panic!(),
    };
    assert_eq!(
        a.version_check(cond(0)),
        Some(VersionCheck {
            op: BinOp::GtEq,
            version: 80100
        })
    );
    assert_eq!(
        a.version_check(cond(1)),
        Some(VersionCheck {
            op: BinOp::Lt,
            version: 80000
        })
    );
    assert_eq!(a.version_check(cond(2)), None);
}
