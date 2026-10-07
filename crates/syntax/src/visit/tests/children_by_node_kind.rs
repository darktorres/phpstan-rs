//! Tests for `any_child_expr` and `any_child_stmt`, covering each AST node kind's
//! set of exposed children.

use super::super::*;
use super::helpers::*;

fn child_expr_count(e: &ExprNode) -> usize {
    let mut count = 0;
    any_child_expr(e, |_| {
        count += 1;
        false
    });
    count
}

fn child_stmt_count(s: &StmtNode) -> usize {
    let mut count = 0;
    any_child_stmt(s, |_| {
        count += 1;
        false
    });
    count
}

#[test]
fn a_literal_has_no_children() {
    assert_eq!(child_expr_count(&parse_expr("42")), 0);
}

#[test]
fn binary_exposes_both_operands() {
    let e = parse_expr("$a + $b");
    assert_eq!(child_expr_count(&e), 2);
}

#[test]
fn array_literal_exposes_both_keys_and_values() {
    let e = parse_expr("['a' => 1, 2]");
    // 'a' key + 1 value, plus the bare positional 2 - 3 children total.
    assert_eq!(child_expr_count(&e), 3);
}

#[test]
fn ternary_exposes_condition_and_both_branches() {
    let full = parse_expr("$a ? $b : $c");
    assert_eq!(child_expr_count(&full), 3);
    // The short-ternary `?:` form has no `then` branch to expose.
    let short = parse_expr("$a ?: $c");
    assert_eq!(child_expr_count(&short), 2);
}

#[test]
fn call_exposes_the_callee_and_every_argument() {
    let e = parse_expr("foo($a, $b, $c)");
    assert_eq!(child_expr_count(&e), 4);
}

#[test]
fn method_call_exposes_receiver_and_arguments() {
    let e = parse_expr("$obj->method($a)");
    assert_eq!(child_expr_count(&e), 2);
}

#[test]
fn index_with_no_key_exposes_only_the_base() {
    let e = parse_expr("$arr[]");
    assert_eq!(child_expr_count(&e), 1);
}

#[test]
fn index_with_a_key_exposes_base_and_key() {
    let e = parse_expr("$arr[$k]");
    assert_eq!(child_expr_count(&e), 2);
}

#[test]
fn instanceof_with_a_named_target_exposes_only_the_left_operand() {
    let e = parse_expr("$x instanceof Foo");
    assert_eq!(child_expr_count(&e), 1);
}

#[test]
fn instanceof_with_a_dynamic_target_exposes_both_operands() {
    let e = parse_expr("$x instanceof $className");
    assert_eq!(child_expr_count(&e), 2);
}

#[test]
fn match_exposes_subject_every_arm_condition_result_and_default() {
    let e = parse_expr("match ($x) { 1, 2 => 'a', default => 'b' }");
    // subject + (1, 2, 'a') + 'b' = 5
    assert_eq!(child_expr_count(&e), 5);
}

/// A closure's and an arrow function's bodies are their own scope; an
/// anonymous class's constructor arguments are evaluated in the
/// enclosing one, its body is not.
#[test]
fn a_closure_and_an_arrow_fn_expose_no_children_and_an_anonymous_class_its_arguments() {
    assert_eq!(
        child_expr_count(&parse_expr("function () use ($x) { return $x; }")),
        0
    );
    assert_eq!(child_expr_count(&parse_expr("fn($x) => $x + 1")), 0);
    assert_eq!(
        child_expr_count(&parse_expr("new class($a) { public $p = 1; }")),
        1
    );
}

#[test]
fn a_bare_statement_expression_has_no_child_statements() {
    assert_eq!(child_stmt_count(&parse_stmt("$x = 1;")), 0);
}

#[test]
fn block_exposes_every_statement_in_order() {
    let s = parse_stmt("{ $a = 1; $b = 2; }");
    assert_eq!(child_stmt_count(&s), 2);
}

#[test]
fn if_with_no_else_exposes_only_the_then_branch() {
    let s = parse_stmt("if ($c) { $a = 1; }");
    assert_eq!(child_stmt_count(&s), 1);
}

#[test]
fn if_with_an_else_exposes_both_branches() {
    let s = parse_stmt("if ($c) { $a = 1; } else { $a = 2; }");
    assert_eq!(child_stmt_count(&s), 2);
}

#[test]
fn switch_flattens_every_case_bodys_own_statements() {
    let s = parse_stmt("switch ($x) { case 1: $a = 1; break; default: $a = 2; }");
    // case 1's body (assign + break) + default's body (assign) = 3
    assert_eq!(child_stmt_count(&s), 3);
}

#[test]
fn try_flattens_body_every_catch_body_and_finally() {
    let s = parse_stmt("try { $a = 1; } catch (Exception $e) { $a = 2; } finally { $a = 3; }");
    assert_eq!(child_stmt_count(&s), 3);
}

#[test]
fn nested_function_and_class_declarations_expose_no_automatic_children() {
    assert_eq!(
        child_stmt_count(&parse_stmt("function foo() { echo 1; }")),
        0
    );
    assert_eq!(
        child_stmt_count(&parse_stmt("class Foo { public function bar() {} }")),
        0
    );
    assert_eq!(child_stmt_count(&parse_stmt("enum Foo { case A; }")), 0);
}
