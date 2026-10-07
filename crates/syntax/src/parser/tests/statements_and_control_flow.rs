//! Parsing of statements and control-flow constructs: loops, conditionals, switch, break/continue, and alternative syntax forms.

use super::super::*;
use super::helpers::*;

#[test]
fn static_local_variable_declaration_parses() {
    let program = parse("<?php function f() { static $count = 0; return $count; } ");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    let f = program.function(*f);
    let StmtKind::StaticVar(vars) = &program.stmt(f.body_stmts(&program.arenas)[0]).kind else {
        panic!("expected StaticVar");
    };
    let [variable] = program.arenas.static_vars(vars.clone()) else {
        panic!("expected one static variable");
    };
    assert_eq!(variable.name.name, "count");
}

#[test]
fn goto_and_labels_parse() {
    let program = parse(
        r#"<?php
            goto label;
            echo "skipped";
            label:
            echo "here";
        "#,
    );
    assert!(matches!(&program[0].kind, StmtKind::Goto(n) if **n == *"label"));
    assert!(matches!(&program[2].kind, StmtKind::Label(n) if **n == *"label"));
}

#[test]
fn declare_simple_and_block_forms_parse() {
    let program = parse(
        r#"<?php
            declare(strict_types=1);
            declare(ticks=1) {
                echo "in block";
            }
        "#,
    );
    let StmtKind::Declare {
        directives,
        body: None,
    } = &program[0].kind
    else {
        panic!("expected declare directive statement");
    };
    let [directive] = program.arenas.declare_directives(directives.clone()) else {
        panic!("expected one declare directive");
    };
    assert_eq!(directive.name.name, "strict_types");
    let StmtKind::Declare {
        body: Some(body), ..
    } = &program[1].kind
    else {
        panic!("expected a declare block, got {:?}", program[1].kind);
    };
    assert!(matches!(
        &program.stmt(program.arenas.stmt_ids(body)[0]).kind,
        StmtKind::Echo(_)
    ));
    assert_eq!(program.len(), 2);
}

#[test]
fn global_with_dynamic_target_parses() {
    let program = parse(r#"<?php global ${sf("a")};"#);
    let StmtKind::Global(vars) = &program[0].kind else {
        panic!("expected Global");
    };
    assert!(matches!(
        program.arenas.global_vars(vars.clone()),
        [GlobalVar::Dynamic(_)]
    ));
}

// Locks in AST-level coverage for parse_switch, parse_do_while,
// parse_if_alt, parse_alt_block, and parse_optional_level - common PHP
// syntax (switch, do-while, the `if (...): ... endif;` alternative
// syntax used throughout template-heavy code, and `break N;`/
// `continue N;`).

#[test]
fn switch_with_brace_syntax_parses_cases_default_and_fallthrough() {
    let program = parse(
        r#"<?php
            switch ($x) {
                case 1:
                case 2:
                    doA();
                    break;
                default:
                    doB();
            }
        "#,
    );
    let StmtKind::Switch { cases, .. } = &program[0].kind else {
        panic!("expected Switch");
    };
    let cases = program.arenas.switch_cases(cases.clone());
    assert_eq!(cases.len(), 3);
    // `case 1:` falls through with no statements of its own - they
    // all attach to the following `case 2:` label, matching real
    // PHP's fallthrough semantics.
    assert!(program.arenas.stmt_ids(&cases[0].body).is_empty());
    assert_eq!(cases[1].body.len(), 2);
    let case_stmts = program.arenas.stmt_ids(&cases[1].body);
    assert!(matches!(
        &program.stmt(case_stmts[0]).kind,
        StmtKind::Expression(_)
    ));
    assert!(matches!(
        &program.stmt(case_stmts[1]).kind,
        StmtKind::Break(_)
    ));
    assert!(cases[2].test.is_none(), "expected the default case");
    assert_eq!(cases[2].body.len(), 1);
}

#[test]
fn switch_alt_syntax_parses_the_same_shape_as_brace_syntax() {
    let program = parse(
        r#"<?php
            switch ($x):
                case 1:
                    doA();
            endswitch;
        "#,
    );
    let StmtKind::Switch { cases, .. } = &program[0].kind else {
        panic!("expected Switch");
    };
    let cases = program.arenas.switch_cases(cases.clone());
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].body.len(), 1);
}

