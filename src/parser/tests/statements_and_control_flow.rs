//! Parsing of statements and control-flow constructs: loops, conditionals, switch, break/continue, and alternative syntax forms.

use super::super::*;
use super::helpers::*;

#[test]
fn static_local_variable_declaration_parses() {
    let program = parse("<?php function f() { static $count = 0; return $count; } ");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert!(matches!(&f.body[0].kind, StmtKind::StaticVar(_)));
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
    assert!(matches!(&program[0].kind, StmtKind::Block(stmts) if stmts.is_empty()));
    assert!(matches!(&program[1].kind, StmtKind::Block(stmts) if !stmts.is_empty()));
}

#[test]
fn global_with_dynamic_target_parses() {
    let program = parse(r#"<?php global ${sf("a")};"#);
    assert!(matches!(&program[0].kind, StmtKind::Global(names) if names == &["{dynamic}"]));
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
    assert_eq!(cases.len(), 3);
    // `case 1:` falls through with no statements of its own - they
    // all attach to the following `case 2:` label, matching real
    // PHP's fallthrough semantics.
    assert!(cases[0].body.is_empty());
    assert_eq!(cases[1].body.len(), 2);
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
    assert!(matches!(&body.kind, StmtKind::Block(stmts) if stmts.len() == 1));
    assert!(matches!(&cond.kind, ExprKind::Binary(BinOp::Lt, ..)));
}

#[test]
fn if_alt_syntax_with_elseif_and_else_folds_into_nested_if() {
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
    let StmtKind::If {
        then_branch,
        else_branch,
        ..
    } = &program[0].kind
    else {
        panic!("expected If");
    };
    assert!(matches!(&then_branch.kind, StmtKind::Block(s) if s.len() == 1));
    let Some(else_branch) = else_branch else {
        panic!("expected an elseif chain, got no else branch at all");
    };
    // The `elseif` re-dispatches as a nested `if`, exactly like the
    // brace syntax's own elseif handling - one `endif;` still closes
    // the whole chain, not just the innermost `if`.
    let StmtKind::If {
        then_branch: elseif_then,
        else_branch: elseif_else,
        ..
    } = &else_branch.kind
    else {
        panic!("expected a nested If for the elseif branch");
    };
    assert!(matches!(&elseif_then.kind, StmtKind::Block(s) if s.len() == 1));
    assert!(
        matches!(elseif_else.as_deref(), Some(Stmt { kind: StmtKind::Block(s), .. }) if s.len() == 1)
    );
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
    assert!(matches!(&body.kind, StmtKind::Block(s) if s.len() == 2));
}
