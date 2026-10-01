use super::super::*;
use super::helpers::{parse, text_at};

/// `expr` as nested parentheses over its binary operators, variables as
/// their names.
fn grouped(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Binary(op, left, right) => {
            format!("({} {op:?} {})", grouped(left), grouped(right))
        }
        ExprKind::Variable(name) => name.to_string(),
        other => panic!("not a binary or variable: {other:?}"),
    }
}

fn statement_expr(src: &str) -> (Vec<Stmt>, usize) {
    (parse(src), 0)
}

fn grouping_of(source: &str) -> String {
    let (stmts, at) = statement_expr(&format!("<?php {source};"));
    match &stmts[at].kind {
        StmtKind::Expression(expr) => grouped(expr),
        other => panic!("not an expression statement: {other:?}"),
    }
}

#[test]
fn each_operator_binds_tighter_than_the_one_above_it_in_the_table() {
    // Loosest to tightest: || && | ^ & == < |> . << + *
    assert_eq!(
        grouping_of("$a || $b && $c | $d ^ $e & $f == $g < $h |> $i . $j << $k + $l * $m"),
        "(a Or (b And (c BitOr (d BitXor (e BitAnd (f Eq (g Lt (h Pipe (i Concat (j Shl (k Add (l Mul m))))))))))))"
    );
    assert_eq!(
        grouping_of("$a * $b + $c << $d . $e |> $f < $g == $h & $i ^ $j | $k && $l || $m"),
        "(((((((((((a Mul b) Add c) Shl d) Concat e) Pipe f) Lt g) Eq h) BitAnd i) BitXor j) BitOr k) And l) Or m)"
    );
}

#[test]
fn operators_of_one_level_group_to_the_left() {
    assert_eq!(grouping_of("$a - $b - $c"), "((a Sub b) Sub c)");
    assert_eq!(
        grouping_of("$a / $b % $c * $d"),
        "(((a Div b) Mod c) Mul d)"
    );
    assert_eq!(
        grouping_of("$a == $b != $c === $d"),
        "(((a Eq b) NotEq c) Identical d)"
    );
    assert_eq!(grouping_of("$a << $b >> $c"), "((a Shl b) Shr c)");
    assert_eq!(grouping_of("$a . $b . $c"), "((a Concat b) Concat c)");
    assert_eq!(grouping_of("$a || $b || $c"), "((a Or b) Or c)");
}

#[test]
fn concat_is_looser_than_arithmetic() {
    assert_eq!(grouping_of("$a . $x + $b"), "(a Concat (x Add b))");
    assert_eq!(grouping_of("$a + $x . $b"), "((a Add x) Concat b)");
}

#[test]
fn a_binary_expressions_span_runs_from_its_first_operand_to_its_last() {
    let src = "<?php $a + $b * $c - $d;";
    let stmts = parse(src);
    let StmtKind::Expression(expr) = &stmts[0].kind else {
        panic!("an expression statement");
    };
    assert_eq!(text_at(src, expr.span), "$a + $b * $c - $d");
    let ExprKind::Binary(_, left, _) = &expr.kind else {
        panic!("a binary");
    };
    assert_eq!(text_at(src, left.span), "$a + $b * $c");
    let ExprKind::Binary(_, _, product) = &left.kind else {
        panic!("a binary");
    };
    assert_eq!(text_at(src, product.span), "$b * $c");
}