#[test]
fn do_while_parses_its_body_and_trailing_condition() {
    let program = parse(
        r#"<?php
            do {
                doSomething();
            } while ($i < 10);
        "#,
    );
    let StmtKind::DoWhile { body, cond } = &program[0].kind else {
        panic!("expected DoWhile");
    };
    assert!(matches!(&program.stmt(*body).kind, StmtKind::Block(stmts) if stmts.len() == 1));
    assert!(matches!(
        &program.expr(*cond).kind,
        ExprKind::Binary(BinOp::Lt, ..)
    ));
}

#[test]
fn if_alt_syntax_with_elseif_and_else_is_one_chain() {
    let program = parse(
        r#"<?php
            if ($a):
                doA();
            elseif ($b):
                doB();
            else:
                doC();
            endif;
        "#,
    );
    let StmtKind::If(if_stmt) = &program[0].kind else {
        panic!("expected If");
    };
    assert!(matches!(&program.stmt(if_stmt.then_branch).kind, StmtKind::Block(s) if s.len() == 1));
    let [elseif] = program.arenas.elseifs(if_stmt.elseifs.clone()) else {
        panic!(
            "expected one elseif, got {}",
            program.arenas.elseifs(if_stmt.elseifs.clone()).len()
        );
    };
    assert!(matches!(&program.stmt(elseif.body).kind, StmtKind::Block(s) if s.len() == 1));
    assert!(
        if_stmt
            .else_branch
            .is_some_and(|id| matches!(&program.stmt(id).kind, StmtKind::Block(s) if s.len() == 1))
    );
}

#[test]
fn elseif_is_part_of_the_chain_and_else_if_is_a_nested_if() {
    let program = parse("<?php if ($a) { } elseif ($b) { } else if ($c) { }");
    let StmtKind::If(if_stmt) = &program[0].kind else {
        panic!("expected If");
    };
    assert_eq!(program.arenas.elseifs(if_stmt.elseifs.clone()).len(), 1);
    assert!(if_stmt.else_branch.is_some_and(
        |id| matches!(&program.stmt(id).kind, StmtKind::If(nested) if program.arenas.elseifs(nested.elseifs.clone()).is_empty())
    ));
}

#[test]
fn break_and_continue_parse_their_optional_numeric_level() {
    let program = parse(
        r#"<?php
            break;
            break 2;
            continue;
            continue 3;
        "#,
    );
    assert!(matches!(&program[0].kind, StmtKind::Break(None)));
    assert!(matches!(&program[1].kind, StmtKind::Break(Some(2))));
    assert!(matches!(&program[2].kind, StmtKind::Continue(None)));
    assert!(matches!(&program[3].kind, StmtKind::Continue(Some(3))));
}

#[test]
fn break_and_continue_parse_a_parenthesized_level() {
    // Real PHP's own grammar is `T_BREAK optional_expr ';'`, not just a
    // bare integer literal - `continue (2);` is real code (dompdf's own
    // `Stylesheet::_parse_css`, a `switch` nested in a `while`) that used
    // to abort the whole file with a parse error, since the parenthesized
    // form was previously indistinguishable from "no level" immediately
    // followed by an unexpected `(`.
    let program = parse(
        r#"<?php
            break (1);
            continue (2);
            break (-1);
        "#,
    );
    assert!(matches!(&program[0].kind, StmtKind::Break(Some(1))));
    assert!(matches!(&program[1].kind, StmtKind::Continue(Some(2))));
    assert!(matches!(&program[2].kind, StmtKind::Break(Some(-1))));
}

#[test]
fn foreach_alt_syntax_parses_its_body_via_parse_alt_block() {
    // parse_if_alt and parse_switch each parse their own alt-syntax
    // body inline - parse_alt_block is the shared path only
    // while/for/foreach's alt syntax (`endwhile;`/`endfor;`/
    // `endforeach;`) goes through, and none of the tests above
    // happen to exercise it.
    let program = parse(
        r#"<?php
            foreach ($items as $item):
                doA();
                doB();
            endforeach;
        "#,
    );
    let StmtKind::Foreach { body, .. } = &program[0].kind else {
        panic!("expected Foreach");
    };
    assert!(matches!(&program.stmt(*body).kind, StmtKind::Block(s) if s.len() == 2));
}

