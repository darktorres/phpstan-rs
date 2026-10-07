use super::super::*;
use super::helpers::text_at;

/// `expr` as nested parentheses over its binary operators, variables as
/// their names.
fn grouped(arenas: &NodeArenas, id: ExprId) -> String {
    let expr = arenas
        .expr(id)
        .expect("expression id belongs to parsed tree");
    match &expr.kind {
        ExprKind::Binary(op, left, right) => {
            format!(
                "({} {op:?} {})",
                grouped(arenas, *left),
                grouped(arenas, *right)
            )
        }
        ExprKind::Variable(name) => name.to_string(),
        other => panic!("not a binary or variable: {other:?}"),
    }
}

fn statement_expr(src: &str) -> crate::ParsedFile {
    crate::parse(src.as_bytes()).expect("parses")
}

fn grouping_of(source: &str) -> String {
    let parsed = statement_expr(&format!("<?php {source};"));
    let stmt = parsed
        .arenas
        .stmt(parsed.statements()[0])
        .expect("statement");
    match &stmt.kind {
        StmtKind::Expression(expr) => grouped(&parsed.arenas, *expr),
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
        "((((((((((((a Mul b) Add c) Shl d) Concat e) Pipe f) Lt g) Eq h) BitAnd i) BitXor j) BitOr k) And l) Or m)"
    );
}

#[test]
fn operators_of_one_level_group_to_the_left() {
    assert_eq!(grouping_of("$a - $b - $c"), "((a Sub b) Sub c)");
    assert_eq!(
        grouping_of("$a / $b % $c * $d"),
        "(((a Div b) Mod c) Mul d)"
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
    let parsed = statement_expr(src);
    let stmt = parsed
        .arenas
        .stmt(parsed.statements()[0])
        .expect("statement");
    let StmtKind::Expression(expr) = &stmt.kind else {
        panic!("an expression statement");
    };
    assert_eq!(
        text_at(src, parsed.arenas.expr_span(*expr).unwrap()),
        "$a + $b * $c - $d"
    );
    let expression = parsed.arenas.expr(*expr).expect("expression");
    let ExprKind::Binary(_, left, _) = &expression.kind else {
        panic!("a binary");
    };
    assert_eq!(
        text_at(src, parsed.arenas.expr_span(*left).unwrap()),
        "$a + $b * $c"
    );
    let left_expr = parsed.arenas.expr(*left).expect("left expression");
    let ExprKind::Binary(_, _, product) = &left_expr.kind else {
        panic!("a binary");
    };
    assert_eq!(
        text_at(src, parsed.arenas.expr_span(*product).unwrap()),
        "$b * $c"
    );
}

#[test]
fn equality_and_comparison_do_not_chain() {
    // Neither is associative in PHP: each of these is a syntax error.
    for source in [
        "$a == $b != $c",
        "$a === $b == $c",
        "$a < $b > $c",
        "$a <= $b < $c",
    ] {
        let tokens = crate::lexer::Lexer::new(&format!("<?php {source};")).tokenize_with_spans();
        assert!(
            crate::parser::Parser::new(tokens).parse_program().is_err(),
            "{source}"
        );
    }
}