#[test]
fn isset_empty_and_unset_are_their_own_nodes() {
    let program = parse("<?php\n$a = isset($x, $y['k']) || empty($z);\nunset($x, $y['k']);\n");
    let StmtKind::Expression(assignment) = &program[0].kind else {
        panic!("expected an assignment, got {:?}", program[0].kind);
    };
    let ExprKind::Assign(_, rhs) = &program.expr(*assignment).kind else {
        panic!("expected assignment expression");
    };
    let ExprKind::Binary(_, isset, empty) = &program.expr(*rhs).kind else {
        panic!("expected a binary expression");
    };
    assert!(matches!(
        &program.expr(*isset).kind,
        ExprKind::Isset(operands) if program.arenas.expr_ids(operands).len() == 2
    ));
    assert!(matches!(&program.expr(*empty).kind, ExprKind::Empty(_)));
    assert!(matches!(
        &program[1].kind,
        StmtKind::Unset(targets) if program.arenas.expr_ids(targets).len() == 2
    ));
}

#[test]
fn list_is_apart_from_an_array_literal() {
    let program = parse("<?php\nlist($a, list($b)) = [1, [2]];\n");
    let StmtKind::Expression(assignment) = &program[0].kind else {
        panic!("expected an assignment, got {:?}", program[0].kind);
    };
    let ExprKind::Assign(lhs, rhs) = &program.expr(*assignment).kind else {
        panic!("expected assignment expression");
    };
    let ExprKind::Destructure(items, DestructureSyntax::List) = &program.expr(*lhs).kind else {
        panic!("expected a list pattern");
    };
    let items = program.arenas.destructure_items(items);
    assert!(matches!(
        &program.expr(items[1].target.expect("nested target")).kind,
        ExprKind::Destructure(_, DestructureSyntax::List)
    ));
    assert!(matches!(
        &program.expr(*rhs).kind,
        ExprKind::ArrayLiteral(_)
    ));
}

#[test]
fn error_suppression_is_kept_as_a_node() {
    let program = parse("<?php\n$f = @fopen('x', 'r');\n");
    let StmtKind::Expression(assignment) = &program[0].kind else {
        panic!("expected an assignment, got {:?}", program[0].kind);
    };
    let ExprKind::Assign(_, rhs) = &program.expr(*assignment).kind else {
        panic!("expected assignment expression");
    };
    let ExprKind::ErrorSuppress(inner) = &program.expr(*rhs).kind else {
        panic!("expected an error suppression");
    };
    assert!(matches!(&program.expr(*inner).kind, ExprKind::Call(..)));
    let peeled = program.arenas.peeled(*rhs).expect("peeled expression");
    assert!(matches!(&program.expr(peeled).kind, ExprKind::Call(..)));
}

#[test]
fn match_arms_are_spanned_with_the_default_in_its_place() {
    let src = "<?php $r = match ($x) { default => 0, 1, 2 => 1 };";
    let program = parse(src);
    let StmtKind::Expression(assignment) = &program[0].kind else {
        panic!("expected an assignment, got {:?}", program[0].kind);
    };
    let ExprKind::Assign(_, rhs) = &program.expr(*assignment).kind else {
        panic!("expected assignment expression");
    };
    let ExprKind::Match { arms, .. } = &program.expr(*rhs).kind else {
        panic!("expected a match expression");
    };
    let arms = program.arenas.match_arms(arms.clone());
    assert!(arms[0].conditions.is_none());
    assert_eq!(
        arms[1]
            .conditions
            .as_ref()
            .map(|conditions| program.arenas.expr_ids(conditions).len()),
        Some(2)
    );
    assert_eq!(
        (
            position(src, arms[1].span.start).col,
            position(src, arms[1].span.end).col
        ),
        (39, 48)
    );
    assert!(match_default(arms).is_some());
}
